use crate::{error::ApiError, state::AppState};
use axum::{extract::State, Json};
use serde::Deserialize;
use sqlx::Row;

/// GET /indexes/:name/namespaces
pub async fn list_namespaces(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = crate::handlers::vectors::resolve_index(&state.pool, &index_name).await?;

    let rows = sqlx::query(&format!(
        "SELECT DISTINCT namespace FROM {}.vectors ORDER BY namespace",
        index.schema_name
    ))
    .fetch_all(&state.pool)
    .await?;

    let namespaces: Vec<String> = rows.iter().map(|r| r.get("namespace")).collect();

    Ok(Json(serde_json::json!({ "namespaces": namespaces })))
}

#[derive(Deserialize)]
pub struct CreateNamespaceRequest {
    pub name: String,
}

/// POST /indexes/:name/namespaces
pub async fn create_namespace(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    Json(req): Json<CreateNamespaceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = crate::handlers::vectors::resolve_index(&state.pool, &index_name).await?;

    // Namespaces are created implicitly on first upsert.
    // This endpoint ensures a stats row exists for the namespace.
    sqlx::query(
        r#"
        INSERT INTO _onecortex_vector.index_stats (index_id, namespace, vector_count)
        VALUES ($1, $2, 0)
        ON CONFLICT (index_id, namespace) DO NOTHING
        "#,
    )
    .bind(index.id)
    .bind(&req.name)
    .execute(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({
        "name": req.name,
        "record_count": 0,
    })))
}

/// GET /indexes/:name/namespaces/:ns
pub async fn describe_namespace(
    State(state): State<AppState>,
    axum::extract::Path((index_name, ns)): axum::extract::Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = crate::handlers::vectors::resolve_index(&state.pool, &index_name).await?;

    let row = sqlx::query(
        "SELECT namespace, vector_count FROM _onecortex_vector.index_stats WHERE index_id = $1 AND namespace = $2",
    )
    .bind(index.id)
    .bind(&ns)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Namespace '{ns}' not found in index '{index_name}'.")))?;

    let namespace: String = row.get("namespace");
    let vector_count: i64 = row.get("vector_count");

    Ok(Json(serde_json::json!({
        "name": namespace,
        "record_count": vector_count,
    })))
}

/// DELETE /indexes/:name/namespaces/:ns
pub async fn delete_namespace(
    State(state): State<AppState>,
    axum::extract::Path((index_name, ns)): axum::extract::Path<(String, String)>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let index = crate::handlers::vectors::resolve_index(&state.pool, &index_name).await?;

    sqlx::query(&format!(
        "DELETE FROM {}.vectors WHERE namespace = $1",
        index.schema_name
    ))
    .bind(&ns)
    .execute(&state.pool)
    .await?;

    sqlx::query("DELETE FROM _onecortex_vector.index_stats WHERE index_id = $1 AND namespace = $2")
        .bind(index.id)
        .bind(&ns)
        .execute(&state.pool)
        .await?;

    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(serde_json::json!({})),
    ))
}
