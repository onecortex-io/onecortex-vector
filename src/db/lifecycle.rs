use sqlx::PgPool;
use uuid::Uuid;

/// Create the schema, records table, and all indexes for a new collection.
///
/// Called synchronously during POST /collections. On an empty table, all DDL
/// including DiskANN index creation is instantaneous.
///
/// Schema name format: "col_{uuid_simple}" e.g. "col_550e8400e29b41d4a7160446"
#[allow(clippy::too_many_arguments)]
pub async fn create_collection_schema(
    pool: &PgPool,
    collection_id: Uuid,
    schema_name: &str,
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
    // schema_name is generated internally (UUID-based), never from user input directly.
    // dimension and metric are validated by the handler before reaching here.

    let create_schema = format!("CREATE SCHEMA IF NOT EXISTS {schema_name}");

    // NOTE: text_content column is included from the start even though Phase 3 activates BM25.
    // Adding columns to large tables later requires a full table rewrite. Adding it now
    // to an empty table has zero cost and avoids that future pain.
    //
    // NOTE: sparse_values column is NOT present — see docs/implementation/00-reference.md §8.
    let create_table = format!(
        r#"
        CREATE TABLE {schema_name}.records (
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

    // StreamingDiskANN index. See 00-reference.md §6 for parameter details.
    // num_neighbors default is 50. See 00-reference.md §6.
    // Column name is `values`.
    let create_diskann = format!(
        r#"
        CREATE INDEX {schema_name}_diskann_idx
            ON {schema_name}.records
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
        CREATE INDEX {schema_name}_metadata_gin_idx
            ON {schema_name}.records
            USING GIN (metadata jsonb_path_ops)
    "#
    );

    // B-tree index on namespace for fast namespace scans
    let create_ns_idx = format!(
        r#"
        CREATE INDEX {schema_name}_namespace_idx
            ON {schema_name}.records (namespace)
    "#
    );

    // Execute all DDL in a transaction
    let mut tx = pool.begin().await?;

    sqlx::query(&create_schema).execute(&mut *tx).await?;
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
            CREATE INDEX {schema_name}_bm25_idx
                ON {schema_name}.records
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
        schema_name,
        dimension,
        metric,
        "Collection schema created successfully"
    );

    Ok(())
}

/// Drop the schema for a collection, removing all records and indexes.
///
/// Called during DELETE /collections/:name after setting status = 'deleting'.
/// DROP SCHEMA CASCADE removes all tables and indexes in the schema.
pub async fn drop_collection_schema(
    pool: &PgPool,
    collection_id: Uuid,
    schema_name: &str,
) -> Result<(), sqlx::Error> {
    let drop_schema = format!("DROP SCHEMA IF EXISTS {schema_name} CASCADE");

    let mut tx = pool.begin().await?;

    sqlx::query(&drop_schema).execute(&mut *tx).await?;

    sqlx::query("DELETE FROM _onecortex_vector.collections WHERE id = $1")
        .bind(collection_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    tracing::info!(collection_id = %collection_id, schema_name, "Collection schema dropped");
    Ok(())
}

/// Generate a schema name from a UUID.
/// Format: "col_{uuid_simple}" — UUID without hyphens, lowercase.
pub fn schema_name_for(collection_id: Uuid) -> String {
    format!("col_{}", collection_id.simple())
}

/// Builds (or rebuilds) the BM25 index on an existing schema.
/// Called when PATCH /collections/:name sets bm25_enabled=true on an existing collection.
pub async fn build_bm25_index(pool: &PgPool, schema_name: &str) -> Result<(), sqlx::Error> {
    // DROP first in case a partial index exists from a previous failed attempt.
    sqlx::query(&format!(
        "DROP INDEX IF EXISTS {schema_name}.{schema_name}_bm25_idx"
    ))
    .execute(pool)
    .await?;

    sqlx::query(&format!(
        r#"
        CREATE INDEX {schema_name}_bm25_idx
            ON {schema_name}.records
            USING bm25 (text_content)
            WITH (text_config = 'english')
        "#
    ))
    .execute(pool)
    .await?;

    tracing::info!(schema_name, "BM25 index built successfully");
    Ok(())
}

/// Drops only the BM25 index (when bm25_enabled is toggled off via PATCH).
pub async fn drop_bm25_index(pool: &PgPool, schema_name: &str) -> Result<(), sqlx::Error> {
    sqlx::query(&format!(
        "DROP INDEX IF EXISTS {schema_name}.{schema_name}_bm25_idx"
    ))
    .execute(pool)
    .await?;

    tracing::info!(schema_name, "BM25 index dropped");
    Ok(())
}
