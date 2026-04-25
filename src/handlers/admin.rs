use crate::{error::ApiError, state::AppState};
use axum::{extract::State, Json};

/// POST /admin/indexes/:name/reindex
pub async fn reindex(
    State(_state): State<AppState>,
    axum::extract::Path(_name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::InvalidArgument(
        "Reindex is not yet implemented.".to_string(),
    ))
}

/// POST /admin/indexes/:name/vacuum
pub async fn vacuum(
    State(_state): State<AppState>,
    axum::extract::Path(_name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::InvalidArgument(
        "Vacuum is not yet implemented.".to_string(),
    ))
}

/// GET /admin/config
pub async fn dump_config(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::InvalidArgument(
        "Config dump is not yet implemented.".to_string(),
    ))
}
