use sqlx::PgPool;
use uuid::Uuid;

/// Generate a table name from a collection UUID.
/// Format: "col_{uuid_simple}" — UUID without hyphens, lowercase.
/// The table lives in the _onecortex schema.
pub fn table_name_for(collection_id: Uuid) -> String {
    format!("col_{}", collection_id.simple())
}

/// Create the records table and all indexes for a new collection inside _onecortex schema.
///
/// Called synchronously during POST /collections. On an empty table, all DDL
/// including DiskANN index creation is instantaneous.
#[allow(clippy::too_many_arguments)]
pub async fn create_collection_table(
    pool: &PgPool,
    collection_id: Uuid,
    dimension: i32,
    metric: &str,
    diskann_neighbors: u32,
    diskann_search_list: u32,
    bm25_enabled: bool,
) -> Result<(), sqlx::Error> {
    // Operator class is determined by metric.
    // See docs/implementation/00-reference.md §5.
    let ops_class = match metric {
        "cosine" => "vector_cosine_ops",
        "euclidean" => "vector_l2_ops",
        "dotproduct" => "vector_ip_ops",
        _ => return Err(sqlx::Error::Protocol(format!("Unknown metric: {metric}"))),
    };

    // DDL cannot use parameterized queries in sqlx — use format!() with validated inputs.
    // table_name is generated internally (UUID-based), never from user input directly.
    // dimension and metric are validated by the handler before reaching here.
    let table_name = table_name_for(collection_id);

    let create_table = format!(
        r#"
        CREATE TABLE _onecortex.{table_name} (
            id           TEXT        NOT NULL CHECK (char_length(id) <= 512),
            namespace    TEXT        NOT NULL DEFAULT '',
            values       VECTOR({dimension}),
            text_content TEXT,
            metadata     JSONB,
            created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
            updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY  (id, namespace)
        )
    "#
    );

    // StreamingDiskANN index. num_neighbors default is 50.
    let create_diskann = format!(
        r#"
        CREATE INDEX {table_name}_diskann_idx
            ON _onecortex.{table_name}
            USING diskann (values {ops_class})
            WITH (
                num_neighbors    = {diskann_neighbors},
                search_list_size = {diskann_search_list}
            )
    "#
    );

    // GIN index for metadata JSONB filtering ($eq, $in, $gt etc.)
    let create_gin = format!(
        r#"
        CREATE INDEX {table_name}_metadata_gin_idx
            ON _onecortex.{table_name}
            USING GIN (metadata jsonb_path_ops)
    "#
    );

    // B-tree index on namespace for fast namespace scans
    let create_ns_idx = format!(
        r#"
        CREATE INDEX {table_name}_namespace_idx
            ON _onecortex.{table_name} (namespace)
    "#
    );

    // Execute all DDL in a transaction
    let mut tx = pool.begin().await?;

    sqlx::query(&create_table).execute(&mut *tx).await?;
    sqlx::query(&create_diskann).execute(&mut *tx).await?;
    sqlx::query(&create_gin).execute(&mut *tx).await?;
    sqlx::query(&create_ns_idx).execute(&mut *tx).await?;

    // BM25 index (only if enabled).
    // OPERATOR REFERENCE (from reference/pg_textsearch/):
    //   <@>   returns a NEGATIVE BM25 score. Negate it when computing relevance.
    //   Syntax: USING bm25(column) WITH (text_config = 'english')
    if bm25_enabled {
        let create_bm25 = format!(
            r#"
            CREATE INDEX {table_name}_bm25_idx
                ON _onecortex.{table_name}
                USING bm25 (text_content)
                WITH (text_config = 'english')
            "#
        );
        sqlx::query(&create_bm25).execute(&mut *tx).await?;
    }

    // Mark collection as ready
    sqlx::query(
        "UPDATE _onecortex_vector.collections SET status = 'ready', updated_at = now() WHERE id = $1",
    )
    .bind(collection_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    tracing::info!(
        collection_id = %collection_id,
        table_name,
        dimension,
        metric,
        "Collection table created successfully"
    );

    Ok(())
}

/// Drop the records table for a collection, removing all records and indexes.
///
/// Called during DELETE /collections/:name after setting status = 'deleting'.
/// DROP TABLE CASCADE removes all indexes on the table.
pub async fn drop_collection_table(pool: &PgPool, collection_id: Uuid) -> Result<(), sqlx::Error> {
    let table_name = table_name_for(collection_id);
    let drop_table = format!("DROP TABLE IF EXISTS _onecortex.{table_name} CASCADE");

    let mut tx = pool.begin().await?;

    sqlx::query(&drop_table).execute(&mut *tx).await?;

    sqlx::query("DELETE FROM _onecortex_vector.collections WHERE id = $1")
        .bind(collection_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    tracing::info!(collection_id = %collection_id, table_name, "Collection table dropped");
    Ok(())
}

/// Builds (or rebuilds) the BM25 index on an existing collection table.
/// Called when PATCH /collections/:name sets bm25_enabled=true on an existing collection.
pub async fn build_bm25_index(pool: &PgPool, table_name: &str) -> Result<(), sqlx::Error> {
    // DROP first in case a partial index exists from a previous failed attempt.
    sqlx::query(&format!(
        "DROP INDEX IF EXISTS _onecortex.{table_name}_bm25_idx"
    ))
    .execute(pool)
    .await?;

    sqlx::query(&format!(
        r#"
        CREATE INDEX {table_name}_bm25_idx
            ON _onecortex.{table_name}
            USING bm25 (text_content)
            WITH (text_config = 'english')
        "#
    ))
    .execute(pool)
    .await?;

    tracing::info!(table_name, "BM25 index built successfully");
    Ok(())
}

/// Run VACUUM ANALYZE on a collection table to reclaim space and refresh planner statistics.
///
/// VACUUM cannot run inside a transaction block; executing directly on the pool uses
/// auto-commit mode, which is correct here.
pub async fn vacuum_collection(pool: &PgPool, table_ref: &str) -> Result<(), sqlx::Error> {
    // table_ref is always internally generated (UUID-based), never raw user input
    sqlx::query(&format!("VACUUM ANALYZE {table_ref}"))
        .execute(pool)
        .await?;
    tracing::info!(table_ref, "VACUUM ANALYZE complete");
    Ok(())
}

/// Drop and recreate the DiskANN (StreamingDiskANN) index for a collection.
///
/// Uses CONCURRENTLY so reads are not blocked during the rebuild.
/// Both DROP and CREATE CONCURRENTLY must run outside a transaction block;
/// executing directly on the pool uses auto-commit mode, which is correct here.
pub async fn rebuild_diskann_index(
    pool: &PgPool,
    table_name: &str, // "col_{uuid_simple}" — no schema prefix
    metric: &str,
) -> Result<(), sqlx::Error> {
    let ops_class = match metric {
        "cosine" => "vector_cosine_ops",
        "euclidean" => "vector_l2_ops",
        "dotproduct" => "vector_ip_ops",
        _ => {
            return Err(sqlx::Error::Protocol(format!(
                "Unknown metric for reindex: {metric}"
            )))
        }
    };
    let idx = format!("{table_name}_diskann_idx");

    sqlx::query(&format!("DROP INDEX IF EXISTS _onecortex.{idx}"))
        .execute(pool)
        .await?;

    // num_neighbors=50 is the project default (CLAUDE.md). search_list_size=75 is the
    // StreamingDiskANN recommended default. Neither is stored in the collections catalog,
    // so project defaults are used here.
    sqlx::query(&format!(
        "CREATE INDEX CONCURRENTLY {idx} \
         ON _onecortex.{table_name} \
         USING diskann (values {ops_class}) \
         WITH (num_neighbors = 50, search_list_size = 75)"
    ))
    .execute(pool)
    .await?;

    tracing::info!(table_name, "DiskANN index rebuilt");
    Ok(())
}

/// Drops only the BM25 index (when bm25_enabled is toggled off via PATCH).
pub async fn drop_bm25_index(pool: &PgPool, table_name: &str) -> Result<(), sqlx::Error> {
    sqlx::query(&format!(
        "DROP INDEX IF EXISTS _onecortex.{table_name}_bm25_idx"
    ))
    .execute(pool)
    .await?;

    tracing::info!(table_name, "BM25 index dropped");
    Ok(())
}
