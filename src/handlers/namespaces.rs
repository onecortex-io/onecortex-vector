use crate::{error::ApiError, state::AppState};
use axum::{extract::State, Json};
use serde::Deserialize;
use sqlx::Row;

/// GET /collections/:name/namespaces
pub async fn list_namespaces(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;

    let rows = sqlx::query(&format!(
        "SELECT DISTINCT namespace FROM {} ORDER BY namespace",
        collection.table_ref()
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

/// POST /collections/:name/namespaces
pub async fn create_namespace(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<CreateNamespaceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;

    // Namespaces are created implicitly on first upsert.
    // This endpoint ensures a stats row exists for the namespace.
    sqlx::query(
        r#"
        INSERT INTO _onecortex_vector.collection_stats (collection_id, namespace, record_count)
        VALUES ($1, $2, 0)
        ON CONFLICT (collection_id, namespace) DO NOTHING
        "#,
    )
    .bind(collection.id)
    .bind(&req.name)
    .execute(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({
        "name": req.name,
        "record_count": 0,
    })))
}

/// GET /collections/:name/namespaces/:ns
pub async fn describe_namespace(
    State(state): State<AppState>,
    axum::extract::Path((collection_name, ns)): axum::extract::Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;

    let row = sqlx::query(
        "SELECT namespace, record_count FROM _onecortex_vector.collection_stats WHERE collection_id = $1 AND namespace = $2",
    )
    .bind(collection.id)
    .bind(&ns)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        ApiError::not_found(format!(
            "Namespace '{ns}' not found in collection '{collection_name}'."
        ))
    })?;

    let namespace: String = row.get("namespace");
    let record_count: i64 = row.get("record_count");

    Ok(Json(serde_json::json!({
        "name": namespace,
        "record_count": record_count,
    })))
}

/// DELETE /collections/:name/namespaces/:ns
pub async fn delete_namespace(
    State(state): State<AppState>,
    axum::extract::Path((collection_name, ns)): axum::extract::Path<(String, String)>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;

    sqlx::query(&format!(
        "DELETE FROM {} WHERE namespace = $1",
        collection.table_ref()
    ))
    .bind(&ns)
    .execute(&state.pool)
    .await?;

    sqlx::query(
        "DELETE FROM _onecortex_vector.collection_stats WHERE collection_id = $1 AND namespace = $2",
    )
    .bind(collection.id)
    .bind(&ns)
    .execute(&state.pool)
    .await?;

    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(serde_json::json!({})),
    ))
}
