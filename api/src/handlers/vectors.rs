use crate::{error::ApiError, state::AppState};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;

pub struct IndexRecord {
    pub id: uuid::Uuid,
    pub schema_name: String,
    pub dimension: i32,
    pub metric: String,
    pub bm25_enabled: bool,
}

pub async fn resolve_index(pool: &sqlx::PgPool, name: &str) -> Result<IndexRecord, ApiError> {
    let row = sqlx::query(
        "SELECT id, schema_name, dimension, metric, bm25_enabled FROM _onecortex_vector.indexes WHERE name = $1 AND status = 'ready'",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("Index '{name}' does not exist or is not ready.")))?;

    Ok(IndexRecord {
        id: row.get("id"),
        schema_name: row.get("schema_name"),
        dimension: row.get("dimension"),
        metric: row.get("metric"),
        bm25_enabled: row.get("bm25_enabled"),
    })
}

// --- Upsert ---

#[derive(Deserialize)]
pub struct UpsertRequest {
    pub vectors: Vec<VectorInput>,
    pub namespace: Option<String>,
}

#[derive(Deserialize)]
pub struct VectorInput {
    pub id: String,
    pub values: Vec<f32>,
    /// Accepted but silently dropped -- see 00-reference.md section 8
    #[serde(rename = "sparseValues")]
    pub sparse_values: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    /// Text content for BM25 hybrid search (Phase 3). Stored in text_content column.
    pub text: Option<String>,
}

#[derive(Serialize)]
pub struct UpsertResponse {
    #[serde(rename = "upsertedCount")]
    pub upserted_count: i64,
}

/// POST /indexes/:name/vectors/upsert
pub async fn upsert_vectors(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    Json(req): Json<UpsertRequest>,
) -> Result<Json<UpsertResponse>, ApiError> {
    if req.vectors.len() > 1000 {
        return Err(ApiError::InvalidArgument(
            "upsert batch cannot exceed 1000 vectors".to_string(),
        ));
    }
    if req.vectors.is_empty() {
        return Ok(Json(UpsertResponse { upserted_count: 0 }));
    }

    let index = resolve_index(&state.pool, &index_name).await?;
    let namespace = req.namespace.unwrap_or_default();

    // Validate all IDs and dimensions before any DB writes
    for v in &req.vectors {
        if v.id.len() > 512 {
            return Err(ApiError::InvalidArgument(format!(
                "vector id '{}...' exceeds 512 character limit",
                &v.id[..20.min(v.id.len())]
            )));
        }
        if v.values.len() != index.dimension as usize {
            return Err(ApiError::InvalidArgument(format!(
                "vector '{}' has {} dimensions but index expects {}",
                v.id,
                v.values.len(),
                index.dimension
            )));
        }
        if v.sparse_values.is_some() {
            tracing::warn!(
                vector_id = %v.id,
                "received sparseValues for vector; sparse vectors are not supported and will be silently ignored"
            );
        }
    }

    let schema = &index.schema_name;
    let upsert_sql = format!(
        "INSERT INTO {schema}.vectors (id, namespace, values, text_content, metadata, updated_at) VALUES "
    );

    let mut qb = sqlx::QueryBuilder::new(upsert_sql);
    let mut sep = qb.separated(", ");
    for v in &req.vectors {
        let embedding_str = format!(
            "[{}]",
            v.values
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        sep.push("(");
        sep.push_bind_unseparated(&v.id);
        sep.push_unseparated(", ");
        sep.push_bind_unseparated(&namespace);
        sep.push_unseparated(&format!(", '{embedding_str}'::vector, "));
        sep.push_bind_unseparated(v.text.as_deref());
        sep.push_unseparated(", ");
        sep.push_bind_unseparated(&v.metadata);
        sep.push_unseparated(", now())");
    }

    qb.push(
        " ON CONFLICT (id, namespace) DO UPDATE SET \
            values       = EXCLUDED.values, \
            text_content = EXCLUDED.text_content, \
            metadata     = EXCLUDED.metadata, \
            updated_at   = now()",
    );

    qb.build().execute(&state.pool).await?;

    let count = req.vectors.len() as i64;

    // Async stats update -- fire and forget, non-blocking
    let pool = state.pool.clone();
    let index_id = index.id;
    let ns = namespace.clone();
    let schema_clone = schema.clone();
    tokio::spawn(async move {
        let result = sqlx::query(&format!(
            r#"
            INSERT INTO _onecortex_vector.index_stats (index_id, namespace, vector_count)
            SELECT $1, $2, COUNT(*) FROM {schema_clone}.vectors WHERE namespace = $2
            ON CONFLICT (index_id, namespace)
            DO UPDATE SET vector_count = EXCLUDED.vector_count, updated_at = now()
            "#
        ))
        .bind(index_id)
        .bind(&ns)
        .execute(&pool)
        .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "Failed to update index stats");
        }
    });

    Ok(Json(UpsertResponse {
        upserted_count: count,
    }))
}

// --- Fetch ---

#[derive(Deserialize)]
pub struct FetchRequest {
    pub ids: Vec<String>,
    pub namespace: Option<String>,
}

/// POST /indexes/:name/vectors/fetch
pub async fn fetch_vectors(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    Json(req): Json<FetchRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.ids.len() > 1000 {
        return Err(ApiError::InvalidArgument(
            "ids array cannot exceed 1000 entries".to_string(),
        ));
    }
    let index = resolve_index(&state.pool, &index_name).await?;
    let namespace = req.namespace.unwrap_or_default();

    let rows = sqlx::query(&format!(
        "SELECT id, values::text, metadata FROM {}.vectors WHERE namespace = $1 AND id = ANY($2::text[])",
        index.schema_name
    ))
    .bind(&namespace)
    .bind(&req.ids)
    .fetch_all(&state.pool)
    .await?;

    let mut vectors = serde_json::Map::new();
    for row in rows {
        let id: String = row.get("id");
        let values_str: Option<String> = row.get("values");
        let metadata: Option<serde_json::Value> = row.get("metadata");

        let values: Option<Vec<f32>> = values_str.map(|s| parse_pgvector_str(&s));
        vectors.insert(
            id.clone(),
            serde_json::json!({
                "id": id,
                "values": values.unwrap_or_default(),
                "metadata": metadata.unwrap_or(serde_json::json!({})),
            }),
        );
    }

    Ok(Json(serde_json::json!({
        "vectors": vectors,
        "namespace": namespace,
    })))
}

// --- Fetch by metadata ---

#[derive(Deserialize)]
pub struct FetchByMetadataRequest {
    pub filter: serde_json::Value,
    pub namespace: Option<String>,
    pub limit: Option<i64>,
    pub include_values: Option<bool>,
    pub include_metadata: Option<bool>,
}

/// POST /indexes/:name/vectors/fetch_by_metadata
pub async fn fetch_by_metadata(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    Json(req): Json<FetchByMetadataRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = resolve_index(&state.pool, &index_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let limit = req.limit.unwrap_or(100).min(1000);

    let (filter_sql, filter_params) =
        crate::planner::filter_translator::translate_filter(&req.filter, 1)
            .map_err(|e| ApiError::InvalidArgument(e.to_string()))?;

    let include_values = req.include_values.unwrap_or(false);
    let values_col = if include_values {
        "values::text"
    } else {
        "NULL::text AS values"
    };

    let sql = format!(
        "SELECT id, {values_col}, metadata FROM {}.vectors WHERE namespace = $1 AND ({filter_sql}) LIMIT {limit}",
        index.schema_name
    );

    let mut query = sqlx::query(&sql).bind(&namespace);
    for p in &filter_params {
        query = query.bind(p.as_str().unwrap_or(""));
    }

    let rows = query.fetch_all(&state.pool).await?;

    let vectors: Vec<serde_json::Value> = rows.into_iter().map(|row| {
        let id: String = row.get("id");
        let values_str: Option<String> = row.get("values");
        let metadata: Option<serde_json::Value> = row.get("metadata");
        let values: Vec<f32> = values_str.map(|s| parse_pgvector_str(&s)).unwrap_or_default();
        serde_json::json!({
            "id": id,
            "values": if include_values { serde_json::json!(values) } else { serde_json::Value::Null },
            "metadata": metadata,
        })
    }).collect();

    Ok(Json(
        serde_json::json!({ "vectors": vectors, "namespace": namespace }),
    ))
}

// --- Delete ---

#[derive(Deserialize)]
pub struct DeleteRequest {
    pub ids: Option<Vec<String>>,
    #[serde(rename = "deleteAll")]
    pub delete_all: Option<bool>,
    pub filter: Option<serde_json::Value>,
    pub namespace: Option<String>,
}

/// POST /indexes/:name/vectors/delete
pub async fn delete_vectors(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    Json(req): Json<DeleteRequest>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let index = resolve_index(&state.pool, &index_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let schema = &index.schema_name;

    if req.delete_all == Some(true) {
        sqlx::query(&format!(
            "DELETE FROM {schema}.vectors WHERE namespace = $1"
        ))
        .bind(&namespace)
        .execute(&state.pool)
        .await?;
    } else if let Some(ids) = &req.ids {
        if ids.len() > 1000 {
            return Err(ApiError::InvalidArgument(
                "ids array cannot exceed 1000 entries".to_string(),
            ));
        }
        sqlx::query(&format!(
            "DELETE FROM {schema}.vectors WHERE namespace = $1 AND id = ANY($2::text[])"
        ))
        .bind(&namespace)
        .bind(ids)
        .execute(&state.pool)
        .await?;
    } else if let Some(filter) = &req.filter {
        let (filter_sql, filter_params) =
            crate::planner::filter_translator::translate_filter(filter, 1)
                .map_err(|e| ApiError::InvalidArgument(e.to_string()))?;
        let sql = format!("DELETE FROM {schema}.vectors WHERE namespace = $1 AND ({filter_sql})");
        let mut q = sqlx::query(&sql).bind(&namespace);
        for p in &filter_params {
            q = q.bind(p.as_str().unwrap_or(""));
        }
        q.execute(&state.pool).await?;
    } else {
        return Err(ApiError::InvalidArgument(
            "Provide ids, filter, or deleteAll=true".to_string(),
        ));
    }

    Ok((axum::http::StatusCode::OK, Json(serde_json::json!({}))))
}

// --- Update ---

#[derive(Deserialize)]
pub struct UpdateRequest {
    pub id: String,
    pub values: Option<Vec<f32>>,
    #[serde(rename = "setMetadata")]
    pub set_metadata: Option<serde_json::Value>,
    pub text: Option<String>,
    pub namespace: Option<String>,
}

/// POST /indexes/:name/vectors/update
pub async fn update_vector(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    Json(req): Json<UpdateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = resolve_index(&state.pool, &index_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let schema = &index.schema_name;

    let embedding_str = req.values.as_ref().map(|v| {
        format!(
            "[{}]",
            v.iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
    });

    // metadata is MERGED (JSONB ||), not replaced
    let result = sqlx::query(&format!(
        r#"
        UPDATE {schema}.vectors SET
            values       = CASE WHEN $3::text IS NOT NULL THEN $3::vector ELSE values END,
            text_content = CASE WHEN $4::text IS NOT NULL THEN $4       ELSE text_content END,
            metadata     = COALESCE(metadata, '{{}}'::jsonb) || COALESCE($5::jsonb, '{{}}'::jsonb),
            updated_at   = now()
        WHERE id = $1 AND namespace = $2
        RETURNING id
        "#
    ))
    .bind(&req.id)
    .bind(&namespace)
    .bind(embedding_str.as_deref())
    .bind(req.text.as_deref())
    .bind(&req.set_metadata)
    .fetch_optional(&state.pool)
    .await?;

    if result.is_none() {
        return Err(ApiError::NotFound(format!(
            "Vector '{}' not found in namespace '{}'.",
            req.id, namespace
        )));
    }

    Ok(Json(serde_json::json!({})))
}

// --- List ---

/// GET /indexes/:name/vectors/list
pub async fn list_vectors(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = resolve_index(&state.pool, &index_name).await?;
    let namespace = params.get("namespace").cloned().unwrap_or_default();
    let prefix = params.get("prefix").cloned().unwrap_or_default();
    let limit: i64 = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
        .min(1000);
    let cursor = params.get("paginationToken").cloned().unwrap_or_default();
    let schema = &index.schema_name;

    let rows = sqlx::query(&format!(
        "SELECT id FROM {schema}.vectors WHERE namespace = $1 AND id LIKE $2 AND id > $3 ORDER BY id LIMIT $4"
    ))
    .bind(&namespace)
    .bind(format!("{prefix}%"))
    .bind(&cursor)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;

    let ids: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let id: String = r.get("id");
            serde_json::json!({"id": id})
        })
        .collect();

    let next_token = if rows.len() == limit as usize {
        Some(rows.last().unwrap().get::<String, _>("id"))
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "vectors": ids,
        "namespace": namespace,
        "pagination": next_token.map(|t| serde_json::json!({"next": t})),
    })))
}

/// Parse pgvector's text representation "[1.0,2.0,3.0]" into Vec<f32>
pub fn parse_pgvector_str(s: &str) -> Vec<f32> {
    s.trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect()
}
