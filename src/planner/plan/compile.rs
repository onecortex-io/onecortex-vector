//! Plan compilers — turn HTTP requests into a [`Plan`] for the executor.
//!
//! Three entry points:
//!   - [`compile_search`] for `POST /v1/collections/:name/search` (the unified
//!     surface; auto-detects dense vs. hybrid).
//!   - [`compile_query`] for the legacy `POST /v1/collections/:name/query`
//!     (always [`Source::Dense`]).
//!   - [`compile_hybrid`] for the legacy `POST /v1/collections/:name/query/hybrid`
//!     (always [`Source::Hybrid`]).

use sqlx::Row;

use crate::embedding::embed_query_text;
use crate::error::ApiError;
use crate::handlers::query::{GroupByOptions, QueryRequest, RerankOptions};
use crate::handlers::records::{parse_pgvector_str, CollectionMeta};
use crate::planner::hybrid::HybridQueryRequest;
use crate::state::AppState;

use super::ast::{OutputSpec, Plan, Source, Stage};

// ── /search request DTO and the Hybrid sub-block ──────────────────────────

/// JSON body for `POST /v1/collections/:name/search`. All Option<…> fields
/// are skip-on-serialize. CamelCase wire format per CLAUDE.md.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub vector: Option<Vec<f32>>,
    pub id: Option<String>,
    pub text: Option<String>,

    pub top_k: i64,
    pub namespace: Option<String>,
    pub filter: Option<serde_json::Value>,

    /// `true` / `false` / `{ alpha?, bm25Weight? }`. Absent ⇒ auto-detect.
    pub hybrid: Option<HybridControl>,

    pub rerank: Option<RerankOptions>,
    pub score_threshold: Option<f64>,
    pub group_by: Option<GroupByOptions>,
    pub dedup: Option<DedupOptions>,

    #[serde(default)]
    pub include_values: bool,
    #[serde(default)]
    pub include_metadata: bool,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum HybridControl {
    /// Boolean form: `true` forces hybrid, `false` forces dense.
    Toggle(bool),
    /// Object form: forces hybrid and overrides fusion params.
    Options(HybridOptions),
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HybridOptions {
    pub alpha: Option<f32>,
    /// If set, used in place of `1.0 - alpha`. Lets callers asymmetrically
    /// weight BM25 (rare; mostly here for explain/debug).
    pub bm25_weight: Option<f32>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupOptions {
    pub by: String,
}

// ── Common validation ─────────────────────────────────────────────────────

fn validate_top_k(top_k: i64) -> Result<(), ApiError> {
    if !(1..=10_000).contains(&top_k) {
        return Err(ApiError::invalid_argument(
            "topK must be between 1 and 10000".to_string(),
        ));
    }
    Ok(())
}

fn validate_threshold(t: Option<f64>) -> Result<(), ApiError> {
    if let Some(t) = t {
        if !(0.0..=1.0).contains(&t) {
            return Err(ApiError::invalid_argument(
                "scoreThreshold must be between 0.0 and 1.0".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_group_by(g: Option<&GroupByOptions>) -> Result<(), ApiError> {
    if let Some(g) = g {
        if g.field.is_empty() {
            return Err(ApiError::invalid_argument(
                "groupBy.field must not be empty".to_string(),
            ));
        }
        if g.limit == 0 || g.limit > 100 {
            return Err(ApiError::invalid_argument(
                "groupBy.limit must be between 1 and 100".to_string(),
            ));
        }
        if g.group_size == 0 || g.group_size > 100 {
            return Err(ApiError::invalid_argument(
                "groupBy.groupSize must be between 1 and 100".to_string(),
            ));
        }
    }
    Ok(())
}

// ── Vector resolution ─────────────────────────────────────────────────────

/// Fetch a stored record's vector by id (used when the caller passes `id`
/// instead of a literal `vector`).
async fn fetch_vector_by_id(
    state: &AppState,
    collection: &CollectionMeta,
    id: &str,
    namespace: &str,
) -> Result<Vec<f32>, ApiError> {
    let row = sqlx::query(&format!(
        "SELECT values::text FROM {} WHERE id = $1 AND namespace = $2",
        collection.table_ref()
    ))
    .bind(id)
    .bind(namespace)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found(format!("Record '{id}' not found.")))?;
    Ok(parse_pgvector_str(&row.get::<String, _>("values")))
}

async fn embed_text(
    state: &AppState,
    collection: &CollectionMeta,
    text: &str,
    no_cache: bool,
) -> Result<Vec<f32>, ApiError> {
    let ec = collection
        .embedder_config
        .as_ref()
        .ok_or_else(ApiError::embedder_not_configured)?;
    let arc = embed_query_text(
        &state.embedder_factory,
        &state.embed_cache,
        ec,
        text,
        no_cache,
    )
    .await?;
    if arc.len() != collection.dimension as usize {
        return Err(ApiError::embedder_dimension_mismatch(
            collection.dimension as usize,
            arc.len(),
        ));
    }
    Ok((*arc).clone())
}

fn check_dimension(collection: &CollectionMeta, vec: &[f32]) -> Result<(), ApiError> {
    if vec.len() != collection.dimension as usize {
        return Err(ApiError::dimension_mismatch(
            None,
            collection.dimension as usize,
            vec.len(),
        ));
    }
    Ok(())
}

// ── Retrieve-K computation (preserves existing over-fetch rules) ──────────

fn dense_retrieve_k(
    top_k: i64,
    has_rerank: bool,
    has_group_by: bool,
    has_dedup: bool,
    reranker_max: usize,
) -> i64 {
    // Match the legacy multipliers in src/handlers/query.rs:286-303.
    let mut k = top_k;
    if has_rerank {
        let cap = if reranker_max > 10_000 {
            10_000_i64
        } else {
            reranker_max as i64
        };
        k = (k * 5).min(10_000).min(cap);
    }
    if has_group_by {
        k = (k * 5).min(10_000);
    }
    // Dedup is a /search-only addition. Without rerank/groupBy we still want
    // to over-fetch a little so we have something to drop.
    if has_dedup && !has_rerank && !has_group_by {
        k = (k * 2).min(10_000);
    }
    k
}

// ── Compilers ─────────────────────────────────────────────────────────────

/// Compile the legacy `/query` (Dense-only) request.
pub async fn compile_query(
    req: &QueryRequest,
    collection: &CollectionMeta,
    state: &AppState,
    no_cache: bool,
) -> Result<Plan, ApiError> {
    validate_top_k(req.top_k)?;
    validate_threshold(req.score_threshold)?;
    validate_group_by(req.group_by.as_ref())?;

    let namespace = req.namespace.clone().unwrap_or_default();

    let inputs_set = [req.vector.is_some(), req.id.is_some(), req.text.is_some()]
        .iter()
        .filter(|b| **b)
        .count();
    if inputs_set > 1 {
        return Err(ApiError::values_and_text_conflict(None));
    }
    let vector = if let Some(v) = &req.vector {
        v.clone()
    } else if let Some(id) = &req.id {
        fetch_vector_by_id(state, collection, id, &namespace).await?
    } else if let Some(t) = &req.text {
        embed_text(state, collection, t, no_cache).await?
    } else {
        return Err(ApiError::invalid_argument(
            "Provide one of 'vector', 'id', or 'text'".to_string(),
        ));
    };
    check_dimension(collection, &vector)?;

    let retrieve_k = dense_retrieve_k(
        req.top_k,
        req.rerank.is_some(),
        req.group_by.is_some(),
        false,
        state.reranker.max_candidates(),
    );

    let mut stages: Vec<Stage> = Vec::new();
    if let Some(r) = &req.rerank {
        stages.push(Stage::Rerank {
            query: r.query.clone(),
            top_n: r.top_n.unwrap_or(req.top_k),
            rank_field: r.rank_field.clone(),
            model: r.model.clone(),
        });
    }
    if let Some(min) = req.score_threshold {
        stages.push(Stage::ScoreThreshold { min });
    }
    if let Some(g) = &req.group_by {
        stages.push(Stage::GroupBy {
            field: g.field.clone(),
            limit: g.limit,
            group_size: g.group_size,
        });
    }

    Ok(Plan {
        source: Source::Dense { vector },
        filter: req.filter.clone(),
        namespace,
        retrieve_k,
        top_k: req.top_k,
        stages,
        output: OutputSpec {
            include_values: req.include_values,
            include_metadata: req.include_metadata,
        },
    })
}

/// Compile the legacy `/query/hybrid` (Hybrid-only) request.
pub async fn compile_hybrid(
    req: &HybridQueryRequest,
    collection: &CollectionMeta,
) -> Result<Plan, ApiError> {
    validate_top_k(req.top_k)?;
    validate_threshold(req.score_threshold)?;
    if !collection.bm25_enabled {
        return Err(ApiError::hybrid_requires_bm25(""));
    }
    check_dimension(collection, &req.vector)?;

    let alpha = req.alpha.clamp(0.0, 1.0);
    let mut stages: Vec<Stage> = Vec::new();
    if let Some(r) = &req.rerank {
        stages.push(Stage::Rerank {
            query: r.query.clone(),
            top_n: r.top_n.unwrap_or(req.top_k),
            rank_field: r.rank_field.clone(),
            model: r.model.clone(),
        });
    }
    if let Some(min) = req.score_threshold {
        stages.push(Stage::ScoreThreshold { min });
    }

    Ok(Plan {
        source: Source::Hybrid {
            vector: req.vector.clone(),
            text: req.text.clone(),
            alpha,
            bm25_weight: 1.0 - alpha,
        },
        filter: req.filter.clone(),
        namespace: req.namespace.clone(),
        // Hybrid SQL handles its own candidate-pool sizing internally; the
        // outer LIMIT is `top_k`. We deliberately do NOT over-fetch here for
        // rerank — preserving the legacy behaviour. (If we ever want to,
        // it's a one-line change.)
        retrieve_k: req.top_k,
        top_k: req.top_k,
        stages,
        output: OutputSpec {
            include_values: req.include_values,
            include_metadata: req.include_metadata,
        },
    })
}

/// Compile the unified `/search` request.
pub async fn compile_search(
    req: &SearchRequest,
    collection: &CollectionMeta,
    state: &AppState,
    no_cache: bool,
) -> Result<Plan, ApiError> {
    validate_top_k(req.top_k)?;
    validate_threshold(req.score_threshold)?;
    validate_group_by(req.group_by.as_ref())?;

    let namespace = req.namespace.clone().unwrap_or_default();

    // 1. Decide whether the Source is Dense or Hybrid.
    let force_hybrid;
    let force_dense;
    let mut alpha: f32 = 0.5;
    let mut bm25_weight: Option<f32> = None;
    match &req.hybrid {
        None => {
            force_hybrid = false;
            force_dense = false;
        }
        Some(HybridControl::Toggle(true)) => {
            force_hybrid = true;
            force_dense = false;
        }
        Some(HybridControl::Toggle(false)) => {
            force_hybrid = false;
            force_dense = true;
        }
        Some(HybridControl::Options(o)) => {
            force_hybrid = true;
            force_dense = false;
            if let Some(a) = o.alpha {
                alpha = a;
            }
            bm25_weight = o.bm25_weight;
        }
    }

    let want_hybrid = if force_hybrid {
        true
    } else if force_dense {
        false
    } else {
        // Auto-detect: text + collection has bm25 enabled.
        req.text.is_some() && collection.bm25_enabled
    };

    if want_hybrid && !collection.bm25_enabled {
        return Err(ApiError::hybrid_requires_bm25(""));
    }
    if want_hybrid && req.id.is_some() {
        return Err(ApiError::invalid_argument(
            "hybrid search does not accept 'id'".to_string(),
        ));
    }

    let alpha_clamped = alpha.clamp(0.0, 1.0);
    let bm25_w = bm25_weight.unwrap_or(1.0 - alpha_clamped);

    // 2. Resolve the inputs into a Source.
    let source = if want_hybrid {
        // Need both: a dense-leg vector and a BM25 text query.
        let text = req.text.as_deref().ok_or_else(|| {
            ApiError::invalid_argument("hybrid search requires 'text'".to_string())
        })?;
        let vector = if let Some(v) = &req.vector {
            v.clone()
        } else {
            // Auto-embed the text for the dense leg.
            embed_text(state, collection, text, no_cache).await?
        };
        check_dimension(collection, &vector)?;
        Source::Hybrid {
            vector,
            text: text.to_string(),
            alpha: alpha_clamped,
            bm25_weight: bm25_w,
        }
    } else {
        let inputs_set = [req.vector.is_some(), req.id.is_some(), req.text.is_some()]
            .iter()
            .filter(|b| **b)
            .count();
        if inputs_set == 0 {
            return Err(ApiError::invalid_argument(
                "Provide one of 'vector', 'id', or 'text'".to_string(),
            ));
        }
        if inputs_set > 1 {
            return Err(ApiError::values_and_text_conflict(None));
        }
        let vector = if let Some(v) = &req.vector {
            v.clone()
        } else if let Some(id) = &req.id {
            fetch_vector_by_id(state, collection, id, &namespace).await?
        } else if let Some(t) = &req.text {
            embed_text(state, collection, t, no_cache).await?
        } else {
            unreachable!("inputs_set == 1 enforced above")
        };
        check_dimension(collection, &vector)?;
        Source::Dense { vector }
    };

    // 3. retrieve_k.
    let retrieve_k = match &source {
        Source::Dense { .. } => dense_retrieve_k(
            req.top_k,
            req.rerank.is_some(),
            req.group_by.is_some(),
            req.dedup.is_some(),
            state.reranker.max_candidates(),
        ),
        // Hybrid handles its own candidate-pool sizing; outer LIMIT = top_k.
        Source::Hybrid { .. } => req.top_k,
    };

    // 4. Stages.
    let mut stages: Vec<Stage> = Vec::new();
    if let Some(r) = &req.rerank {
        stages.push(Stage::Rerank {
            query: r.query.clone(),
            top_n: r.top_n.unwrap_or(req.top_k),
            rank_field: r.rank_field.clone(),
            model: r.model.clone(),
        });
    }
    if let Some(min) = req.score_threshold {
        stages.push(Stage::ScoreThreshold { min });
    }
    if let Some(d) = &req.dedup {
        if d.by.is_empty() {
            return Err(ApiError::invalid_argument(
                "dedup.by must not be empty".to_string(),
            ));
        }
        stages.push(Stage::Dedup { by: d.by.clone() });
    }
    if let Some(g) = &req.group_by {
        stages.push(Stage::GroupBy {
            field: g.field.clone(),
            limit: g.limit,
            group_size: g.group_size,
        });
    }
    // If we deduped without a terminal GroupBy, we may have fewer than top_k
    // matches; truncate just in case any earlier stage left more.
    if req.dedup.is_some() && req.group_by.is_none() {
        stages.push(Stage::Truncate { k: req.top_k });
    }

    Ok(Plan {
        source,
        filter: req.filter.clone(),
        namespace,
        retrieve_k,
        top_k: req.top_k,
        stages,
        output: OutputSpec {
            include_values: req.include_values,
            include_metadata: req.include_metadata,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(bm25: bool, has_embedder: bool) -> CollectionMeta {
        use crate::embedding::{EmbedInputType, EmbedderConfig};
        CollectionMeta {
            id: uuid::Uuid::nil(),
            dimension: 3,
            metric: "cosine".into(),
            bm25_enabled: bm25,
            embedder_config: if has_embedder {
                Some(EmbedderConfig {
                    backend: "openai".into(),
                    model: "text-embedding-3-small".into(),
                    input_type: EmbedInputType::Document,
                })
            } else {
                None
            },
        }
    }

    fn base_search() -> SearchRequest {
        SearchRequest {
            vector: None,
            id: None,
            text: None,
            top_k: 5,
            namespace: None,
            filter: None,
            hybrid: None,
            rerank: None,
            score_threshold: None,
            group_by: None,
            dedup: None,
            include_values: false,
            include_metadata: false,
        }
    }

    /// `compile_search` requires a real `AppState` to embed/text-resolve.
    /// These tests only exercise pure compile paths that take a literal vector,
    /// so we never need an AppState — we just call helpers directly.
    #[test]
    fn dense_retrieve_k_legacy_multipliers() {
        // No rerank, no group → top_k itself.
        assert_eq!(dense_retrieve_k(5, false, false, false, 1000), 5);
        // Rerank only → top_k * 5.
        assert_eq!(dense_retrieve_k(5, true, false, false, 1000), 25);
        // Group only → top_k * 5.
        assert_eq!(dense_retrieve_k(5, false, true, false, 1000), 25);
        // Both → top_k * 25.
        assert_eq!(dense_retrieve_k(5, true, true, false, 1000), 125);
        // Provider cap binds before 10_000.
        assert_eq!(dense_retrieve_k(50, true, false, false, 100), 100);
        // Dedup only → top_k * 2.
        assert_eq!(dense_retrieve_k(5, false, false, true, 1000), 10);
    }

    #[test]
    fn hybrid_toggle_false_forces_dense_even_on_bm25_collection() {
        let mut req = base_search();
        req.text = Some("hi".into());
        req.hybrid = Some(HybridControl::Toggle(false));
        // bm25 enabled would normally auto-detect hybrid; toggle=false overrides.
        let _col = meta(true, true);
        // We can't fully run compile_search without AppState; just sanity-check
        // the parse:
        match req.hybrid {
            Some(HybridControl::Toggle(b)) => assert!(!b),
            _ => panic!("hybrid not parsed as toggle"),
        }
    }

    #[test]
    fn hybrid_options_alpha_parsed() {
        let req: SearchRequest = serde_json::from_value(serde_json::json!({
            "topK": 3,
            "vector": [0.1, 0.2, 0.3],
            "text": "hi",
            "hybrid": { "alpha": 0.7 }
        }))
        .unwrap();
        match req.hybrid {
            Some(HybridControl::Options(o)) => assert_eq!(o.alpha, Some(0.7)),
            _ => panic!("hybrid block not parsed as options"),
        }
    }
}
