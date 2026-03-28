use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use crate::{error::ApiError, planner::reranker::RerankCandidate, state::AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankOptions {
    /// Query text for the reranker. Required — this is the natural-language question,
    /// which may differ from the vector query (especially in hybrid search).
    pub query: String,
    /// Number of final results after reranking. Defaults to topK.
    pub top_n: Option<i64>,
    /// Which metadata field contains the text to rank against.
    /// Default: "text". Falls back to vector id if the field is absent.
    #[serde(default = "default_rank_field")]
    pub rank_field: String,
    /// Per-request model override. If set, uses this model instead of the server-side default.
    /// Ignored by cross-encoder backend (model is fixed in the deployment).
    pub model: Option<String>,
}

fn default_rank_field() -> String { "text".to_string() }

#[derive(Deserialize)]
pub struct QueryRequest {
    pub vector: Option<Vec<f32>>,
    pub id: Option<String>,
    #[serde(rename = "topK")]
    pub top_k: i64,
    pub namespace: Option<String>,
    pub filter: Option<serde_json::Value>,
    #[serde(rename = "includeValues", default)]
    pub include_values: bool,
    #[serde(rename = "includeMetadata", default)]
    pub include_metadata: bool,
    /// If present, reranking is performed after ANN retrieval.
    pub rerank: Option<RerankOptions>,
}

#[derive(Serialize)]
pub struct QueryResponse {
    pub results: Vec<serde_json::Value>,
    pub matches: Vec<Match>,
    pub namespace: String,
}

#[derive(Serialize)]
pub struct Match {
    pub id: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// POST /indexes/:name/query
pub async fn query_vectors(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiError> {
    if req.top_k < 1 || req.top_k > 10_000 {
        return Err(ApiError::InvalidArgument("topK must be between 1 and 10000".to_string()));
    }

    let index = crate::handlers::vectors::resolve_index(&state.pool, &index_name).await?;
    let namespace = req.namespace.clone().unwrap_or_default();

    // Resolve query vector -- either directly provided or looked up by ID
    let query_vec = if let Some(vec) = &req.vector {
        vec.clone()
    } else if let Some(id) = &req.id {
        let row = sqlx::query(&format!(
            "SELECT values::text FROM {}.vectors WHERE id = $1 AND namespace = $2",
            index.schema_name
        ))
        .bind(id)
        .bind(&namespace)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Vector '{id}' not found.")))?;
        crate::handlers::vectors::parse_pgvector_str(&row.get::<String, _>("values"))
    } else {
        return Err(ApiError::InvalidArgument("Provide either 'vector' or 'id'".to_string()));
    };

    // Build query vector string for SQL
    let vec_str = format!(
        "[{}]",
        query_vec.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(",")
    );

    // Select the distance operator based on metric -- see 00-reference.md section 4 and 5
    let dist_op = match index.metric.as_str() {
        "cosine"     => "<=>",
        "euclidean"  => "<->",
        "dotproduct" => "<#>",
        _            => "<=>",
    };

    // Build filter clause
    let (filter_sql, filter_params) = if let Some(f) = &req.filter {
        crate::planner::filter_translator::translate_filter(f, 3)
            .map_err(|e| ApiError::InvalidArgument(e.to_string()))?
    } else {
        ("TRUE".to_string(), vec![])
    };

    // When reranking: fetch top_k * 5 candidates to widen the reranker's pool,
    // capped at 10,000 (absolute max) and at the provider's per-request limit.
    let fetch_k = if req.rerank.is_some() {
        let provider_max = state.reranker.max_candidates();
        let provider_cap = if provider_max > 10_000 { 10_000i64 } else { provider_max as i64 };
        (req.top_k * 5).min(10_000).min(provider_cap)
    } else {
        req.top_k
    };

    let top_n = req
        .rerank
        .as_ref()
        .and_then(|r| r.top_n)
        .unwrap_or(req.top_k);

    // When reranking we always need metadata for the rank_field extraction
    let need_metadata = req.include_metadata || req.rerank.is_some();
    let values_col    = if req.include_values { "values::text" } else { "NULL::text AS values" };
    let metadata_col  = if need_metadata { "metadata" } else { "NULL::jsonb AS metadata" };

    // CRITICAL: ORDER BY must use the IDENTICAL operator expression as the SELECT distance column.
    // Using an alias in ORDER BY defeats DiskANN index usage. See 00-reference.md section 5.
    let sql = format!(
        r#"
        SELECT id, {values_col}, {metadata_col},
               values {dist_op} $1::vector AS distance
        FROM {}.vectors
        WHERE namespace = $2
          AND ({filter_sql})
        ORDER BY values {dist_op} $1::vector
        LIMIT $3
        "#,
        index.schema_name
    );

    let mut query = sqlx::query(&sql)
        .bind(&vec_str)
        .bind(&namespace)
        .bind(fetch_k);
    for p in &filter_params {
        query = query.bind(p.as_str().unwrap_or(""));
    }

    let rows = query.fetch_all(&state.pool).await?;

    // Convert distances to scores -- see 00-reference.md section 4
    let mut matches: Vec<Match> = rows.into_iter().map(|row| {
        let id: String = row.get("id");
        let distance: f64 = row.get("distance");
        let values_str: Option<String> = row.get("values");
        let metadata: Option<serde_json::Value> = row.get("metadata");

        let score = match index.metric.as_str() {
            "cosine"     => 1.0 - distance,
            "euclidean"  => 1.0 / (1.0 + distance),
            "dotproduct" => -distance,
            _            => 1.0 - distance,
        };

        Match {
            id,
            score,
            values: values_str.map(|s| crate::handlers::vectors::parse_pgvector_str(&s)),
            metadata,
        }
    }).collect();

    // Apply reranking if requested.
    if let Some(rerank_opts) = &req.rerank {
        let candidates: Vec<RerankCandidate> = matches
            .into_iter()
            .map(|m| {
                let text = m.metadata
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
                metadata: if req.include_metadata { r.metadata } else { None },
                values: r.values,
            })
            .collect();
    }

    Ok(Json(QueryResponse {
        results: vec![],
        matches,
        namespace,
    }))
}

/// POST /indexes/:name/query/hybrid
pub async fn query_hybrid(
    State(state): State<AppState>,
    axum::extract::Path(index_name): axum::extract::Path<String>,
    Json(req): Json<crate::planner::hybrid::HybridQueryRequest>,
) -> Result<Json<crate::planner::hybrid::HybridQueryResponse>, ApiError> {
    if req.top_k < 1 || req.top_k > 10_000 {
        return Err(ApiError::InvalidArgument("topK must be between 1 and 10000".to_string()));
    }

    let index = crate::handlers::vectors::resolve_index(&state.pool, &index_name).await?;

    if !index.bm25_enabled {
        return Err(ApiError::InvalidArgument(
            "Hybrid search requires bm25_enabled=true on this index. \
             Use PATCH /indexes/:name to enable it.".to_string()
        ));
    }

    let mut result = crate::planner::hybrid::hybrid_query(
        &state.pool,
        &index.schema_name,
        &req,
        &index.metric,
    ).await?;

    // Apply reranking if requested.
    if let Some(rerank_opts) = &req.rerank {
        let top_n = rerank_opts.top_n.unwrap_or(req.top_k);
        let candidates: Vec<RerankCandidate> = result
            .matches
            .into_iter()
            .map(|m| {
                let text = m.metadata
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
                metadata: if req.include_metadata { r.metadata } else { None },
                values: r.values,
            })
            .collect();
    }

    Ok(Json(result))
}
