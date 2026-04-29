use crate::{error::ApiError, planner::reranker::RerankCandidate, state::AppState};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankOptions {
    /// Query text for the reranker. Required — this is the natural-language question,
    /// which may differ from the vector query (especially in hybrid search).
    pub query: String,
    /// Number of final results after reranking. Defaults to topK.
    pub top_n: Option<i64>,
    /// Which metadata field contains the text to rank against.
    /// Default: "text". Falls back to record id if the field is absent.
    #[serde(default = "default_rank_field")]
    pub rank_field: String,
    /// Per-request model override. If set, uses this model instead of the server-side default.
    /// Ignored by cross-encoder backend (model is fixed in the deployment).
    pub model: Option<String>,
}

fn default_rank_field() -> String {
    "text".to_string()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    pub vector: Option<Vec<f32>>,
    pub id: Option<String>,
    pub top_k: i64,
    pub namespace: Option<String>,
    pub filter: Option<serde_json::Value>,
    #[serde(default)]
    pub include_values: bool,
    #[serde(default)]
    pub include_metadata: bool,
    pub rerank: Option<RerankOptions>,
    pub score_threshold: Option<f64>,
    pub group_by: Option<GroupByOptions>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupByOptions {
    pub field: String,
    #[serde(default = "default_group_limit")]
    pub limit: usize,
    #[serde(default = "default_group_size")]
    pub group_size: usize,
}

fn default_group_limit() -> usize {
    10
}
fn default_group_size() -> usize {
    3
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    pub namespace: String,
    pub matches: Vec<Match>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupResult {
    pub key: String,
    pub matches: Vec<Match>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupedQueryResponse {
    pub namespace: String,
    pub grouped: bool,
    pub groups: Vec<GroupResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub id: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchQueryRequest {
    pub queries: Vec<QueryRequest>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchQueryResponse {
    pub results: Vec<serde_json::Value>,
}

/// POST /collections/:name/query
pub async fn query_vectors(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.top_k < 1 || req.top_k > 10_000 {
        return Err(ApiError::invalid_argument(
            "topK must be between 1 and 10000".to_string(),
        ));
    }
    if let Some(threshold) = req.score_threshold {
        if !(0.0..=1.0).contains(&threshold) {
            return Err(ApiError::invalid_argument(
                "scoreThreshold must be between 0.0 and 1.0".to_string(),
            ));
        }
    }
    if let Some(ref group_opts) = req.group_by {
        if group_opts.field.is_empty() {
            return Err(ApiError::invalid_argument(
                "groupBy.field must not be empty".to_string(),
            ));
        }
        if group_opts.limit == 0 || group_opts.limit > 100 {
            return Err(ApiError::invalid_argument(
                "groupBy.limit must be between 1 and 100".to_string(),
            ));
        }
        if group_opts.group_size == 0 || group_opts.group_size > 100 {
            return Err(ApiError::invalid_argument(
                "groupBy.groupSize must be between 1 and 100".to_string(),
            ));
        }
    }

    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.clone().unwrap_or_default();

    // Resolve query vector -- either directly provided or looked up by ID
    let query_vec = if let Some(vec) = &req.vector {
        vec.clone()
    } else if let Some(id) = &req.id {
        let row = sqlx::query(&format!(
            "SELECT values::text FROM {} WHERE id = $1 AND namespace = $2",
            collection.table_ref()
        ))
        .bind(id)
        .bind(&namespace)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Record '{id}' not found.")))?;
        crate::handlers::records::parse_pgvector_str(&row.get::<String, _>("values"))
    } else {
        return Err(ApiError::invalid_argument(
            "Provide either 'vector' or 'id'".to_string(),
        ));
    };

    // Build query vector string for SQL
    let vec_str = format!(
        "[{}]",
        query_vec
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    // Select the distance operator based on metric -- see 00-reference.md section 4 and 5
    let dist_op = match collection.metric.as_str() {
        "cosine" => "<=>",
        "euclidean" => "<->",
        "dotproduct" => "<#>",
        _ => "<=>",
    };

    // Build filter clause
    let (filter_sql, filter_params) = if let Some(f) = &req.filter {
        crate::planner::filter_translator::translate_filter(f, 3)
            .map_err(|e| ApiError::invalid_argument(e.to_string()))?
    } else {
        ("TRUE".to_string(), vec![])
    };

    // When reranking: fetch top_k * 5 candidates to widen the reranker's pool,
    // capped at 10,000 (absolute max) and at the provider's per-request limit.
    let fetch_k = if req.rerank.is_some() {
        let provider_max = state.reranker.max_candidates();
        let provider_cap = if provider_max > 10_000 {
            10_000i64
        } else {
            provider_max as i64
        };
        (req.top_k * 5).min(10_000).min(provider_cap)
    } else {
        req.top_k
    };

    // When grouping: over-fetch to ensure enough diverse groups
    let fetch_k = if req.group_by.is_some() {
        (fetch_k * 5).min(10_000)
    } else {
        fetch_k
    };

    let top_n = req
        .rerank
        .as_ref()
        .and_then(|r| r.top_n)
        .unwrap_or(req.top_k);

    // When reranking or grouping we always need metadata
    let need_metadata = req.include_metadata || req.rerank.is_some() || req.group_by.is_some();
    let values_col = if req.include_values {
        "values::text"
    } else {
        "NULL::text AS values"
    };
    let metadata_col = if need_metadata {
        "metadata"
    } else {
        "NULL::jsonb AS metadata"
    };

    // CRITICAL: ORDER BY must use the IDENTICAL operator expression as the SELECT distance column.
    // Using an alias in ORDER BY defeats DiskANN index usage. See 00-reference.md section 5.
    let sql = format!(
        r#"
        SELECT id, {values_col}, {metadata_col},
               values {dist_op} $1::vector AS distance
        FROM {}
        WHERE namespace = $2
          AND ({filter_sql})
        ORDER BY values {dist_op} $1::vector
        LIMIT $3
        "#,
        collection.table_ref()
    );

    let mut query = sqlx::query(&sql)
        .bind(&vec_str)
        .bind(&namespace)
        .bind(fetch_k);
    for p in &filter_params {
        query = match p {
            serde_json::Value::String(s) => query.bind(s.as_str()),
            _ => query.bind(p.to_string()),
        };
    }

    let rows = query.fetch_all(&state.pool).await?;

    // Convert distances to scores -- see 00-reference.md section 4
    let mut matches: Vec<Match> = rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            let distance: f64 = row.get("distance");
            let values_str: Option<String> = row.get("values");
            let metadata: Option<serde_json::Value> = row.get("metadata");

            let score = match collection.metric.as_str() {
                "cosine" => 1.0 - distance,
                "euclidean" => 1.0 / (1.0 + distance),
                "dotproduct" => -distance,
                _ => 1.0 - distance,
            };

            Match {
                id,
                score,
                values: values_str.map(|s| crate::handlers::records::parse_pgvector_str(&s)),
                metadata,
            }
        })
        .collect();

    // Apply reranking if requested.
    if let Some(rerank_opts) = &req.rerank {
        let candidates: Vec<RerankCandidate> = matches
            .into_iter()
            .map(|m| {
                let text = m
                    .metadata
                    .as_ref()
                    .and_then(|meta| meta.get(&rerank_opts.rank_field))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                RerankCandidate {
                    id: m.id,
                    score: m.score as f32,
                    text,
                    metadata: m.metadata,
                    values: m.values,
                }
            })
            .collect();

        let reranked = state
            .reranker
            .rerank(
                &rerank_opts.query,
                candidates,
                top_n as usize,
                rerank_opts.model.as_deref(),
            )
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!(e.to_string())))?;

        matches = reranked
            .into_iter()
            .map(|r| Match {
                id: r.id,
                score: r.rerank_score as f64,
                metadata: if req.include_metadata {
                    r.metadata
                } else {
                    None
                },
                values: r.values,
            })
            .collect();
    }

    // Apply score threshold filtering (after reranking if applicable)
    if let Some(threshold) = req.score_threshold {
        matches.retain(|m| m.score >= threshold);
    }

    // Grouping: bucket matches by a metadata field value
    if let Some(group_opts) = &req.group_by {
        let mut group_order: Vec<String> = Vec::new();
        let mut groups_map: std::collections::HashMap<String, Vec<Match>> =
            std::collections::HashMap::new();

        for m in matches {
            let group_key = m
                .metadata
                .as_ref()
                .and_then(|meta| meta.get(&group_opts.field))
                .map(|v| match v.as_str() {
                    Some(s) => s.to_string(),
                    None => v.to_string(),
                })
                .unwrap_or_default();

            if !groups_map.contains_key(&group_key) {
                group_order.push(group_key.clone());
            }
            let entry = groups_map.entry(group_key).or_default();
            if entry.len() < group_opts.group_size {
                entry.push(Match {
                    id: m.id,
                    score: m.score,
                    values: if req.include_values { m.values } else { None },
                    metadata: if req.include_metadata {
                        m.metadata
                    } else {
                        None
                    },
                });
            }
        }

        let groups: Vec<GroupResult> = group_order
            .into_iter()
            .take(group_opts.limit)
            .map(|key| GroupResult {
                matches: groups_map.remove(&key).unwrap_or_default(),
                key,
            })
            .collect();

        return Ok(Json(
            serde_json::to_value(GroupedQueryResponse {
                namespace,
                grouped: true,
                groups,
            })
            .unwrap(),
        ));
    }

    Ok(Json(
        serde_json::to_value(QueryResponse { namespace, matches }).unwrap(),
    ))
}

/// POST /collections/:name/query/hybrid
pub async fn query_hybrid(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<crate::planner::hybrid::HybridQueryRequest>,
) -> Result<Json<crate::planner::hybrid::HybridQueryResponse>, ApiError> {
    if req.top_k < 1 || req.top_k > 10_000 {
        return Err(ApiError::invalid_argument(
            "topK must be between 1 and 10000".to_string(),
        ));
    }
    if let Some(threshold) = req.score_threshold {
        if !(0.0..=1.0).contains(&threshold) {
            return Err(ApiError::invalid_argument(
                "scoreThreshold must be between 0.0 and 1.0".to_string(),
            ));
        }
    }

    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;

    if !collection.bm25_enabled {
        return Err(ApiError::invalid_argument(
            "Hybrid search requires bm25_enabled=true on this collection. \
             Use PATCH /collections/:name to enable it."
                .to_string(),
        ));
    }

    let mut result = crate::planner::hybrid::hybrid_query(
        &state.pool,
        &collection.table_ref(),
        &req,
        &collection.metric,
    )
    .await?;

    // Apply reranking if requested.
    if let Some(rerank_opts) = &req.rerank {
        let top_n = rerank_opts.top_n.unwrap_or(req.top_k);
        let candidates: Vec<RerankCandidate> = result
            .matches
            .into_iter()
            .map(|m| {
                let text = m
                    .metadata
                    .as_ref()
                    .and_then(|meta| meta.get(&rerank_opts.rank_field))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                RerankCandidate {
                    id: m.id,
                    score: m.score as f32,
                    text,
                    metadata: m.metadata,
                    values: m.values,
                }
            })
            .collect();

        let reranked = state
            .reranker
            .rerank(
                &rerank_opts.query,
                candidates,
                top_n as usize,
                rerank_opts.model.as_deref(),
            )
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!(e.to_string())))?;

        result.matches = reranked
            .into_iter()
            .map(|r| crate::planner::hybrid::HybridMatch {
                id: r.id,
                score: r.rerank_score as f64,
                metadata: if req.include_metadata {
                    r.metadata
                } else {
                    None
                },
                values: r.values,
            })
            .collect();
    }

    if let Some(threshold) = req.score_threshold {
        result.matches.retain(|m| m.score >= threshold);
    }

    Ok(Json(result))
}

/// POST /collections/:name/query/batch
pub async fn query_batch(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<BatchQueryRequest>,
) -> Result<Json<BatchQueryResponse>, ApiError> {
    if req.queries.is_empty() {
        return Err(ApiError::invalid_argument(
            "queries array must not be empty".to_string(),
        ));
    }
    if req.queries.len() > 10 {
        return Err(ApiError::invalid_argument(
            "queries array cannot exceed 10 entries".to_string(),
        ));
    }

    let mut handles = Vec::with_capacity(req.queries.len());
    for single_req in req.queries {
        let s = state.clone();
        let name = collection_name.clone();
        handles.push(tokio::spawn(async move {
            query_vectors(State(s), axum::extract::Path(name), Json(single_req)).await
        }));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        let res = handle
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("Task join error: {e}")))?;
        let Json(query_resp) = res?;
        results.push(query_resp);
    }

    Ok(Json(BatchQueryResponse { results }))
}

/// Internal: execute a dense ANN query and return scored matches.
/// Handles distance→score conversion but NOT reranking, score threshold, or grouping.
#[allow(clippy::too_many_arguments)]
async fn execute_ann_query(
    pool: &sqlx::PgPool,
    collection: &crate::handlers::records::CollectionMeta,
    query_vec: &[f32],
    top_k: i64,
    namespace: &str,
    filter: &Option<serde_json::Value>,
    include_values: bool,
    include_metadata: bool,
) -> Result<Vec<Match>, ApiError> {
    let vec_str = format!(
        "[{}]",
        query_vec
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    let dist_op = match collection.metric.as_str() {
        "cosine" => "<=>",
        "euclidean" => "<->",
        "dotproduct" => "<#>",
        _ => "<=>",
    };

    let (filter_sql, filter_params) = if let Some(f) = filter {
        crate::planner::filter_translator::translate_filter(f, 3)
            .map_err(|e| ApiError::invalid_argument(e.to_string()))?
    } else {
        ("TRUE".to_string(), vec![])
    };

    let values_col = if include_values {
        "values::text"
    } else {
        "NULL::text AS values"
    };
    let metadata_col = if include_metadata {
        "metadata"
    } else {
        "NULL::jsonb AS metadata"
    };

    let sql = format!(
        r#"
        SELECT id, {values_col}, {metadata_col},
               values {dist_op} $1::vector AS distance
        FROM {}
        WHERE namespace = $2
          AND ({filter_sql})
        ORDER BY values {dist_op} $1::vector
        LIMIT $3
        "#,
        collection.table_ref()
    );

    let mut query = sqlx::query(&sql).bind(&vec_str).bind(namespace).bind(top_k);
    for p in &filter_params {
        query = match p {
            serde_json::Value::String(s) => query.bind(s.as_str()),
            _ => query.bind(p.to_string()),
        };
    }

    let rows = query.fetch_all(pool).await?;

    let matches = rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            let distance: f64 = row.get("distance");
            let values_str: Option<String> = row.get("values");
            let metadata: Option<serde_json::Value> = row.get("metadata");

            let score = match collection.metric.as_str() {
                "cosine" => 1.0 - distance,
                "euclidean" => 1.0 / (1.0 + distance),
                "dotproduct" => -distance,
                _ => 1.0 - distance,
            };

            Match {
                id,
                score,
                values: values_str.map(|s| crate::handlers::records::parse_pgvector_str(&s)),
                metadata,
            }
        })
        .collect();

    Ok(matches)
}

// --- Faceted Counts ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetsRequest {
    pub field: String,
    pub filter: Option<serde_json::Value>,
    pub namespace: Option<String>,
    #[serde(default = "default_facet_limit")]
    pub limit: i64,
}

fn default_facet_limit() -> i64 {
    20
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetEntry {
    pub value: String,
    pub count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetsResponse {
    pub facets: Vec<FacetEntry>,
    pub field: String,
    pub namespace: String,
}

/// POST /collections/:name/facets
pub async fn facets(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<FacetsRequest>,
) -> Result<Json<FacetsResponse>, ApiError> {
    // Validate field name — it is embedded directly in SQL (JSONB operators cannot be parameterized)
    if req.field.is_empty() || req.field.len() > 100 {
        return Err(ApiError::invalid_argument(
            "field must be between 1 and 100 characters".to_string(),
        ));
    }
    let valid = {
        let mut chars = req.field.chars();
        let first_ok = chars
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false);
        first_ok && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    };
    if !valid {
        return Err(ApiError::invalid_argument(
            "field must start with a letter or underscore and contain only letters, digits, underscores, or dots".to_string(),
        ));
    }
    if req.limit < 1 || req.limit > 100 {
        return Err(ApiError::invalid_argument(
            "limit must be between 1 and 100".to_string(),
        ));
    }

    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.unwrap_or_default();
    let table = collection.table_ref();
    let limit = req.limit;

    let field_accessor = crate::planner::filter_translator::jsonb_field_accessor(&req.field);

    let (filter_sql, filter_params) = if let Some(f) = &req.filter {
        crate::planner::filter_translator::translate_filter(f, 1)
            .map_err(|e| ApiError::invalid_argument(e.to_string()))?
    } else {
        ("TRUE".to_string(), vec![])
    };

    let sql = format!(
        r#"
        SELECT {field_accessor} AS value, COUNT(*) AS count
        FROM {table}
        WHERE namespace = $1
          AND ({filter_sql})
          AND {field_accessor} IS NOT NULL
        GROUP BY {field_accessor}
        ORDER BY count DESC
        LIMIT {limit}
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

    let facet_entries: Vec<FacetEntry> = rows
        .into_iter()
        .map(|row| {
            let value: String = row.get("value");
            let count: i64 = row.get("count");
            FacetEntry { value, count }
        })
        .collect();

    Ok(Json(FacetsResponse {
        facets: facet_entries,
        field: req.field,
        namespace,
    }))
}

// --- Recommendation API ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendRequest {
    pub positive_ids: Vec<String>,
    #[serde(default)]
    pub negative_ids: Vec<String>,
    pub top_k: i64,
    pub namespace: Option<String>,
    pub filter: Option<serde_json::Value>,
    #[serde(default)]
    pub include_values: bool,
    #[serde(default)]
    pub include_metadata: bool,
    pub score_threshold: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendResponse {
    pub matches: Vec<Match>,
    pub namespace: String,
}

/// POST /collections/:name/recommend
pub async fn recommend(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<RecommendRequest>,
) -> Result<Json<RecommendResponse>, ApiError> {
    if req.positive_ids.is_empty() {
        return Err(ApiError::invalid_argument(
            "positiveIds must contain at least one ID".to_string(),
        ));
    }
    if req.positive_ids.len() + req.negative_ids.len() > 100 {
        return Err(ApiError::invalid_argument(
            "Total positive + negative IDs cannot exceed 100".to_string(),
        ));
    }
    if req.top_k < 1 || req.top_k > 10_000 {
        return Err(ApiError::invalid_argument(
            "topK must be between 1 and 10000".to_string(),
        ));
    }

    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;
    let namespace = req.namespace.clone().unwrap_or_default();
    let table = collection.table_ref();
    let dim = collection.dimension as usize;

    // Fetch all positive and negative vectors
    let all_ids: Vec<&str> = req
        .positive_ids
        .iter()
        .chain(req.negative_ids.iter())
        .map(|s| s.as_str())
        .collect();

    let rows = sqlx::query(&format!(
        "SELECT id, values::text FROM {table} WHERE namespace = $1 AND id = ANY($2::text[])"
    ))
    .bind(&namespace)
    .bind(&all_ids)
    .fetch_all(&state.pool)
    .await?;

    let mut vec_map: std::collections::HashMap<String, Vec<f32>> = std::collections::HashMap::new();
    for row in &rows {
        let id: String = row.get("id");
        let values_str: String = row.get("values");
        vec_map.insert(
            id,
            crate::handlers::records::parse_pgvector_str(&values_str),
        );
    }

    // Verify all positive IDs were found
    for pid in &req.positive_ids {
        if !vec_map.contains_key(pid) {
            return Err(ApiError::not_found(format!(
                "Positive record '{pid}' not found in namespace '{namespace}'."
            )));
        }
    }

    // Compute synthetic query vector: mean(positives) - mean(negatives)
    let mut synthetic = vec![0.0f32; dim];

    let pos_count = req.positive_ids.len() as f32;
    for pid in &req.positive_ids {
        let v = &vec_map[pid];
        for (i, val) in v.iter().enumerate() {
            if i < dim {
                synthetic[i] += val / pos_count;
            }
        }
    }

    if !req.negative_ids.is_empty() {
        let neg_count = req.negative_ids.len() as f32;
        for nid in &req.negative_ids {
            if let Some(v) = vec_map.get(nid) {
                for (i, val) in v.iter().enumerate() {
                    if i < dim {
                        synthetic[i] -= val / neg_count;
                    }
                }
            }
        }
    }

    // Run ANN search with synthetic vector (extra results to compensate for filtering out input IDs)
    let extra = (req.positive_ids.len() + req.negative_ids.len()) as i64;
    let fetch_k = (req.top_k + extra).min(10_000);

    let mut matches = execute_ann_query(
        &state.pool,
        &collection,
        &synthetic,
        fetch_k,
        &namespace,
        &req.filter,
        req.include_values,
        req.include_metadata,
    )
    .await?;

    // Exclude input IDs from results
    let exclude: std::collections::HashSet<&str> = req
        .positive_ids
        .iter()
        .chain(req.negative_ids.iter())
        .map(|s| s.as_str())
        .collect();
    matches.retain(|m| !exclude.contains(m.id.as_str()));

    // Truncate to requested top_k
    matches.truncate(req.top_k as usize);

    // Apply score threshold
    if let Some(threshold) = req.score_threshold {
        matches.retain(|m| m.score >= threshold);
    }

    Ok(Json(RecommendResponse { matches, namespace }))
}
