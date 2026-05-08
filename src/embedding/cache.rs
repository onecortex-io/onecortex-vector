use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;

/// LRU cache for query-side embeddings, keyed by `(backend, model, normalised_text)`.
/// Upserts are NEVER cached (content is one-shot).
///
/// Bounded by capacity + per-entry TTL to keep memory predictable. The cache key
/// includes the backend + model so two collections using different models do not
/// collide. Text is `trim()`-normalised; case is preserved (some models are
/// case-sensitive).
pub struct QueryEmbedCache {
    inner: Cache<String, Arc<Vec<f32>>>,
}

impl QueryEmbedCache {
    pub fn new(capacity: u64, ttl_secs: u64) -> Self {
        let inner = Cache::builder()
            .max_capacity(capacity)
            .time_to_live(Duration::from_secs(ttl_secs))
            .build();
        Self { inner }
    }

    pub fn make_key(backend: &str, model: &str, text: &str) -> String {
        format!("{}|{}|{}", backend, model, text.trim())
    }

    pub async fn get(&self, key: &str) -> Option<Arc<Vec<f32>>> {
        self.inner.get(key).await
    }

    pub async fn insert(&self, key: String, value: Arc<Vec<f32>>) {
        self.inner.insert(key, value).await;
    }

    /// Test/observability hook.
    #[allow(dead_code)]
    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }
}

impl Default for QueryEmbedCache {
    fn default() -> Self {
        Self::new(10_000, 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_format_trims_text_preserves_case() {
        assert_eq!(
            QueryEmbedCache::make_key("openai", "text-embedding-3-small", "  Hello World  "),
            "openai|text-embedding-3-small|Hello World"
        );
    }

    #[tokio::test]
    async fn get_miss_then_hit_after_insert() {
        let cache = QueryEmbedCache::new(16, 60);
        assert!(cache.get("k").await.is_none());
        cache
            .insert("k".to_string(), Arc::new(vec![1.0, 2.0]))
            .await;
        let hit = cache.get("k").await.expect("expected cache hit");
        assert_eq!(hit.as_ref(), &vec![1.0, 2.0]);
    }
}
