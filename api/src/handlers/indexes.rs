use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use crate::{error::ApiError, state::AppState};

#[derive(Deserialize)]
pub struct CreateIndexRequest {
    pub name: String,
    pub dimension: i32,
    #[serde(default = "default_metric")]
    pub metric: String,
    /// Accepted but ignored -- Onecortex is self-hosted, no cloud spec needed
    pub spec: Option<serde_json::Value>,
    /// Onecortex extension -- enables BM25 index on this index
    pub bm25_enabled: Option<bool>,
    /// Accepted: "enabled" | "disabled" -- stored in deletion_protected column
    pub deletion_protection: Option<String>,
    /// Arbitrary JSON tags
    pub tags: Option<serde_json::Value>,
}

fn default_metric() -> String { "cosine".to_string() }

#[derive(Deserialize)]
pub struct ConfigureIndexRequest {
    pub deletion_protection: Option<String>,
    pub tags: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct IndexResponse {
    pub name: String,
    pub dimension: i32,
    pub metric: String,
    pub status: IndexStatus,
    pub host: String,
    pub spec: serde_json::Value,
    pub vector_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct IndexStatus {
    pub ready: bool,
    pub state: String,
}

#[derive(Serialize)]
pub struct IndexListResponse {
    pub indexes: Vec<IndexResponse>,
}

#[derive(Serialize)]
pub struct DescribeIndexStatsResponse {
    pub namespaces: std::collections::HashMap<String, NamespaceSummary>,
    pub dimension: i32,
    #[serde(rename = "indexFullness")]
    pub index_fullness: f64,
    #[serde(rename = "totalVectorCount")]
    pub total_vector_count: i64,
}

#[derive(Serialize)]
pub struct NamespaceSummary {
    #[serde(rename = "vectorCount")]
    pub vector_count: i64,
}

fn validate_index_name(name: &str) -> Result<(), ApiError> {
    let valid = !name.is_empty()
        && name.len() <= 45
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if !valid {
        return Err(ApiError::InvalidArgument(
            "index name must be 1-45 characters, lowercase alphanumeric and hyphens, \
             not starting or ending with a hyphen".to_string()
        ));
    }
    Ok(())
}

/// POST /indexes
pub async fn create_index(
    State(state): State<AppState>,
    Json(req): Json<CreateIndexRequest>,
) -> Result<(axum::http::StatusCode, Json<IndexResponse>), ApiError> {
    validate_index_name(&req.name)?;
    if req.dimension < 1 || req.dimension > 20_000 {
        return Err(ApiError::InvalidArgument(
            "dimension must be between 1 and 20000".to_string()
        ));
    }
    if !["cosine", "euclidean", "dotproduct"].contains(&req.metric.as_str()) {
        return Err(ApiError::InvalidArgument(
            "metric must be one of: cosine, euclidean, dotproduct".to_string()
        ));
    }

    let index_id = Uuid::new_v4();
    let schema_name = crate::db::lifecycle::schema_name_for(index_id);
    let bm25_enabled = req.bm25_enabled.unwrap_or(false);
    let deletion_protected = req.deletion_protection.as_deref() == Some("enabled");

    // Insert into catalog -- unique constraint on name gives us 409 on duplicate
    let insert_result = sqlx::query(
        r#"
        INSERT INTO _onecortex_vector.indexes
            (id, name, dimension, metric, bm25_enabled, schema_name, deletion_protected, tags)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(index_id)
    .bind(&req.name)
    .bind(req.dimension)
    .bind(&req.metric)
    .bind(bm25_enabled)
    .bind(&schema_name)
    .bind(deletion_protected)
    .bind(&req.tags)
    .execute(&state.pool)
    .await;

    match insert_result {
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(ApiError::AlreadyExists(
                format!("Index '{}' already exists.", req.name)
            ));
        }
        Err(e) => return Err(ApiError::Database(e)),
        Ok(_) => {}
    }

    // Create the Postgres schema, vectors table, and DiskANN index
    crate::db::lifecycle::create_index_schema(
        &state.pool,
        index_id,
        &schema_name,
        req.dimension,
        &req.metric,
        state.config.default_diskann_neighbors,
        state.config.default_diskann_search_list,
    ).await?;

    let host = format!("{}:{}", state.config.api_host, state.config.api_port);

    Ok((axum::http::StatusCode::CREATED, Json(IndexResponse {
        name: req.name,
        dimension: req.dimension,
        metric: req.metric,
        status: IndexStatus { ready: true, state: "Ready".to_string() },
        host,
        spec: serde_json::json!({}),
        vector_type: "dense".to_string(),
        tags: req.tags,
    })))
}

/// GET /indexes
pub async fn list_indexes(
    State(state): State<AppState>,
) -> Result<Json<IndexListResponse>, ApiError> {
    let rows = sqlx::query(
        "SELECT name, dimension, metric, status, tags FROM _onecortex_vector.indexes ORDER BY created_at"
    )
    .fetch_all(&state.pool)
    .await?;

    let host = format!("{}:{}", state.config.api_host, state.config.api_port);

    let indexes = rows.into_iter().map(|r| {
        let name: String = r.get("name");
        let dimension: i32 = r.get("dimension");
        let metric: String = r.get("metric");
        let status_str: String = r.get("status");
        let tags: Option<serde_json::Value> = r.get("tags");
        IndexResponse {
            name,
            dimension,
            metric,
            status: IndexStatus {
                ready: status_str == "ready",
                state: match status_str.as_str() {
                    "ready"        => "Ready",
                    "initializing" => "Initializing",
                    "deleting"     => "Terminating",
                    _              => "Unknown",
                }.to_string(),
            },
            host: host.clone(),
            spec: serde_json::json!({}),
            vector_type: "dense".to_string(),
            tags,
        }
    }).collect();

    Ok(Json(IndexListResponse { indexes }))
}

/// GET /indexes/:name
pub async fn describe_index(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<IndexResponse>, ApiError> {
    let row = sqlx::query(
        "SELECT id, name, dimension, metric, status, tags FROM _onecortex_vector.indexes WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Index '{name}' does not exist.")))?;

    let host = format!("{}:{}", state.config.api_host, state.config.api_port);
    let status_str: String = row.get("status");

    Ok(Json(IndexResponse {
        name: row.get("name"),
        dimension: row.get("dimension"),
        metric: row.get("metric"),
        status: IndexStatus {
            ready: status_str == "ready",
            state: if status_str == "ready" { "Ready".to_string() } else { "Initializing".to_string() },
        },
        host,
        spec: serde_json::json!({}),
        vector_type: "dense".to_string(),
        tags: row.get("tags"),
    }))
}

/// DELETE /indexes/:name
pub async fn delete_index(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let row = sqlx::query(
        "SELECT id, schema_name, deletion_protected FROM _onecortex_vector.indexes WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Index '{name}' does not exist.")))?;

    let deletion_protected: bool = row.get("deletion_protected");
    if deletion_protected {
        return Err(ApiError::PermissionDenied(
            format!("Index '{name}' has deletion protection enabled. Disable it before deleting.")
        ));
    }

    let index_id: Uuid = row.get("id");
    let schema_name: String = row.get("schema_name");

    // Mark as deleting first
    sqlx::query(
        "UPDATE _onecortex_vector.indexes SET status = 'deleting', updated_at = now() WHERE id = $1",
    )
    .bind(index_id)
    .execute(&state.pool)
    .await?;

    // Drop the schema -- this also deletes the row from _onecortex_vector.indexes
    crate::db::lifecycle::drop_index_schema(&state.pool, index_id, &schema_name).await?;

    Ok((axum::http::StatusCode::ACCEPTED, Json(serde_json::json!({}))))
}

/// PATCH /indexes/:name
pub async fn configure_index(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(req): Json<ConfigureIndexRequest>,
) -> Result<Json<IndexResponse>, ApiError> {
    let deletion_protected = req.deletion_protection.as_deref().map(|s| s == "enabled");

    let row = sqlx::query(
        r#"
        UPDATE _onecortex_vector.indexes
        SET
            deletion_protected = COALESCE($2, deletion_protected),
            tags               = COALESCE($3::jsonb, tags),
            updated_at         = now()
        WHERE name = $1
        RETURNING id, name, dimension, metric, status, deletion_protected, tags
        "#,
    )
    .bind(&name)
    .bind(deletion_protected)
    .bind(&req.tags)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Index '{name}' does not exist.")))?;

    let host = format!("{}:{}", state.config.api_host, state.config.api_port);
    let status_str: String = row.get("status");

    Ok(Json(IndexResponse {
        name: row.get("name"),
        dimension: row.get("dimension"),
        metric: row.get("metric"),
        status: IndexStatus { ready: status_str == "ready", state: "Ready".to_string() },
        host,
        spec: serde_json::json!({}),
        vector_type: "dense".to_string(),
        tags: row.get("tags"),
    }))
}

/// POST /indexes/:name/describe_index_stats
pub async fn describe_index_stats(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    _body: Option<Json<serde_json::Value>>,
) -> Result<Json<DescribeIndexStatsResponse>, ApiError> {
    let index = sqlx::query(
        "SELECT id, dimension, schema_name FROM _onecortex_vector.indexes WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Index '{name}' does not exist.")))?;

    let dimension: i32 = index.get("dimension");
    let schema_name: String = index.get("schema_name");

    // Query live vector counts per namespace directly from the vectors table
    // to avoid any async stats cache lag
    let stats = sqlx::query(&format!(
        "SELECT namespace, COUNT(*) AS vector_count FROM {schema_name}.vectors GROUP BY namespace",
    ))
    .fetch_all(&state.pool)
    .await?;

    let mut namespaces = std::collections::HashMap::new();
    let mut total = 0i64;
    for s in stats {
        let ns: String = s.get("namespace");
        let vc: i64 = s.get("vector_count");
        total += vc;
        namespaces.insert(ns, NamespaceSummary { vector_count: vc });
    }

    Ok(Json(DescribeIndexStatsResponse {
        namespaces,
        dimension,
        index_fullness: 0.0,
        total_vector_count: total,
    }))
}
