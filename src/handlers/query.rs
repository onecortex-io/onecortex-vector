use crate::{
    error::ApiError,
    planner::plan::{self, ExecutionResult},
    state::AppState,
};
use axum::{
    extract::{Query as AxumQuery, State},
    Json,
};
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
    /// Natural-language query text. Embedded server-side via the collection's
    /// bound embedder. Mutually exclusive with `vector` and `id`.
    pub text: Option<String>,
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

/// Query-string options accepted alongside the JSON body.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueryParams {
    /// Bypass the query-side embedding LRU. Only relevant when `text` is used.
    #[serde(default)]
    pub no_cache: bool,
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

/// POST /collections/:name/query — dense ANN. Compiles to a Plan with a
/// fixed `Source::Dense` and runs through the shared executor.
pub async fn query_vectors(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    AxumQuery(params): AxumQuery<QueryParams>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;
    let compiled = plan::compile_query(&req, &collection, &state, params.no_cache).await?;
    let namespace = compiled.namespace.clone();
    match plan::execute(&state, &collection, compiled).await? {
        ExecutionResult::Flat { matches } => Ok(Json(
            serde_json::to_value(QueryResponse { namespace, matches }).unwrap(),
        )),
        ExecutionResult::Grouped { groups } => Ok(Json(
            serde_json::to_value(GroupedQueryResponse {
                namespace,
                grouped: true,
                groups,
            })
            .unwrap(),
        )),
    }
}

/// POST /collections/:name/query/hybrid — dense + BM25 RRF. Compiles to a
/// Plan with a fixed `Source::Hybrid`. Vector + text are still required here
/// (preserving backwards compat); `/search` is the path for text-only hybrid.
pub async fn query_hybrid(
    State(state): State<AppState>,
    axum::extract::Path(collection_name): axum::extract::Path<String>,
    Json(req): Json<crate::planner::hybrid::HybridQueryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let collection =
        crate::handlers::records::resolve_collection(&state.pool, &collection_name).await?;
    // Ensure the legacy error message includes the collection name (compile_hybrid
    // only knows the collection meta, not the request path component).
    if !collection.bm25_enabled {
        return Err(ApiError::hybrid_requires_bm25(&collection_name));
    }
    let compiled = plan::compile_hybrid(&req, &collection).await?;
    let namespace = compiled.namespace.clone();
    match plan::execute(&state, &collection, compiled).await? {
        ExecutionResult::Flat { matches } => Ok(Json(
            serde_json::to_value(QueryResponse { namespace, matches }).unwrap(),
        )),
        ExecutionResult::Grouped { groups } => Ok(Json(
            serde_json::to_value(GroupedQueryResponse {
                namespace,
                grouped: true,
                groups,
            })
            .unwrap(),
        )),
    }
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
            query_vectors(
                State(s),
                axum::extract::Path(name),
                AxumQuery(QueryParams::default()),
                Json(single_req),
            )
            .await
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
/// Used by `recommend`. The unified `/search` and `/query` paths go through the
/// Plan executor instead — see `crate::planner::plan::sources::dense`.
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
        crate::planner::filter_translator::translate_filter(f, 3)?
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
        return Err(ApiError::facet_field_invalid(
            &req.field,
            "must be between 1 and 100 characters",
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
        return Err(ApiError::facet_field_invalid(
            &req.field,
            "must start with a letter or underscore and contain only letters, digits, underscores, or dots",
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
        crate::planner::filter_translator::translate_filter(f, 1)?
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
