//! `POST /v1/collections/:name/search` — unified dense + hybrid retrieval.
//!
//! Auto-detects hybrid when `text` is set on a collection with `bm25Enabled`.
//! Caller can force the mode via `hybrid: true | false | { alpha?, bm25Weight? }`.
//!
//! `?explain=true` returns the compiled Plan as JSON without executing.

use axum::{
    extract::{Path, Query as AxumQuery, State},
    Json,
};
use serde::Deserialize;

use crate::error::ApiError;
use crate::handlers::query::{GroupedQueryResponse, QueryResponse};
use crate::handlers::records::resolve_collection;
use crate::planner::plan::{self, ExecutionResult, SearchRequest};
use crate::state::AppState;

/// Query-string options for /search.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchParams {
    /// Bypass the query-side embedding LRU. Only relevant when `text` is used.
    #[serde(default)]
    pub no_cache: bool,
    /// Compile the plan and return it as JSON without executing.
    #[serde(default)]
    pub explain: bool,
}

pub async fn search(
    State(state): State<AppState>,
    Path(collection_name): Path<String>,
    AxumQuery(params): AxumQuery<SearchParams>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let collection = resolve_collection(&state.pool, &collection_name).await?;
    let compiled = plan::compile_search(&req, &collection, &state, params.no_cache).await?;

    if params.explain {
        // Return the Plan as JSON. We do NOT execute. We do NOT translate the
        // filter; the JSON form is more useful to debuggers and avoids paying
        // the SQL translation cost when the user just wants to see the plan.
        return Ok(Json(serde_json::json!({ "plan": compiled })));
    }

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
