use crate::{error::ApiError, state::AppState};
use axum::{extract::State, Json};
use sha2::{Digest, Sha256};
use sqlx::Row;

#[derive(serde::Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: Option<String>,
    pub allowed_namespaces: Option<Vec<String>>,
}

/// POST /admin/api_keys
pub async fn create_api_key(
    State(state): State<AppState>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let raw_key = format!("ocv-{}", uuid::Uuid::new_v4());
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let row = sqlx::query(
        r#"
        INSERT INTO _onecortex_vector.api_keys (key_hash, name, allowed_namespaces)
        VALUES ($1, $2, $3)
        RETURNING id, created_at
        "#,
    )
    .bind(&key_hash)
    .bind(req.name.as_deref())
    .bind(&req.allowed_namespaces)
    .fetch_one(&state.pool)
    .await?;

    let id: uuid::Uuid = row.get("id");
    let created_at: chrono::DateTime<chrono::Utc> = row.get("created_at");

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id,
            "key": raw_key,
            "name": req.name,
            "created_at": created_at.to_rfc3339(),
        })),
    ))
}

/// DELETE /admin/api_keys/:id
pub async fn revoke_api_key(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let result = sqlx::query(
        "UPDATE _onecortex_vector.api_keys SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL RETURNING id",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    if result.is_none() {
        return Err(ApiError::NotFound(format!(
            "API key '{id}' not found or already revoked."
        )));
    }

    Ok(Json(serde_json::json!({"status": "revoked"})))
}

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
