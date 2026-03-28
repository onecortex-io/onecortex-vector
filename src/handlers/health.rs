use crate::state::AppState;
use axum::{extract::State, Json};

pub async fn health() -> axum::http::StatusCode {
    axum::http::StatusCode::OK
}

pub async fn ready(State(state): State<AppState>) -> axum::http::StatusCode {
    match sqlx::query("SELECT 1").fetch_one(&state.pool).await {
        Ok(_) => axum::http::StatusCode::OK,
        Err(_) => axum::http::StatusCode::SERVICE_UNAVAILABLE,
    }
}

pub async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn metrics() -> String {
    // Implemented in Phase 6. Return empty for now.
    String::new()
}
