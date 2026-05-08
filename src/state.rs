use std::sync::Arc;

pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: crate::config::AppConfig,
    pub reranker: Arc<dyn crate::planner::reranker::Reranker>,
    /// Lazy factory: builds (and memoizes) embedders by (backend, model).
    pub embedder_factory: Arc<crate::embedding::EmbedderFactory>,
    /// Query-side LRU cache for `(backend, model, text) -> Vec<f32>`.
    pub embed_cache: Arc<crate::embedding::QueryEmbedCache>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            config: self.config.clone(),
            reranker: Arc::clone(&self.reranker),
            embedder_factory: Arc::clone(&self.embedder_factory),
            embed_cache: Arc::clone(&self.embed_cache),
        }
    }
}
