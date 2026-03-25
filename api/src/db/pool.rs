use sqlx::postgres::PgPoolOptions;
use crate::config::AppConfig;

/// Create a connection pool and run all pending migrations.
/// Migrations are in api/migrations/ and run via sqlx::migrate!().
pub async fn create_pool(config: &AppConfig) -> Result<sqlx::PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_conns)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&config.database_url)
        .await?;

    // Run all pending migrations from api/migrations/
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run database migrations");

    tracing::info!("Database pool created and migrations applied");
    Ok(pool)
}
