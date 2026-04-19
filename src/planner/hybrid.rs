use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::planner::filter_translator::translate_filter;

#[derive(Debug, Deserialize)]
pub struct HybridQueryRequest {
    pub vector: Vec<f32>,
    pub text: String,
    #[serde(rename = "topK")]
    pub top_k: i64,
    #[serde(default = "default_alpha")]
    pub alpha: f32,
    pub filter: Option<serde_json::Value>,
    #[serde(default)]
    pub namespace: String,
    #[serde(rename = "includeMetadata", default)]
    pub include_metadata: bool,
    #[serde(rename = "includeValues", default)]
    pub include_values: bool,
    /// If present, reranking is performed after RRF fusion.
    pub rerank: Option<crate::handlers::query::RerankOptions>,
    #[serde(rename = "scoreThreshold")]
    pub score_threshold: Option<f64>,
}

fn default_alpha() -> f32 {
    0.5
}

#[derive(Debug, Serialize)]
pub struct HybridMatch {
    pub id: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct HybridQueryResponse {
    pub matches: Vec<HybridMatch>,
    pub namespace: String,
}

/// Executes a hybrid ANN + BM25 query with Reciprocal Rank Fusion.
///
/// RRF Formula: score(d) = alpha / (k + dense_rank) + (1 - alpha) / (k + bm25_rank)
/// where k = 60 (standard constant).
///
/// Both legs retrieve `top_k * 6` candidates to ensure sufficient overlap for RRF.
///
/// BM25 NOTE: The <@> operator returns NEGATIVE scores. We negate the result so
/// that higher relevance = higher positive number, then rank descending.
pub async fn hybrid_query(
    pool: &PgPool,
    table_ref: &str,
    req: &HybridQueryRequest,
    metric: &str,
) -> Result<HybridQueryResponse, crate::error::ApiError> {
    let candidate_k = (req.top_k * 6).min(10_000);
    let rrf_k: f64 = 60.0;
    let alpha = (req.alpha as f64).clamp(0.0, 1.0);
    let bm25_weight = 1.0 - alpha;

    // Distance operator — must match the DiskANN index (see 00-reference.md section 5).
    let dist_op = match metric {
        "cosine" => "<=>",
        "euclidean" => "<->",
        "dotproduct" => "<#>",
        _ => "<=>",
    };

    // Build filter clause. Parameters $1-$5 are reserved:
    //   $1 = vector, $2 = namespace, $3 = candidate_k, $4 = text query, $5 = top_k
    // Filter params start at $6 (param_offset=5).
    let (filter_sql, filter_params) = if let Some(f) = &req.filter {
        translate_filter(f, 5)
            .map_err(|e| crate::error::ApiError::InvalidArgument(e.to_string()))?
    } else {
        ("TRUE".to_string(), vec![])
    };

    // Build vector string literal for SQL casting
    let vec_str = format!(
        "[{}]",
        req.vector
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    // Build the RRF SQL with three CTEs.
    // Alpha, bm25_weight, and rrf_k are server-computed floats interpolated via format!().
    // Table reference and distance operator are also interpolated (validated internally).
    let sql = format!(
        r#"
        WITH ann_results AS (
            SELECT
                v.id,
                v.namespace,
                v.metadata,
                v.values::text AS values_text,
                ROW_NUMBER() OVER (
                    ORDER BY v.values {dist_op} $1::vector
                ) AS dense_rank
            FROM {table_ref} v
            WHERE v.namespace = $2
              AND ({filter_sql})
            ORDER BY v.values {dist_op} $1::vector
            LIMIT $3
        ),
        bm25_raw AS (
            SELECT
                v.id,
                v.namespace,
                v.metadata,
                v.values::text AS values_text,
                (v.text_content <@> to_bm25query($4, '{table_ref}_bm25_idx')) AS bm25_score
            FROM {table_ref} v
            WHERE v.namespace = $2
              AND v.text_content IS NOT NULL
              AND ({filter_sql})
            ORDER BY v.text_content <@> to_bm25query($4, '{table_ref}_bm25_idx')
            LIMIT $3
        ),
        bm25_results AS (
            SELECT
                id,
                namespace,
                metadata,
                values_text,
                ROW_NUMBER() OVER (ORDER BY bm25_score ASC) AS bm25_rank
            FROM bm25_raw
        ),
        rrf AS (
            SELECT
                COALESCE(a.id, b.id)                     AS id,
                COALESCE(a.metadata, b.metadata)         AS metadata,
                COALESCE(a.values_text, b.values_text)   AS values_text,
                (
                    CASE WHEN a.dense_rank IS NOT NULL
                         THEN {alpha}::float8 / ({rrf_k}::float8 + a.dense_rank::float8)
                         ELSE 0
                    END
                    +
                    CASE WHEN b.bm25_rank IS NOT NULL
                         THEN {bm25_weight}::float8 / ({rrf_k}::float8 + b.bm25_rank::float8)
                         ELSE 0
                    END
                ) AS rrf_score
            FROM ann_results a
            FULL OUTER JOIN bm25_results b ON a.id = b.id AND a.namespace = b.namespace
        )
        SELECT id, metadata, values_text, rrf_score
        FROM rrf
        ORDER BY rrf_score DESC
        LIMIT $5
        "#
    );

    let mut q = sqlx::query(&sql)
        .bind(&vec_str) // $1
        .bind(&req.namespace) // $2
        .bind(candidate_k) // $3
        .bind(&req.text) // $4
        .bind(req.top_k); // $5

    // Bind filter params at $6+
    for param in &filter_params {
        q = match param {
            serde_json::Value::String(s) => q.bind(s.as_str()),
            _ => q.bind(param.to_string()),
        };
    }

    let rows = q.fetch_all(pool).await?;

    let matches = rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            let rrf_score: f64 = row.get("rrf_score");

            let need_metadata = req.include_metadata || req.rerank.is_some();
            let metadata: Option<serde_json::Value> = if need_metadata {
                row.get("metadata")
            } else {
                None
            };

            let values: Option<Vec<f32>> = if req.include_values {
                let v: Option<String> = row.get("values_text");
                v.map(|s| crate::handlers::records::parse_pgvector_str(&s))
            } else {
                None
            };

            HybridMatch {
                id,
                score: rrf_score,
                metadata,
                values,
            }
        })
        .collect();

    Ok(HybridQueryResponse {
        matches,
        namespace: req.namespace.clone(),
    })
}
