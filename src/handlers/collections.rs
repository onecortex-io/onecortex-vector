use crate::{error::ApiError, state::AppState};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCollectionRequest {
    pub name: String,
    pub dimension: i32,
    #[serde(default = "default_metric")]
    pub metric: String,
    pub bm25_enabled: Option<bool>,
    pub deletion_protected: Option<bool>,
    pub tags: Option<serde_json::Value>,
}

fn default_metric() -> String {
    "cosine".to_string()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureCollectionRequest {
    pub deletion_protected: Option<bool>,
    pub tags: Option<serde_json::Value>,
    pub bm25_enabled: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResponse {
    pub name: String,
    pub dimension: i32,
    pub metric: String,
    pub status: CollectionStatus,
    pub host: String,
    pub vector_type: String,
    pub bm25_enabled: bool,
    pub deletion_protected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<serde_json::Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionStatus {
    pub ready: bool,
    pub state: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionListResponse {
    pub collections: Vec<CollectionResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeCollectionStatsResponse {
    pub namespaces: std::collections::HashMap<String, NamespaceSummary>,
    pub dimension: i32,
    pub collection_fullness: f64,
    pub total_record_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceSummary {
    pub record_count: i64,
}

fn validate_collection_name(name: &str) -> Result<(), ApiError> {
    let valid = !name.is_empty()
        && name.len() <= 45
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if !valid {
        return Err(ApiError::invalid_argument(
            "collection name must be 1-45 characters, lowercase alphanumeric and hyphens, \
             not starting or ending with a hyphen"
                .to_string(),
        ));
    }
    Ok(())
}

/// POST /collections
pub async fn create_collection(
    State(state): State<AppState>,
    Json(req): Json<CreateCollectionRequest>,
) -> Result<(axum::http::StatusCode, Json<CollectionResponse>), ApiError> {
    validate_collection_name(&req.name)?;
    if req.dimension < 1 || req.dimension > 20_000 {
        return Err(ApiError::invalid_argument(
            "dimension must be between 1 and 20000".to_string(),
        ));
    }
    if !["cosine", "euclidean", "dotproduct"].contains(&req.metric.as_str()) {
        return Err(ApiError::invalid_argument(
            "metric must be one of: cosine, euclidean, dotproduct".to_string(),
        ));
    }

    let collection_id = Uuid::new_v4();
    let bm25_enabled = req.bm25_enabled.unwrap_or(false);
    let deletion_protected = req.deletion_protected.unwrap_or(false);

    // Insert into catalog -- unique constraint on name gives us 409 on duplicate
    let insert_result = sqlx::query(
        r#"
        INSERT INTO _onecortex_vector.collections
            (id, name, dimension, metric, bm25_enabled, deletion_protected, tags)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(collection_id)
    .bind(&req.name)
    .bind(req.dimension)
    .bind(&req.metric)
    .bind(bm25_enabled)
    .bind(deletion_protected)
    .bind(&req.tags)
    .execute(&state.pool)
    .await;

    match insert_result {
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(ApiError::collection_already_exists(&req.name));
        }
        Err(e) => return Err(ApiError::Database(e)),
        Ok(_) => {}
    }

    // Create the records table and DiskANN index in _onecortex_vector
    crate::db::lifecycle::create_collection_table(
        &state.pool,
        collection_id,
        req.dimension,
        &req.metric,
        state.config.default_diskann_neighbors,
        state.config.default_diskann_search_list,
        bm25_enabled,
    )
    .await?;

    let host = format!("{}:{}", state.config.api_host, state.config.api_port);

    Ok((
        axum::http::StatusCode::CREATED,
        Json(CollectionResponse {
            name: req.name,
            dimension: req.dimension,
            metric: req.metric,
            status: CollectionStatus {
                ready: true,
                state: "Ready".to_string(),
            },
            host,
            vector_type: "dense".to_string(),
            bm25_enabled,
            deletion_protected,
            tags: req.tags,
        }),
    ))
}

/// GET /collections
pub async fn list_collections(
    State(state): State<AppState>,
) -> Result<Json<CollectionListResponse>, ApiError> {
    let rows = sqlx::query(
        "SELECT name, dimension, metric, status, bm25_enabled, deletion_protected, tags FROM _onecortex_vector.collections ORDER BY created_at"
    )
    .fetch_all(&state.pool)
    .await?;

    let host = format!("{}:{}", state.config.api_host, state.config.api_port);

    let collections = rows
        .into_iter()
        .map(|r| {
            let name: String = r.get("name");
            let dimension: i32 = r.get("dimension");
            let metric: String = r.get("metric");
            let status_str: String = r.get("status");
            let bm25_enabled: bool = r.get("bm25_enabled");
            let deletion_protected: bool = r.get("deletion_protected");
            let tags: Option<serde_json::Value> = r.get("tags");
            CollectionResponse {
                name,
                dimension,
                metric,
                status: CollectionStatus {
                    ready: status_str == "ready",
                    state: match status_str.as_str() {
                        "ready" => "Ready",
                        "initializing" => "Initializing",
                        "deleting" => "Terminating",
                        _ => "Unknown",
                    }
                    .to_string(),
                },
                host: host.clone(),
                vector_type: "dense".to_string(),
                bm25_enabled,
                deletion_protected,
                tags,
            }
        })
        .collect();

    Ok(Json(CollectionListResponse { collections }))
}

/// GET /collections/:name
pub async fn describe_collection(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<CollectionResponse>, ApiError> {
    let row = sqlx::query(
        "SELECT id, name, dimension, metric, status, bm25_enabled, deletion_protected, tags FROM _onecortex_vector.collections WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found(format!("Collection '{name}' does not exist.")))?;

    let host = format!("{}:{}", state.config.api_host, state.config.api_port);
    let status_str: String = row.get("status");

    Ok(Json(CollectionResponse {
        name: row.get("name"),
        dimension: row.get("dimension"),
        metric: row.get("metric"),
        status: CollectionStatus {
            ready: status_str == "ready",
            state: if status_str == "ready" {
                "Ready".to_string()
            } else {
                "Initializing".to_string()
            },
        },
        host,
        vector_type: "dense".to_string(),
        bm25_enabled: row.get("bm25_enabled"),
        deletion_protected: row.get("deletion_protected"),
        tags: row.get("tags"),
    }))
}

/// DELETE /collections/:name
pub async fn delete_collection(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let row = sqlx::query(
        "SELECT id, deletion_protected FROM _onecortex_vector.collections WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found(format!("Collection '{name}' does not exist.")))?;

    let deletion_protected: bool = row.get("deletion_protected");
    if deletion_protected {
        return Err(ApiError::permission_denied(format!(
            "Collection '{name}' has deletion protection enabled. Disable it before deleting."
        )));
    }

    let collection_id: Uuid = row.get("id");

    // Mark as deleting first
    sqlx::query(
        "UPDATE _onecortex_vector.collections SET status = 'deleting', updated_at = now() WHERE id = $1",
    )
    .bind(collection_id)
    .execute(&state.pool)
    .await?;

    // Drop the table -- this also deletes the row from _onecortex_vector.collections
    crate::db::lifecycle::drop_collection_table(&state.pool, collection_id).await?;

    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(serde_json::json!({})),
    ))
}

/// PATCH /collections/:name
pub async fn configure_collection(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(req): Json<ConfigureCollectionRequest>,
) -> Result<Json<CollectionResponse>, ApiError> {
    let deletion_protected = req.deletion_protected;

    let row = sqlx::query(
        r#"
        UPDATE _onecortex_vector.collections
        SET
            deletion_protected = COALESCE($2, deletion_protected),
            tags               = COALESCE($3::jsonb, tags),
            bm25_enabled       = COALESCE($4, bm25_enabled),
            updated_at         = now()
        WHERE name = $1
        RETURNING id, name, dimension, metric, status, deletion_protected, bm25_enabled, tags
        "#,
    )
    .bind(&name)
    .bind(deletion_protected)
    .bind(&req.tags)
    .bind(req.bm25_enabled)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found(format!("Collection '{name}' does not exist.")))?;

    // If bm25_enabled was toggled, build or drop the BM25 index in the background.
    if let Some(bm25) = req.bm25_enabled {
        let pool = state.pool.clone();
        let collection_id: Uuid = row.get("id");
        let table_name = crate::db::lifecycle::table_name_for(collection_id);
        tokio::spawn(async move {
            if bm25 {
                if let Err(e) = crate::db::lifecycle::build_bm25_index(&pool, &table_name).await {
                    tracing::error!(error = %e, "BM25 index build failed");
                }
            } else if let Err(e) = crate::db::lifecycle::drop_bm25_index(&pool, &table_name).await {
                tracing::error!(error = %e, "BM25 index drop failed");
            }
        });
    }

    let host = format!("{}:{}", state.config.api_host, state.config.api_port);
    let status_str: String = row.get("status");

    Ok(Json(CollectionResponse {
        name: row.get("name"),
        dimension: row.get("dimension"),
        metric: row.get("metric"),
        status: CollectionStatus {
            ready: status_str == "ready",
            state: "Ready".to_string(),
        },
        host,
        vector_type: "dense".to_string(),
        bm25_enabled: row.get("bm25_enabled"),
        deletion_protected: row.get("deletion_protected"),
        tags: row.get("tags"),
    }))
}

/// POST /collections/:name/describe_collection_stats
pub async fn describe_collection_stats(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    _body: Option<Json<serde_json::Value>>,
) -> Result<Json<DescribeCollectionStatsResponse>, ApiError> {
    let collection =
        sqlx::query("SELECT id, dimension FROM _onecortex_vector.collections WHERE name = $1")
            .bind(&name)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found(format!("Collection '{name}' does not exist.")))?;

    let collection_id: Uuid = collection.get("id");
    let dimension: i32 = collection.get("dimension");
    let table_name = crate::db::lifecycle::table_name_for(collection_id);

    // Query live record counts per namespace directly from the records table
    // to avoid any async stats cache lag
    let stats = sqlx::query(&format!(
        "SELECT namespace, COUNT(*) AS record_count FROM _onecortex.{table_name} GROUP BY namespace",
    ))
    .fetch_all(&state.pool)
    .await?;

    let mut namespaces = std::collections::HashMap::new();
    let mut total = 0i64;
    for s in stats {
        let ns: String = s.get("namespace");
        let rc: i64 = s.get("record_count");
        total += rc;
        namespaces.insert(ns, NamespaceSummary { record_count: rc });
    }

    Ok(Json(DescribeCollectionStatsResponse {
        namespaces,
        dimension,
        collection_fullness: 0.0,
        total_record_count: total,
    }))
}
