use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use crate::{error::ApiError, state::AppState};

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

    let values_col    = if req.include_values { "values::text" } else { "NULL::text AS values" };
    let metadata_col  = if req.include_metadata { "metadata" } else { "NULL::jsonb AS metadata" };

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
        .bind(req.top_k);
    for p in &filter_params {
        query = query.bind(p.as_str().unwrap_or(""));
    }

    let rows = query.fetch_all(&state.pool).await?;

    // Convert distances to scores -- see 00-reference.md section 4
    let matches = rows.into_iter().map(|row| {
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

    Ok(Json(QueryResponse {
        results: vec![],
        matches,
        namespace,
    }))
}

/// POST /indexes/:name/query/hybrid -- Phase 3 stub
pub async fn query_hybrid(
    State(_state): State<AppState>,
    axum::extract::Path(_index_name): axum::extract::Path<String>,
    Json(_req): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::InvalidArgument("Hybrid search is not yet implemented. Coming in Phase 3.".to_string()))
}
