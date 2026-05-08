//! Hybrid (dense + BM25 RRF) source — adapter over the existing
//! `crate::planner::hybrid::hybrid_query` SQL builder. We do not duplicate the
//! RRF SQL here; we just translate the Plan inputs into a `HybridQueryRequest`
//! and convert the returned `HybridMatch` → `Match`.

use sqlx::PgPool;

use crate::error::ApiError;
use crate::handlers::query::Match;
use crate::handlers::records::CollectionMeta;
use crate::planner::hybrid::{hybrid_query, HybridQueryRequest};

#[allow(clippy::too_many_arguments)]
pub async fn run(
    pool: &PgPool,
    collection: &CollectionMeta,
    vector: &[f32],
    text: &str,
    alpha: f32,
    top_k: i64,
    namespace: &str,
    filter: &Option<serde_json::Value>,
    fetch_values: bool,
    fetch_metadata: bool,
) -> Result<Vec<Match>, ApiError> {
    let req = HybridQueryRequest {
        vector: vector.to_vec(),
        text: text.to_string(),
        top_k,
        alpha,
        filter: filter.clone(),
        namespace: namespace.to_string(),
        include_metadata: fetch_metadata,
        include_values: fetch_values,
        // We never let the inner builder rerank or threshold — those run as
        // Plan stages above the Source.
        rerank: None,
        score_threshold: None,
    };
    let result = hybrid_query(pool, &collection.table_ref(), &req, &collection.metric).await?;
    Ok(result
        .matches
        .into_iter()
        .map(|m| Match {
            id: m.id,
            score: m.score,
            values: m.values,
            metadata: m.metadata,
        })
        .collect())
}
