#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: crate::config::AppConfig,
}
