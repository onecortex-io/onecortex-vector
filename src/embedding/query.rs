//! Query-side embedding helper, shared by the `/query`, `/query/hybrid`, and
//! `/search` handlers and by the Plan compiler.
//!
//! Lives in this module (not in `handlers::query`) so the planner can call it
//! without a reverse dependency on the handlers layer.

use std::sync::Arc;

use crate::embedding::{EmbedInputType, EmbedderConfig, EmbedderFactory, QueryEmbedCache};
use crate::error::ApiError;

/// Embed a single query text via the bound embedder, hitting (and populating)
/// the shared LRU cache unless `no_cache` is true.
pub async fn embed_query_text(
    factory: &Arc<EmbedderFactory>,
    cache: &Arc<QueryEmbedCache>,
    ec: &EmbedderConfig,
    text: &str,
    no_cache: bool,
) -> Result<Arc<Vec<f32>>, ApiError> {
    let embedder = factory.for_config(ec).map_err(ApiError::from)?;
    if no_cache {
        let mut vectors = embedder
            .embed(&[text.to_string()], EmbedInputType::Query)
            .await
            .map_err(ApiError::from)?;
        let v = vectors.pop().ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!("embedder returned empty result for query"))
        })?;
        return Ok(Arc::new(v));
    }
    let key = QueryEmbedCache::make_key(embedder.backend(), embedder.model(), text);
    if let Some(hit) = cache.get(&key).await {
        return Ok(hit);
    }
    let mut vectors = embedder
        .embed(&[text.to_string()], EmbedInputType::Query)
        .await
        .map_err(ApiError::from)?;
    let v = vectors.pop().ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!("embedder returned empty result for query"))
    })?;
    let arc = Arc::new(v);
    cache.insert(key, arc.clone()).await;
    Ok(arc)
}
