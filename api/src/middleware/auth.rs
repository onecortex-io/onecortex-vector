use crate::{error::ApiError, state::AppState};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};
use sqlx::Row;

/// Injected into request extensions after successful authentication.
#[derive(Clone, Debug)]
pub struct AuthContext {
    pub key_id: uuid::Uuid,
    /// None = unrestricted (access to all namespaces)
    pub allowed_namespaces: Option<Vec<String>>,
}

/// Paths that do not require authentication.
const EXEMPT_PATHS: &[&str] = &["/health", "/ready", "/version", "/metrics"];

/// Axum middleware function. Use with:
///   .layer(axum::middleware::from_fn_with_state(state, auth_middleware))
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let path = req.uri().path().to_string();

    // Exempt paths bypass authentication entirely
    if EXEMPT_PATHS.iter().any(|p| path == *p) {
        return Ok(next.run(req).await);
    }

    // Extract API key from header.
    // Accept both "Api-Key: <key>" and "Authorization: Api-Key <key>"
    let raw_key = extract_api_key(&req)
        .ok_or_else(|| ApiError::Unauthenticated("Missing Api-Key header.".to_string()))?;

    // SHA-256 hash the key
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    // Look up in database
    let row = sqlx::query(
        r#"
        SELECT id, allowed_namespaces
        FROM _onecortex_vector.api_keys
        WHERE key_hash = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(&key_hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::Internal(e.into()))?;

    match row {
        None => Err(ApiError::Unauthenticated("Invalid API key.".to_string())),
        Some(r) => {
            let key_id: uuid::Uuid = r.get("id");
            let allowed_namespaces: Option<Vec<String>> = r.get("allowed_namespaces");
            let ctx = AuthContext {
                key_id,
                allowed_namespaces,
            };
            req.extensions_mut().insert(ctx);
            Ok(next.run(req).await)
        }
    }
}

fn extract_api_key(req: &Request) -> Option<String> {
    // Primary: "Api-Key: <key>"
    if let Some(val) = req.headers().get("Api-Key") {
        return val.to_str().ok().map(|s| s.to_string());
    }
    // Secondary: "Authorization: Api-Key <key>"
    if let Some(val) = req.headers().get("authorization") {
        if let Ok(s) = val.to_str() {
            if let Some(key) = s.strip_prefix("Api-Key ") {
                return Some(key.to_string());
            }
        }
    }
    None
}

/// Insert a test API key into the database. Returns the raw key string.
pub async fn seed_test_key(pool: &sqlx::PgPool) -> String {
    let raw_key = "test-api-key-12345";
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    sqlx::query(
        "INSERT INTO _onecortex_vector.api_keys (key_hash, name) VALUES ($1, 'test-key')
         ON CONFLICT (key_hash) DO NOTHING",
    )
    .bind(&key_hash)
    .execute(pool)
    .await
    .unwrap();

    raw_key.to_string()
}
