use crate::{error::ApiError, state::AppState};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAliasRequest {
    pub alias: String,
    pub index_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasResponse {
    pub alias: String,
    pub index_name: String,
}

#[derive(Serialize)]
pub struct AliasListResponse {
    pub aliases: Vec<AliasResponse>,
}

/// POST /aliases — create or update an alias
pub async fn create_alias(
    State(state): State<AppState>,
    Json(req): Json<CreateAliasRequest>,
) -> Result<(axum::http::StatusCode, Json<AliasResponse>), ApiError> {
    if req.alias.is_empty() || req.alias.len() > 45 {
        return Err(ApiError::InvalidArgument(
            "alias must be 1-45 characters".to_string(),
        ));
    }

    // Verify the target index exists and is ready
    let _ = crate::handlers::vectors::resolve_index(&state.pool, &req.index_name).await?;

    let row = sqlx::query(
        r#"
        INSERT INTO _onecortex_vector.aliases (alias, index_name)
        VALUES ($1, $2)
        ON CONFLICT (alias) DO UPDATE SET
            index_name = EXCLUDED.index_name,
            updated_at = now()
        RETURNING alias, index_name
        "#,
    )
    .bind(&req.alias)
    .bind(&req.index_name)
    .fetch_one(&state.pool)
    .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(AliasResponse {
            alias: row.get("alias"),
            index_name: row.get("index_name"),
        }),
    ))
}

/// GET /aliases — list all aliases
pub async fn list_aliases(
    State(state): State<AppState>,
) -> Result<Json<AliasListResponse>, ApiError> {
    let rows = sqlx::query(
        "SELECT alias, index_name FROM _onecortex_vector.aliases ORDER BY created_at",
    )
    .fetch_all(&state.pool)
    .await?;

    let aliases = rows
        .into_iter()
        .map(|r| AliasResponse {
            alias: r.get("alias"),
            index_name: r.get("index_name"),
        })
        .collect();

    Ok(Json(AliasListResponse { aliases }))
}

/// GET /aliases/:alias — describe a single alias
pub async fn describe_alias(
    State(state): State<AppState>,
    axum::extract::Path(alias): axum::extract::Path<String>,
) -> Result<Json<AliasResponse>, ApiError> {
    let row = sqlx::query(
        "SELECT alias, index_name FROM _onecortex_vector.aliases WHERE alias = $1",
    )
    .bind(&alias)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Alias '{alias}' does not exist.")))?;

    Ok(Json(AliasResponse {
        alias: row.get("alias"),
        index_name: row.get("index_name"),
    }))
}

/// DELETE /aliases/:alias — delete an alias
pub async fn delete_alias(
    State(state): State<AppState>,
    axum::extract::Path(alias): axum::extract::Path<String>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let result = sqlx::query("DELETE FROM _onecortex_vector.aliases WHERE alias = $1")
        .bind(&alias)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound(format!(
            "Alias '{alias}' does not exist."
        )));
    }

    Ok((axum::http::StatusCode::OK, Json(serde_json::json!({}))))
}
