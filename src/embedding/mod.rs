//! Server-side embeddings (F1).
//!
//! A collection may bind a default `Embedder` at creation time. When bound,
//! callers may send `text` instead of `values` on upsert and query. The server
//! resolves the embedder per-request via [`EmbedderFactory`] and embeds the
//! input before falling through to the existing pgvector path.
//!
//! - HTTP retry / backoff is shared with the reranker via [`crate::http_retry`].
//! - Query-side embeddings are cached in [`cache::QueryEmbedCache`] (LRU + TTL).
//! - Upsert-side embeddings are never cached (content is one-shot).

use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub mod cache;
mod cohere;
mod jina;
mod openai;
mod tei;
mod voyage;

pub use cache::QueryEmbedCache;
pub use cohere::CohereEmbedder;
pub use jina::JinaEmbedder;
pub use openai::OpenAiEmbedder;
pub use tei::TeiEmbedder;
pub use voyage::VoyageEmbedder;

use crate::http_retry::{HttpKind, HttpRetryError};

// ── Public types ───────────────────────────────────────────────────────────

/// Whether the input is a corpus document (being indexed) or a search query.
/// Some providers (Voyage, Cohere) take this as an explicit parameter and
/// return slightly different embeddings depending on the value.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EmbedInputType {
    #[default]
    Document,
    Query,
}

/// Embedder configuration persisted in `_onecortex_vector.collections.embedder_config`.
/// API keys are NOT stored here — they live in env vars on the server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EmbedderConfig {
    pub backend: String,
    pub model: String,
    /// Default input type used at upsert time. Queries always override to `Query`.
    /// Optional in the request DTO; defaults to `Document`.
    #[serde(default)]
    pub input_type: EmbedInputType,
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedderError {
    #[error("Embedder HTTP error ({kind:?}): {message}")]
    Http { kind: HttpKind, message: String },
    #[error("Embedder response parse error: {0}")]
    Parse(String),
    #[error("Embedder configuration error: {0}")]
    Config(String),
    #[error("Embedder upstream rate limited after {0} retries")]
    RateLimited(u32),
    #[error("Embedder returned dimension {got}; collection requires {expected}")]
    DimensionMismatch { expected: usize, got: usize },
}

impl EmbedderError {
    pub fn from_reqwest(e: reqwest::Error) -> Self {
        EmbedderError::Http {
            kind: crate::http_retry::classify(&e),
            message: e.to_string(),
        }
    }

    pub fn http_status(status: reqwest::StatusCode, message: impl Into<String>) -> Self {
        EmbedderError::Http {
            kind: HttpKind::Status(status.as_u16()),
            message: message.into(),
        }
    }
}

impl From<HttpRetryError> for EmbedderError {
    fn from(err: HttpRetryError) -> Self {
        match err {
            HttpRetryError::Transport { kind, message } => EmbedderError::Http { kind, message },
            HttpRetryError::RateLimited(n) => EmbedderError::RateLimited(n),
        }
    }
}

/// Embedder trait. All implementations must be Send + Sync (stored in Arc<dyn Embedder>).
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    /// Embed a batch of texts. The returned `Vec<Vec<f32>>` preserves input order.
    /// Callers are responsible for splitting batches larger than [`Embedder::max_batch`].
    async fn embed(
        &self,
        texts: &[String],
        input_type: EmbedInputType,
    ) -> Result<Vec<Vec<f32>>, EmbedderError>;

    /// Maximum batch size accepted by the upstream provider in a single request.
    /// Default: 96 (a conservative value most providers accept).
    fn max_batch(&self) -> usize {
        96
    }

    /// Backend identifier (e.g. "openai") for cache keys and logging.
    fn backend(&self) -> &str;

    /// Active model name for cache keys and logging.
    fn model(&self) -> &str;
}

// ── Factory ────────────────────────────────────────────────────────────────

/// Lazy factory: builds an `Arc<dyn Embedder>` for a given (backend, model)
/// pair on first use, then memoizes it. Avoids paying client-build cost on
/// every request. Constructed once at startup with the loaded API keys.
pub struct EmbedderFactory {
    cfg: EmbedderFactoryConfig,
    cache: dashmap::DashMap<(String, String), Arc<dyn Embedder>>,
}

/// API keys + HTTP knobs needed to build any embedder backend.
/// Mirrors the reranker config layout in [`crate::config::AppConfig`].
#[derive(Debug, Clone, Default)]
pub struct EmbedderFactoryConfig {
    pub openai_api_key: Option<String>,
    pub voyage_api_key: Option<String>,
    pub cohere_api_key: Option<String>,
    pub jina_api_key: Option<String>,
    pub tei_url: Option<String>,
    pub http_timeout_secs: u64,
    pub max_retries: u32,
}

impl EmbedderFactory {
    pub fn new(cfg: EmbedderFactoryConfig) -> Self {
        Self {
            cfg,
            cache: dashmap::DashMap::new(),
        }
    }

    /// Resolve (or build) an embedder for the collection's bound config.
    pub fn for_config(&self, ec: &EmbedderConfig) -> Result<Arc<dyn Embedder>, EmbedderError> {
        let key = (ec.backend.clone(), ec.model.clone());
        if let Some(existing) = self.cache.get(&key) {
            return Ok(existing.clone());
        }
        let built: Arc<dyn Embedder> = self.build(ec)?;
        self.cache.insert(key, built.clone());
        Ok(built)
    }

    fn build(&self, ec: &EmbedderConfig) -> Result<Arc<dyn Embedder>, EmbedderError> {
        let timeout = self.cfg.http_timeout_secs;
        let retries = self.cfg.max_retries;
        let model = ec.model.clone();
        match ec.backend.as_str() {
            "openai" => {
                let key = self.cfg.openai_api_key.clone().ok_or_else(|| {
                    EmbedderError::Config(
                        "ONECORTEX_VECTOR_EMBED_OPENAI_API_KEY is not set; \
                         cannot use embedder backend 'openai'"
                            .into(),
                    )
                })?;
                Ok(Arc::new(OpenAiEmbedder::new(key, model, timeout, retries)))
            }
            "voyage" => {
                let key = self.cfg.voyage_api_key.clone().ok_or_else(|| {
                    EmbedderError::Config(
                        "ONECORTEX_VECTOR_EMBED_VOYAGE_API_KEY is not set; \
                         cannot use embedder backend 'voyage'"
                            .into(),
                    )
                })?;
                Ok(Arc::new(VoyageEmbedder::new(key, model, timeout, retries)))
            }
            "cohere" => {
                let key = self.cfg.cohere_api_key.clone().ok_or_else(|| {
                    EmbedderError::Config(
                        "ONECORTEX_VECTOR_EMBED_COHERE_API_KEY is not set; \
                         cannot use embedder backend 'cohere'"
                            .into(),
                    )
                })?;
                Ok(Arc::new(CohereEmbedder::new(key, model, timeout, retries)))
            }
            "jina" => {
                let key = self.cfg.jina_api_key.clone().ok_or_else(|| {
                    EmbedderError::Config(
                        "ONECORTEX_VECTOR_EMBED_JINA_API_KEY is not set; \
                         cannot use embedder backend 'jina'"
                            .into(),
                    )
                })?;
                Ok(Arc::new(JinaEmbedder::new(key, model, timeout, retries)))
            }
            "huggingface-tei" | "tei" => {
                let url = self.cfg.tei_url.clone().ok_or_else(|| {
                    EmbedderError::Config(
                        "ONECORTEX_VECTOR_EMBED_TEI_URL is not set; \
                         cannot use embedder backend 'huggingface-tei'"
                            .into(),
                    )
                })?;
                Ok(Arc::new(TeiEmbedder::new(url, model, timeout)))
            }
            other => Err(EmbedderError::Config(format!(
                "unknown embedder backend '{other}'; \
                 expected one of: openai, voyage, cohere, jina, huggingface-tei"
            ))),
        }
    }
}

/// Embed a list of texts in chunks of `embedder.max_batch()`, preserving input order.
pub async fn embed_in_batches(
    embedder: &dyn Embedder,
    texts: &[String],
    input_type: EmbedInputType,
) -> Result<Vec<Vec<f32>>, EmbedderError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let batch_size = embedder.max_batch().max(1);
    let mut out = Vec::with_capacity(texts.len());
    for chunk in texts.chunks(batch_size) {
        let mut part = embedder.embed(chunk, input_type).await?;
        out.append(&mut part);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test-only embedder: returns `[idx, batch_index, len, 0...]` for each input
    /// so we can assert order preservation across batches and how many upstream
    /// batch calls happened.
    struct CountingEmbedder {
        batch_size: usize,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Embedder for CountingEmbedder {
        async fn embed(
            &self,
            texts: &[String],
            _input_type: EmbedInputType,
        ) -> Result<Vec<Vec<f32>>, EmbedderError> {
            let batch_idx = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(texts
                .iter()
                .enumerate()
                .map(|(i, t)| vec![i as f32, batch_idx as f32, t.len() as f32])
                .collect())
        }
        fn max_batch(&self) -> usize {
            self.batch_size
        }
        fn backend(&self) -> &str {
            "test"
        }
        fn model(&self) -> &str {
            "stub"
        }
    }

    #[tokio::test]
    async fn embed_in_batches_chunks_and_preserves_order() {
        let e = CountingEmbedder {
            batch_size: 2,
            calls: AtomicUsize::new(0),
        };
        let inputs: Vec<String> = (0..5).map(|i| format!("t{i}")).collect();
        let out = embed_in_batches(&e, &inputs, EmbedInputType::Document)
            .await
            .unwrap();
        assert_eq!(out.len(), 5);
        // 5 inputs / batch_size 2 → 3 batches (2,2,1)
        assert_eq!(e.calls.load(Ordering::SeqCst), 3);
        // Order preserved: batch_idx slot (index 1) goes 0,0,1,1,2.
        let batch_idxs: Vec<f32> = out.iter().map(|v| v[1]).collect();
        assert_eq!(batch_idxs, vec![0.0, 0.0, 1.0, 1.0, 2.0]);
    }

    #[tokio::test]
    async fn embed_in_batches_empty_input_skips_upstream() {
        let e = CountingEmbedder {
            batch_size: 2,
            calls: AtomicUsize::new(0),
        };
        let out = embed_in_batches(&e, &[], EmbedInputType::Document)
            .await
            .unwrap();
        assert!(out.is_empty());
        assert_eq!(e.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn factory_rejects_missing_api_key_with_clear_message() {
        let factory = EmbedderFactory::new(EmbedderFactoryConfig::default());
        let err = factory
            .for_config(&EmbedderConfig {
                backend: "openai".into(),
                model: "text-embedding-3-small".into(),
                input_type: EmbedInputType::Document,
            })
            .err()
            .expect("expected Config error when key is missing");
        match err {
            EmbedderError::Config(msg) => {
                assert!(msg.contains("OPENAI_API_KEY"), "message was: {msg}");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn factory_rejects_unknown_backend() {
        let factory = EmbedderFactory::new(EmbedderFactoryConfig::default());
        let err = factory
            .for_config(&EmbedderConfig {
                backend: "totally-fake".into(),
                model: "nope".into(),
                input_type: EmbedInputType::Query,
            })
            .err()
            .unwrap();
        match err {
            EmbedderError::Config(msg) => {
                assert!(msg.contains("unknown embedder backend"), "{msg}");
                assert!(msg.contains("totally-fake"), "{msg}");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn embedder_config_serde_round_trip_camelcase() {
        let cfg = EmbedderConfig {
            backend: "openai".into(),
            model: "text-embedding-3-small".into(),
            input_type: EmbedInputType::Query,
        };
        let v = serde_json::to_value(&cfg).unwrap();
        // camelCase on the wire (CLAUDE.md rule)
        assert_eq!(v["backend"], "openai");
        assert_eq!(v["inputType"], "query");
        let back: EmbedderConfig = serde_json::from_value(v).unwrap();
        assert_eq!(back, cfg);
    }
}
