//! Plan executor — runs a [`Plan`] against the database and returns either a
//! flat list of matches or a grouped result.
//!
//! The executor is the single place stages compose. Source SQL lives in
//! [`super::sources`]; stage logic lives in [`super::stages`].

use crate::error::ApiError;
use crate::handlers::records::CollectionMeta;
use crate::state::AppState;

use super::ast::{ExecutionResult, Plan, Source, Stage};
use super::sources;
use super::stages;

pub async fn execute(
    state: &AppState,
    collection: &CollectionMeta,
    plan: Plan,
) -> Result<ExecutionResult, ApiError> {
    // 1. Decide what to fetch from the DB.
    //    - We always need `metadata` if any later stage depends on it.
    //    - We always need `values` if the user asked, *or* if we ever decide
    //      to feed them into a Stage (today no Stage uses values, so just
    //      mirror the user request).
    let fetch_metadata = plan.output.include_metadata || plan.stages_need_metadata();
    let fetch_values = plan.output.include_values;

    // 2. Materialise the candidate pool from the Source.
    let mut matches = match &plan.source {
        Source::Dense { vector } => {
            sources::dense::run(
                &state.pool,
                collection,
                vector,
                plan.retrieve_k,
                &plan.namespace,
                &plan.filter,
                fetch_values,
                fetch_metadata,
            )
            .await?
        }
        Source::Hybrid {
            vector,
            text,
            alpha,
            ..
        } => {
            sources::hybrid::run(
                &state.pool,
                collection,
                vector,
                text,
                *alpha,
                plan.retrieve_k,
                &plan.namespace,
                &plan.filter,
                fetch_values,
                fetch_metadata,
            )
            .await?
        }
    };

    // 3. Run stages in declared order. GroupBy is terminal.
    for stage in &plan.stages {
        match stage {
            Stage::Rerank {
                query,
                top_n,
                rank_field,
                model,
            } => {
                matches = stages::rerank::run(
                    &state.reranker,
                    matches,
                    query,
                    *top_n,
                    rank_field,
                    model.as_deref(),
                )
                .await?;
            }
            Stage::ScoreThreshold { min } => {
                matches = stages::threshold::run(matches, *min);
            }
            Stage::Dedup { by } => {
                matches = stages::dedup::run(matches, by);
            }
            Stage::Truncate { k } => {
                matches = stages::truncate::run(matches, *k);
            }
            Stage::GroupBy {
                field,
                limit,
                group_size,
            } => {
                let out = stages::group_by::run(matches, field, *limit, *group_size);
                if out.total_input > 0 && !out.field_seen {
                    return Err(ApiError::groupby_field_missing(field));
                }
                let mut groups = out.groups;
                trim_grouped_output(&mut groups, &plan);
                return Ok(ExecutionResult::Grouped { groups });
            }
        }
    }

    // 4. Trim output fields per OutputSpec on the flat path.
    trim_flat_output(&mut matches, &plan);
    Ok(ExecutionResult::Flat { matches })
}

fn trim_flat_output(matches: &mut [crate::handlers::query::Match], plan: &Plan) {
    if !plan.output.include_metadata {
        for m in matches.iter_mut() {
            m.metadata = None;
        }
    }
    if !plan.output.include_values {
        for m in matches.iter_mut() {
            m.values = None;
        }
    }
}

fn trim_grouped_output(groups: &mut [crate::handlers::query::GroupResult], plan: &Plan) {
    for g in groups.iter_mut() {
        trim_flat_output(&mut g.matches, plan);
    }
}
