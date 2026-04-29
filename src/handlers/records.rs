use crate::{error::ApiError, state::AppState};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;

pub struct CollectionMeta {
    pub id: uuid::Uuid,
    pub dimension: i32,
    pub metric: String,
    pub bm25_enabled: bool,
}

impl CollectionMeta {
    /// Returns the fully-qualified table reference for this collection.
    /// Format: "_onecortex.col_{uuid_simple}"
    pub fn table_ref(&self) -> String {
        format!("_onecortex.col_{}", self.id.simple())
    }
}

pub async fn resolve_collection(
    pool: &sqlx::PgPool,
    name: &str,
) -> Result<CollectionMeta, ApiError> {
    // Try direct collection lookup first
    let row = sqlx::query(
        "SELECT id, dimension, metric, bm25_enabled \
         FROM _onecortex_vector.collections WHERE name = $1 AND status = 'ready'",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        return Ok(CollectionMeta {
            id: row.get("id"),
            dimension: row.get("dimension"),
            metric: row.get("metric"),
            bm25_enabled: row.get("bm25_enabled"),
        });
    }

    // Fall back to alias resolution: alias -> collection_name -> collection record
    let alias_row = sqlx::query(
        "SELECT i.id, i.dimension, i.metric, i.bm25_enabled \
         FROM _onecortex_vector.aliases a \
         JOIN _onecortex_vector.collections i ON a.collection_name = i.name \
         WHERE a.alias = $1 AND i.status = 'ready'",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::collection_not_found(name))?;

    Ok(CollectionMeta {
        id: alias_row.get("id"),
        dimension: alias_row.get("dimension"),
        metric: alias_row.get("metric"),
        bm25_enabled: alias_row.get("bm25_enabled"),
    })
}

// --- Upsert ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertRequest {
    pub records: Vec<RecordInput>,
    pub namespace: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordInput {
    pub id: String,
    pub values: Vec<f32>,
    pub sparse_values: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    pub text: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertResponse {
    pub upserted_count: i64,
}

/// POST /collections/:name/records/upsert
pub async fn upsert_records(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<UpsertRequest>,
) -> Result<Json<UpsertResponse>, ApiError> {
    if req.records.len() > 1000 {
        return Err(ApiError::invalid_argument(
            "upsert batch cannot exceed 1000 records".to_string(),
        ));
    }
    if req.records.is_empty() {
        return Ok(Json(UpsertResponse { upserted_count: 0 }));
    }

    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.unwrap_or_default();

    // Validate all IDs and dimensions before any DB writes
    for r in &req.records {
        if r.id.len() > 512 {
            return Err(ApiError::invalid_argument(format!(
                "record id '{}...' exceeds 512 character limit",
                &r.id[..20.min(r.id.len())]
            )));
        }
        if r.values.len() != collection.dimension as usize {
            return Err(ApiError::invalid_argument(format!(
                "record '{}' has {} dimensions but collection expects {}",
                r.id,
                r.values.len(),
                collection.dimension
            )));
        }
        if r.sparse_values.is_some() {
            tracing::warn!(
                record_id = %r.id,
                "received sparseValues for record; sparse vectors are not supported and will be silently ignored"
            );
        }
    }

    let table = collection.table_ref();
    let upsert_sql = format!(
        "INSERT INTO {table} (id, namespace, values, text_content, metadata, updated_at) VALUES "
    );

    let mut qb = sqlx::QueryBuilder::new(upsert_sql);
    let mut sep = qb.separated(", ");
    for r in &req.records {
        let embedding_str = format!(
            "[{}]",
            r.values
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        sep.push("(");
        sep.push_bind_unseparated(&r.id);
        sep.push_unseparated(", ");
        sep.push_bind_unseparated(&namespace);
        sep.push_unseparated(format!(", '{embedding_str}'::vector, "));
        sep.push_bind_unseparated(r.text.as_deref());
        sep.push_unseparated(", ");
        sep.push_bind_unseparated(&r.metadata);
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

    let count = req.records.len() as i64;

    // Async stats update -- fire and forget, non-blocking
    let pool = state.pool.clone();
    let collection_id = collection.id;
    let ns = namespace.clone();
    let table_clone = table.clone();
    tokio::spawn(async move {
        let result = sqlx::query(&format!(
            r#"
            INSERT INTO _onecortex_vector.collection_stats (collection_id, namespace, record_count)
            SELECT $1, $2, COUNT(*) FROM {table_clone} WHERE namespace = $2
            ON CONFLICT (collection_id, namespace)
            DO UPDATE SET record_count = EXCLUDED.record_count, updated_at = now()
            "#
        ))
        .bind(collection_id)
        .bind(&ns)
        .execute(&pool)
        .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "Failed to update collection stats");
        }
    });

    Ok(Json(UpsertResponse {
        upserted_count: count,
    }))
}

// --- Fetch ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchResponse {
    pub namespace: String,
    pub records: Vec<FetchRecord>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchRequest {
    pub ids: Vec<String>,
    pub namespace: Option<String>,
}

/// POST /collections/:name/records/fetch
pub async fn fetch_records(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<FetchRequest>,
) -> Result<Json<FetchResponse>, ApiError> {
    if req.ids.len() > 1000 {
        return Err(ApiError::invalid_argument(
            "ids array cannot exceed 1000 entries".to_string(),
        ));
    }
    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.unwrap_or_default();

    let rows = sqlx::query(&format!(
        "SELECT id, values::text, metadata FROM {} WHERE namespace = $1 AND id = ANY($2::text[])",
        collection.table_ref()
    ))
    .bind(&namespace)
    .bind(&req.ids)
    .fetch_all(&state.pool)
    .await?;

    let records: Vec<FetchRecord> = rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            let values_str: Option<String> = row.get("values");
            let metadata: Option<serde_json::Value> = row.get("metadata");
            FetchRecord {
                id,
                values: values_str.map(|s| parse_pgvector_str(&s)),
                metadata,
            }
        })
        .collect();

    Ok(Json(FetchResponse {
        namespace,
        records,
        next_cursor: None,
    }))
}

// --- Fetch by metadata ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchByMetadataRequest {
    pub filter: serde_json::Value,
    pub namespace: Option<String>,
    pub limit: Option<i64>,
    pub include_values: Option<bool>,
    #[allow(dead_code)]
    pub include_metadata: Option<bool>,
}

/// POST /collections/:name/records/fetch_by_metadata
pub async fn fetch_by_metadata(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<FetchByMetadataRequest>,
) -> Result<Json<FetchResponse>, ApiError> {
    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let limit = req.limit.unwrap_or(100).min(1000);

    let (filter_sql, filter_params) =
        crate::planner::filter_translator::translate_filter(&req.filter, 1)?;

    let include_values = req.include_values.unwrap_or(false);
    let values_col = if include_values {
        "values::text"
    } else {
        "NULL::text AS values"
    };

    let sql = format!(
        "SELECT id, {values_col}, metadata FROM {} WHERE namespace = $1 AND ({filter_sql}) LIMIT {limit}",
        collection.table_ref()
    );

    let mut query = sqlx::query(&sql).bind(&namespace);
    for p in &filter_params {
        query = match p {
            serde_json::Value::String(s) => query.bind(s.as_str()),
            _ => query.bind(p.to_string()),
        };
    }

    let rows = query.fetch_all(&state.pool).await?;

    let records: Vec<FetchRecord> = rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            let values_str: Option<String> = row.get("values");
            let metadata: Option<serde_json::Value> = row.get("metadata");
            FetchRecord {
                id,
                values: if include_values {
                    values_str.map(|s| parse_pgvector_str(&s))
                } else {
                    None
                },
                metadata,
            }
        })
        .collect();

    Ok(Json(FetchResponse {
        namespace,
        records,
        next_cursor: None,
    }))
}

// --- Delete ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRequest {
    pub ids: Option<Vec<String>>,
    pub delete_all: Option<bool>,
    pub filter: Option<serde_json::Value>,
    pub namespace: Option<String>,
}

/// POST /collections/:name/records/delete
pub async fn delete_records(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<DeleteRequest>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let table = collection.table_ref();

    if req.delete_all == Some(true) {
        sqlx::query(&format!("DELETE FROM {table} WHERE namespace = $1"))
            .bind(&namespace)
            .execute(&state.pool)
            .await?;
    } else if let Some(ids) = &req.ids {
        if ids.len() > 1000 {
            return Err(ApiError::invalid_argument(
                "ids array cannot exceed 1000 entries".to_string(),
            ));
        }
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE namespace = $1 AND id = ANY($2::text[])"
        ))
        .bind(&namespace)
        .bind(ids)
        .execute(&state.pool)
        .await?;
    } else if let Some(filter) = &req.filter {
        let (filter_sql, filter_params) =
            crate::planner::filter_translator::translate_filter(filter, 1)?;
        let sql = format!("DELETE FROM {table} WHERE namespace = $1 AND ({filter_sql})");
        let mut q = sqlx::query(&sql).bind(&namespace);
        for p in &filter_params {
            q = match p {
                serde_json::Value::String(s) => q.bind(s.as_str()),
                _ => q.bind(p.to_string()),
            };
        }
        q.execute(&state.pool).await?;
    } else {
        return Err(ApiError::invalid_argument(
            "Provide ids, filter, or deleteAll=true".to_string(),
        ));
    }

    Ok((axum::http::StatusCode::OK, Json(serde_json::json!({}))))
}

// --- Update ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRequest {
    pub id: String,
    pub values: Option<Vec<f32>>,
    pub set_metadata: Option<serde_json::Value>,
    pub text: Option<String>,
    pub namespace: Option<String>,
}

/// POST /collections/:name/records/update
pub async fn update_record(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<UpdateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let table = collection.table_ref();

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
        UPDATE {table} SET
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
        return Err(ApiError::not_found(format!(
            "Record '{}' not found in namespace '{}'.",
            req.id, namespace
        )));
    }

    Ok(Json(serde_json::json!({})))
}

// --- List ---

/// GET /collections/:name/records/list
pub async fn list_records(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let namespace = params.get("namespace").cloned().unwrap_or_default();
    let prefix = params.get("prefix").cloned().unwrap_or_default();
    let limit: i64 = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
        .min(1000);
    let cursor = params.get("paginationToken").cloned().unwrap_or_default();
    let table = collection.table_ref();

    let rows = sqlx::query(&format!(
        "SELECT id FROM {table} WHERE namespace = $1 AND id LIKE $2 AND id > $3 ORDER BY id LIMIT $4"
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
        "records": ids,
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

// --- Scroll ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollRequest {
    pub namespace: Option<String>,
    pub filter: Option<serde_json::Value>,
    #[serde(default = "default_scroll_limit")]
    pub limit: i64,
    pub cursor: Option<String>,
    #[serde(default)]
    pub include_values: bool,
    #[serde(default = "default_include_true")]
    pub include_metadata: bool,
}

fn default_scroll_limit() -> i64 {
    100
}
fn default_include_true() -> bool {
    true
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollResponse {
    pub records: Vec<ScrollRecord>,
    pub namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollRecord {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// POST /collections/:name/records/scroll
pub async fn scroll_records(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<ScrollRequest>,
) -> Result<Json<ScrollResponse>, ApiError> {
    let limit = req.limit.clamp(1, 1000);
    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let table = collection.table_ref();
    let cursor = req.cursor.unwrap_or_default();

    let values_col = if req.include_values {
        "values::text"
    } else {
        "NULL::text AS values"
    };
    let metadata_col = if req.include_metadata {
        "metadata"
    } else {
        "NULL::jsonb AS metadata"
    };

    let (filter_sql, filter_params) = if let Some(f) = &req.filter {
        crate::planner::filter_translator::translate_filter(f, 2)?
    } else {
        ("TRUE".to_string(), vec![])
    };

    let fetch_limit = limit + 1;

    let sql = format!(
        r#"
        SELECT id, {values_col}, {metadata_col}
        FROM {table}
        WHERE namespace = $1
          AND id > $2
          AND ({filter_sql})
        ORDER BY id
        LIMIT {fetch_limit}
        "#
    );

    let mut query = sqlx::query(&sql).bind(&namespace).bind(&cursor);
    for p in &filter_params {
        query = match p {
            serde_json::Value::String(s) => query.bind(s.as_str()),
            _ => query.bind(p.to_string()),
        };
    }

    let rows = query.fetch_all(&state.pool).await?;

    let has_more = rows.len() as i64 > limit;
    let take_count = if has_more { limit as usize } else { rows.len() };

    let records: Vec<ScrollRecord> = rows
        .into_iter()
        .take(take_count)
        .map(|row| {
            let id: String = row.get("id");
            let values_str: Option<String> = row.get("values");
            let metadata: Option<serde_json::Value> = row.get("metadata");
            ScrollRecord {
                id,
                values: values_str.map(|s| parse_pgvector_str(&s)),
                metadata,
            }
        })
        .collect();

    let next_cursor = if has_more {
        records.last().map(|r| r.id.clone())
    } else {
        None
    };

    Ok(Json(ScrollResponse {
        records,
        namespace,
        next_cursor,
    }))
}

// --- Sample ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleRequest {
    pub namespace: Option<String>,
    pub filter: Option<serde_json::Value>,
    #[serde(default = "default_sample_size")]
    pub size: i64,
    #[serde(default)]
    pub include_values: bool,
    #[serde(default = "default_include_true")]
    pub include_metadata: bool,
}

fn default_sample_size() -> i64 {
    10
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleResponse {
    pub records: Vec<ScrollRecord>,
    pub namespace: String,
}

/// POST /collections/:name/sample
pub async fn sample_records(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<SampleRequest>,
) -> Result<Json<SampleResponse>, ApiError> {
    let size = req.size.clamp(1, 1000);
    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let table = collection.table_ref();

    let values_col = if req.include_values {
        "values::text"
    } else {
        "NULL::text AS values"
    };
    let metadata_col = if req.include_metadata {
        "metadata"
    } else {
        "NULL::jsonb AS metadata"
    };

    let (filter_sql, filter_params) = if let Some(f) = &req.filter {
        crate::planner::filter_translator::translate_filter(f, 1)?
    } else {
        ("TRUE".to_string(), vec![])
    };

    let sql = format!(
        r#"
        SELECT id, {values_col}, {metadata_col}
        FROM {table}
        WHERE namespace = $1
          AND ({filter_sql})
        ORDER BY random()
        LIMIT {size}
        "#
    );

    let mut query = sqlx::query(&sql).bind(&namespace);
    for p in &filter_params {
        query = match p {
            serde_json::Value::String(s) => query.bind(s.as_str()),
            _ => query.bind(p.to_string()),
        };
    }

    let rows = query.fetch_all(&state.pool).await?;

    let records: Vec<ScrollRecord> = rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            let values_str: Option<String> = row.get("values");
            let metadata: Option<serde_json::Value> = row.get("metadata");
            ScrollRecord {
                id,
                values: values_str.map(|s| parse_pgvector_str(&s)),
                metadata,
            }
        })
        .collect();

    Ok(Json(SampleResponse { records, namespace }))
}
