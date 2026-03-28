use std::sync::Arc;

pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: crate::config::AppConfig,
    pub reranker: Arc<dyn crate::planner::reranker::Reranker>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            config: self.config.clone(),
            reranker: Arc::clone(&self.reranker),
        }
    }
}
