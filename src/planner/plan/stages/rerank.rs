//! Rerank stage. Wraps `Arc<dyn Reranker>` from the AppState.

use std::sync::Arc;

use crate::error::ApiError;
use crate::handlers::query::Match;
use crate::planner::reranker::{RerankCandidate, Reranker};

pub async fn run(
    reranker: &Arc<dyn Reranker>,
    matches: Vec<Match>,
    query: &str,
    top_n: i64,
    rank_field: &str,
    model: Option<&str>,
) -> Result<Vec<Match>, ApiError> {
    let candidates: Vec<RerankCandidate> = matches
        .into_iter()
        .map(|m| {
            let text = m
                .metadata
                .as_ref()
                .and_then(|meta| meta.get(rank_field))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            RerankCandidate {
                id: m.id,
                score: m.score as f32,
                text,
                metadata: m.metadata,
                values: m.values,
            }
        })
        .collect();

    let reranked = reranker
        .rerank(query, candidates, top_n.max(0) as usize, model)
        .await
        .map_err(ApiError::from)?;

    Ok(reranked
        .into_iter()
        .map(|r| Match {
            id: r.id,
            score: r.rerank_score as f64,
            metadata: r.metadata,
            values: r.values,
        })
        .collect())
}
