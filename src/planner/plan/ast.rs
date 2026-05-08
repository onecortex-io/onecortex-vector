//! Query Plan AST.
//!
//! Every public query endpoint (`/search`, `/query`, `/query/hybrid`) compiles
//! its request into a [`Plan`] and runs it through the single executor in
//! [`super::execute`]. New post-processing stages plug in here as variants of
//! [`Stage`] and become available to all three endpoints at once.
//!
//! See `docs/analysis/03_ARCHITECTURE_CHANGES.md` §1 for the design rationale.

use serde::Serialize;

pub use crate::handlers::query::{GroupResult, Match};

/// Where the candidate pool comes from.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Source {
    /// Single-leg dense ANN over `vector`.
    Dense { vector: Vec<f32> },
    /// Dense ANN + BM25 fused via Reciprocal Rank Fusion.
    Hybrid {
        vector: Vec<f32>,
        text: String,
        alpha: f32,
        bm25_weight: f32,
    },
}

/// A post-processing stage. Stages are pure transformations over a candidate
/// list (`Rerank` is async; the rest are sync). [`Stage::GroupBy`] is terminal —
/// once it runs, the executor returns a [`ExecutionResult::Grouped`] and any
/// later stages are skipped.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Stage {
    /// External reranker (Cohere/Voyage/Jina/Pinecone/cross-encoder/noop).
    Rerank {
        query: String,
        #[serde(rename = "topN")]
        top_n: i64,
        rank_field: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// Drop matches whose score is below `min`.
    ScoreThreshold { min: f64 },
    /// First-occurrence-wins dedupe by a metadata field.
    Dedup { by: String },
    /// Bucket matches by a metadata field. Terminal.
    GroupBy {
        field: String,
        limit: usize,
        group_size: usize,
    },
    /// Keep only the first `k` matches.
    Truncate { k: i64 },
}

/// What the response should include.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputSpec {
    pub include_values: bool,
    pub include_metadata: bool,
}

/// A compiled query plan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub source: Source,
    /// Original (untranslated) JSON filter; the executor passes it through
    /// `crate::planner::filter_translator::translate_filter`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<serde_json::Value>,
    pub namespace: String,
    /// Candidate-pool size pulled from the Source (before stages run).
    pub retrieve_k: i64,
    /// User-requested final size; honoured by `Truncate` and by `Rerank.topN`.
    pub top_k: i64,
    pub stages: Vec<Stage>,
    pub output: OutputSpec,
}

/// Executor output — flat or grouped.
pub enum ExecutionResult {
    Flat { matches: Vec<Match> },
    Grouped { groups: Vec<GroupResult> },
}

impl Plan {
    /// True if any stage requires metadata to be materialised from the DB.
    /// The Source SQL uses this to decide whether to SELECT `metadata`.
    pub fn stages_need_metadata(&self) -> bool {
        self.stages.iter().any(|s| {
            matches!(
                s,
                Stage::Rerank { .. } | Stage::Dedup { .. } | Stage::GroupBy { .. }
            )
        })
    }
}
