//! Query Plan AST + executor.
//!
//! See `docs/analysis/03_ARCHITECTURE_CHANGES.md` §1 and the implementation
//! plan at `~/.claude/plans/amazing-let-s-go-ahead-parsed-comet.md`.
//!
//! Layout:
//! ```text
//! ast.rs       — types: Plan, Source, Stage, OutputSpec, ExecutionResult
//! compile.rs   — request → Plan (search/query/hybrid)
//! execute.rs   — Plan → DB rows → stages → ExecutionResult
//! sources/     — Dense (single-leg ANN) and Hybrid (RRF) source modules
//! stages/      — pure post-processing stages (rerank/threshold/dedup/groupBy/truncate)
//! ```

pub mod ast;
pub mod compile;
pub mod execute;
pub mod sources;
pub mod stages;

pub use ast::{ExecutionResult, OutputSpec, Plan, Source, Stage};
pub use compile::{compile_hybrid, compile_query, compile_search, SearchRequest};
pub use execute::execute;
