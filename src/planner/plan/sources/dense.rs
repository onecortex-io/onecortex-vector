//! Dense single-leg ANN source.
//!
//! Extracted verbatim from the inline SQL that used to live in
//! `handlers::query::query_vectors`. Owns nothing about stages — it returns
//! `retrieve_k` candidates with distance→score conversion already applied.

use sqlx::{PgPool, Row};

use crate::error::ApiError;
use crate::handlers::query::Match;
use crate::handlers::records::{parse_pgvector_str, CollectionMeta};
use crate::planner::filter_translator::translate_filter;

/// Run a dense ANN query against `collection.table_ref()`.
///
/// `fetch_metadata` should be true if any later stage needs metadata
/// (rerank, dedup, group_by) even when the caller asked `include_metadata=false`.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    pool: &PgPool,
    collection: &CollectionMeta,
    vector: &[f32],
    retrieve_k: i64,
    namespace: &str,
    filter: &Option<serde_json::Value>,
    fetch_values: bool,
    fetch_metadata: bool,
) -> Result<Vec<Match>, ApiError> {
    let vec_str = format!(
        "[{}]",
        vector
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    // Distance operator — must match the DiskANN index. See 00-reference.md §5.
    let dist_op = match collection.metric.as_str() {
        "cosine" => "<=>",
        "euclidean" => "<->",
        "dotproduct" => "<#>",
        _ => "<=>",
    };

    let (filter_sql, filter_params) = if let Some(f) = filter {
        translate_filter(f, 3)?
    } else {
        ("TRUE".to_string(), vec![])
    };

    let values_col = if fetch_values {
        "values::text"
    } else {
        "NULL::text AS values"
    };
    let metadata_col = if fetch_metadata {
        "metadata"
    } else {
        "NULL::jsonb AS metadata"
    };

    // CRITICAL: ORDER BY must use the IDENTICAL operator expression as the
    // SELECT distance column. Using an alias defeats DiskANN index usage.
    let sql = format!(
        r#"
        SELECT id, {values_col}, {metadata_col},
               values {dist_op} $1::vector AS distance
        FROM {table}
        WHERE namespace = $2
          AND ({filter_sql})
        ORDER BY values {dist_op} $1::vector
        LIMIT $3
        "#,
        table = collection.table_ref()
    );

    let mut q = sqlx::query(&sql)
        .bind(&vec_str)
        .bind(namespace)
        .bind(retrieve_k);
    for p in &filter_params {
        q = match p {
            serde_json::Value::String(s) => q.bind(s.as_str()),
            _ => q.bind(p.to_string()),
        };
    }

    let rows = q.fetch_all(pool).await?;

    let metric = collection.metric.as_str();
    Ok(rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            let distance: f64 = row.get("distance");
            let values_str: Option<String> = row.get("values");
            let metadata: Option<serde_json::Value> = row.get("metadata");

            let score = match metric {
                "cosine" => 1.0 - distance,
                "euclidean" => 1.0 / (1.0 + distance),
                "dotproduct" => -distance,
                _ => 1.0 - distance,
            };

            Match {
                id,
                score,
                values: values_str.map(|s| parse_pgvector_str(&s)),
                metadata,
            }
        })
        .collect())
}
