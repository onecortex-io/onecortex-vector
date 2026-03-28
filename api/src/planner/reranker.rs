use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// A single candidate to be reranked.
#[derive(Debug, Clone)]
pub struct RerankCandidate {
    pub id: String,
    pub score: f32,
    /// Text to rank against. Extracted from metadata[rank_field] by the query handler.
    pub text: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub values: Option<Vec<f32>>,
}

/// A single reranked result.
#[derive(Debug, Clone)]
pub struct RerankResult {
    pub id: String,
    /// Score from the reranker (higher = more relevant).
    /// Cohere/Voyage/Jina/Pinecone: calibrated [0, 1]. TEI cross-encoder: raw logit.
    pub rerank_score: f32,
    pub metadata: Option<serde_json::Value>,
    pub values: Option<Vec<f32>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RerankerError {
    #[error("Reranker HTTP error: {0}")]
    Http(String),
    #[error("Reranker response parse error: {0}")]
    Parse(String),
    #[error("Reranker configuration error: {0}")]
    Config(String),
    #[error("Reranker upstream rate limited after {0} retries")]
    RateLimited(u32),
}

/// Reranker trait. All implementations must be Send + Sync (stored in Arc<dyn Reranker>).
#[async_trait::async_trait]
pub trait Reranker: Send + Sync {
    /// Re-scores `candidates` against the `query` text.
    /// Returns results sorted by rerank_score descending, truncated to `top_n`.
    /// `model_override`: if Some, use this model instead of the backend default.
    ///   Ignored by NoopReranker and CrossEncoderReranker (no model selection).
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<RerankCandidate>,
        top_n: usize,
        model_override: Option<&str>,
    ) -> Result<Vec<RerankResult>, RerankerError>;

    /// Maximum number of candidates this provider accepts per request.
    /// The query handler caps the ANN fetch size at this limit.
    /// Default: 1000 (matches Cohere/Voyage/Jina limits).
    fn max_candidates(&self) -> usize {
        1000
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared retry helper — exponential backoff for HTTP 429 (rate limit).
// ─────────────────────────────────────────────────────────────────────────────

/// Sends an HTTP request, retrying on 429 with exponential backoff.
/// `build_request`: closure that returns a fresh `RequestBuilder` each attempt
/// (required because `RequestBuilder` is not `Clone`).
async fn send_with_retry(
    build_request: impl Fn() -> reqwest::RequestBuilder,
    max_retries: u32,
) -> Result<reqwest::Response, RerankerError> {
    let mut delay_ms = 1_000u64;
    for attempt in 0..=max_retries {
        let response = build_request()
            .send()
            .await
            .map_err(|e| RerankerError::Http(e.to_string()))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if attempt == max_retries {
                return Err(RerankerError::RateLimited(max_retries));
            }
            warn!(
                attempt = attempt + 1,
                delay_ms,
                "Reranker rate limited (429); retrying"
            );
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms = (delay_ms * 2).min(30_000);
            continue;
        }
        return Ok(response);
    }
    // Unreachable: the loop always returns inside the body.
    Err(RerankerError::RateLimited(max_retries))
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. NoopReranker — passes candidates through unchanged.
// ─────────────────────────────────────────────────────────────────────────────

pub struct NoopReranker;

#[async_trait::async_trait]
impl Reranker for NoopReranker {
    async fn rerank(
        &self,
        _query: &str,
        candidates: Vec<RerankCandidate>,
        top_n: usize,
        _model_override: Option<&str>,
    ) -> Result<Vec<RerankResult>, RerankerError> {
        let results = candidates
            .into_iter()
            .take(top_n)
            .map(|c| RerankResult {
                id: c.id,
                rerank_score: c.score,
                metadata: c.metadata,
                values: c.values,
            })
            .collect();
        Ok(results)
    }

    fn max_candidates(&self) -> usize {
        usize::MAX
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. CohereReranker — Cohere Rerank v2 API.
//
//   Endpoint: POST https://api.cohere.com/v2/rerank
//   Auth:     Authorization: Bearer <key>
//   Models:   rerank-v3.5 (default, multilingual), rerank-v4.0-pro, rerank-english-v3.0
//   Limits:   1,000 documents/request; documents auto-truncated to max_tokens_per_doc (4096).
// ─────────────────────────────────────────────────────────────────────────────

pub struct CohereReranker {
    client: reqwest::Client,
    api_key: String,
    default_model: String,
    max_retries: u32,
}

impl CohereReranker {
    pub fn new(api_key: String, default_model: String, timeout_secs: u64, max_retries: u32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self { client, api_key, default_model, max_retries }
    }
}

#[derive(Serialize)]
struct CohereV2RerankRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<&'a str>,
    top_n: usize,
    return_documents: bool,
}

#[derive(Deserialize)]
struct CohereV2RerankResponse {
    results: Vec<CohereV2Result>,
}

#[derive(Deserialize)]
struct CohereV2Result {
    index: usize,
    relevance_score: f32,
}

#[async_trait::async_trait]
impl Reranker for CohereReranker {
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<RerankCandidate>,
        top_n: usize,
        model_override: Option<&str>,
    ) -> Result<Vec<RerankResult>, RerankerError> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }
        let model = model_override.unwrap_or(&self.default_model);
        let texts: Vec<String> = extract_texts(&candidates);
        let doc_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let effective_top_n = top_n.min(candidates.len());

        let api_key = self.api_key.clone();
        let body = CohereV2RerankRequest {
            model,
            query,
            documents: doc_refs,
            top_n: effective_top_n,
            return_documents: false,
        };

        let response = send_with_retry(
            || {
                self.client
                    .post("https://api.cohere.com/v2/rerank")
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
            },
            self.max_retries,
        )
        .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RerankerError::Http(format!("Cohere error {status}: {text}")));
        }

        let resp: CohereV2RerankResponse = response
            .json()
            .await
            .map_err(|e| RerankerError::Parse(e.to_string()))?;

        let mut results = map_indexed_results(resp.results.iter().map(|r| (r.index, r.relevance_score)), &candidates);
        results.sort_by(|a, b| b.rerank_score.partial_cmp(&a.rerank_score).unwrap_or(std::cmp::Ordering::Equal));

        debug!(reranker = "cohere", model, candidates = candidates.len(), returned = results.len());
        Ok(results)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. VoyageReranker — Voyage AI Rerank API.
//
//   Endpoint: POST https://api.voyageai.com/v1/rerank
//   Auth:     Authorization: Bearer <key>
//   Models:   rerank-2.5 (default), rerank-2.5-lite, rerank-2, rerank-lite-1
//   Limits:   1,000 documents/request; 600K total tokens for rerank-2.5.
//   Note:     Uses "top_k" (not "top_n") for the output limit parameter.
//             Set truncation=true to avoid hard errors on long documents.
// ─────────────────────────────────────────────────────────────────────────────

pub struct VoyageReranker {
    client: reqwest::Client,
    api_key: String,
    default_model: String,
    max_retries: u32,
}

impl VoyageReranker {
    pub fn new(api_key: String, default_model: String, timeout_secs: u64, max_retries: u32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self { client, api_key, default_model, max_retries }
    }
}

#[derive(Serialize)]
struct VoyageRerankRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<&'a str>,
    /// Voyage uses "top_k" (not "top_n") — maps to the same concept.
    top_k: usize,
    truncation: bool,
    return_documents: bool,
}

#[derive(Deserialize)]
struct VoyageRerankResponse {
    data: Vec<VoyageResult>,
}

#[derive(Deserialize)]
struct VoyageResult {
    index: usize,
    relevance_score: f32,
}

#[async_trait::async_trait]
impl Reranker for VoyageReranker {
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<RerankCandidate>,
        top_n: usize,
        model_override: Option<&str>,
    ) -> Result<Vec<RerankResult>, RerankerError> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }
        let model = model_override.unwrap_or(&self.default_model);
        let texts: Vec<String> = extract_texts(&candidates);
        let doc_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let effective_top_k = top_n.min(candidates.len());

        let api_key = self.api_key.clone();
        let body = VoyageRerankRequest {
            model,
            query,
            documents: doc_refs,
            top_k: effective_top_k, // Voyage's parameter name differs from Cohere/Jina
            truncation: true,        // Prevent hard errors on long documents
            return_documents: false,
        };

        let response = send_with_retry(
            || {
                self.client
                    .post("https://api.voyageai.com/v1/rerank")
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
            },
            self.max_retries,
        )
        .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RerankerError::Http(format!("Voyage error {status}: {text}")));
        }

        let resp: VoyageRerankResponse = response
            .json()
            .await
            .map_err(|e| RerankerError::Parse(e.to_string()))?;

        let mut results = map_indexed_results(resp.data.iter().map(|r| (r.index, r.relevance_score)), &candidates);
        results.sort_by(|a, b| b.rerank_score.partial_cmp(&a.rerank_score).unwrap_or(std::cmp::Ordering::Equal));

        debug!(reranker = "voyage", model, candidates = candidates.len(), returned = results.len());
        Ok(results)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. JinaReranker — Jina AI Rerank API.
//
//   Endpoint: POST https://api.jina.ai/v1/rerank
//   Auth:     Authorization: Bearer <key>
//   Models:   jina-reranker-v2-base-multilingual (default), jina-reranker-v1-base-en
//   Schema:   Drop-in replacement for Cohere's format (same request/response shape).
//   Strengths: Cost-effective; excellent for long documents in RAG pipelines.
// ─────────────────────────────────────────────────────────────────────────────

pub struct JinaReranker {
    client: reqwest::Client,
    api_key: String,
    default_model: String,
    max_retries: u32,
}

impl JinaReranker {
    pub fn new(api_key: String, default_model: String, timeout_secs: u64, max_retries: u32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self { client, api_key, default_model, max_retries }
    }
}

// Jina uses the same request/response schema as Cohere v2.
#[derive(Serialize)]
struct JinaRerankRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<&'a str>,
    top_n: usize,
    return_documents: bool,
}

#[derive(Deserialize)]
struct JinaRerankResponse {
    results: Vec<JinaResult>,
}

#[derive(Deserialize)]
struct JinaResult {
    index: usize,
    relevance_score: f32,
}

#[async_trait::async_trait]
impl Reranker for JinaReranker {
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<RerankCandidate>,
        top_n: usize,
        model_override: Option<&str>,
    ) -> Result<Vec<RerankResult>, RerankerError> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }
        let model = model_override.unwrap_or(&self.default_model);
        let texts: Vec<String> = extract_texts(&candidates);
        let doc_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let effective_top_n = top_n.min(candidates.len());

        let api_key = self.api_key.clone();
        let body = JinaRerankRequest {
            model,
            query,
            documents: doc_refs,
            top_n: effective_top_n,
            return_documents: false,
        };

        let response = send_with_retry(
            || {
                self.client
                    .post("https://api.jina.ai/v1/rerank")
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json")
                    .json(&body)
            },
            self.max_retries,
        )
        .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RerankerError::Http(format!("Jina error {status}: {text}")));
        }

        let resp: JinaRerankResponse = response
            .json()
            .await
            .map_err(|e| RerankerError::Parse(e.to_string()))?;

        let mut results = map_indexed_results(resp.results.iter().map(|r| (r.index, r.relevance_score)), &candidates);
        results.sort_by(|a, b| b.rerank_score.partial_cmp(&a.rerank_score).unwrap_or(std::cmp::Ordering::Equal));

        debug!(reranker = "jina", model, candidates = candidates.len(), returned = results.len());
        Ok(results)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. PineconeReranker — Pinecone Inference Rerank API.
//
//   Endpoint: POST https://api.pinecone.io/rerank
//   Auth:     Api-Key: <key>  (NOT Bearer — different header name)
//   Models:   pinecone-rerank-v0 (proprietary, default)
//             bge-reranker-v2-m3 (hosted by Pinecone)
//             cohere-rerank-3.5 (hosted by Pinecone)
//   Limits:   HARD LIMIT of 100 documents per request. Enforced via max_candidates().
//             Query: max 256 tokens. Document: max 1024 tokens.
//   Note:     Uses "Api-Key" header (not "Authorization: Bearer").
//             truncation via parameters.truncate field.
// ─────────────────────────────────────────────────────────────────────────────

pub struct PineconeReranker {
    client: reqwest::Client,
    api_key: String,
    default_model: String,
    max_retries: u32,
}

impl PineconeReranker {
    pub fn new(api_key: String, default_model: String, timeout_secs: u64, max_retries: u32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self { client, api_key, default_model, max_retries }
    }
}

#[derive(Serialize)]
struct PineconeRerankRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<&'a str>,
    top_n: usize,
    return_documents: bool,
    parameters: PineconeRerankParams,
}

#[derive(Serialize)]
struct PineconeRerankParams {
    truncate: &'static str,   // "END" | "NONE"
}

#[derive(Deserialize)]
struct PineconeRerankResponse {
    data: Vec<PineconeResult>,
}

#[derive(Deserialize)]
struct PineconeResult {
    index: usize,
    score: f32,  // Pinecone uses "score" (not "relevance_score")
}

#[async_trait::async_trait]
impl Reranker for PineconeReranker {
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<RerankCandidate>,
        top_n: usize,
        model_override: Option<&str>,
    ) -> Result<Vec<RerankResult>, RerankerError> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }
        let model = model_override.unwrap_or(&self.default_model);
        let texts: Vec<String> = extract_texts(&candidates);
        let doc_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        // Pinecone hard limit: 100 docs. max_candidates() enforces this upstream,
        // but guard here as a safety net.
        if doc_refs.len() > 100 {
            return Err(RerankerError::Config(
                "Pinecone reranker: max 100 documents per request".to_string()
            ));
        }
        let effective_top_n = top_n.min(candidates.len());

        let api_key = self.api_key.clone();
        let body = PineconeRerankRequest {
            model,
            query,
            documents: doc_refs,
            top_n: effective_top_n,
            return_documents: false,
            parameters: PineconeRerankParams { truncate: "END" },
        };

        let response = send_with_retry(
            || {
                self.client
                    .post("https://api.pinecone.io/rerank")
                    .header("Api-Key", &api_key)  // Note: "Api-Key" not "Authorization: Bearer"
                    .header("Content-Type", "application/json")
                    .json(&body)
            },
            self.max_retries,
        )
        .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RerankerError::Http(format!("Pinecone error {status}: {text}")));
        }

        let resp: PineconeRerankResponse = response
            .json()
            .await
            .map_err(|e| RerankerError::Parse(e.to_string()))?;

        let mut results = map_indexed_results(resp.data.iter().map(|r| (r.index, r.score)), &candidates);
        results.sort_by(|a, b| b.rerank_score.partial_cmp(&a.rerank_score).unwrap_or(std::cmp::Ordering::Equal));

        debug!(reranker = "pinecone", model, candidates = candidates.len(), returned = results.len());
        Ok(results)
    }

    /// Pinecone imposes a hard 100-document limit per request.
    fn max_candidates(&self) -> usize {
        100
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. CrossEncoderReranker — Self-hosted HuggingFace TEI server.
//
//   Compatible with HuggingFace Text Embeddings Inference (TEI) /rerank endpoint.
//   Default model: BAAI/bge-reranker-v2-m3 (multilingual, 8K token context, strong recall).
//   See deploy/cross-encoder/README.md for deployment notes.
//
//   Request:  POST <url>/rerank   { "query": "...", "texts": ["..."], "truncate": true }
//   Response: [{ "index": 0, "score": 0.98 }, ...]
//   Note:     model_override is ignored — model is baked into the server deployment.
// ─────────────────────────────────────────────────────────────────────────────

pub struct CrossEncoderReranker {
    client: reqwest::Client,
    base_url: String,
}

impl CrossEncoderReranker {
    pub fn new(base_url: String, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

#[derive(Serialize)]
struct TeiRerankRequest<'a> {
    query: &'a str,
    texts: Vec<&'a str>,
    truncate: bool,
}

#[derive(Deserialize)]
struct TeiRerankResult {
    index: usize,
    score: f32,
}

#[async_trait::async_trait]
impl Reranker for CrossEncoderReranker {
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<RerankCandidate>,
        top_n: usize,
        _model_override: Option<&str>,  // Ignored — model is fixed in the TEI deployment
    ) -> Result<Vec<RerankResult>, RerankerError> {
        if candidates.is_empty() {
            return Ok(vec![]);
        }
        let texts: Vec<String> = extract_texts(&candidates);
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

        let body = TeiRerankRequest {
            query,
            texts: text_refs,
            truncate: true,
        };

        let response = self
            .client
            .post(format!("{}/rerank", self.base_url))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| RerankerError::Http(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(RerankerError::Http(format!("Cross-encoder error {status}: {text}")));
        }

        let tei_results: Vec<TeiRerankResult> = response
            .json()
            .await
            .map_err(|e| RerankerError::Parse(e.to_string()))?;

        let mut results = map_indexed_results(tei_results.iter().map(|r| (r.index, r.score)), &candidates);
        results.sort_by(|a, b| b.rerank_score.partial_cmp(&a.rerank_score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_n);

        debug!(reranker = "cross-encoder", candidates = candidates.len(), returned = results.len());
        Ok(results)
    }

    fn max_candidates(&self) -> usize {
        usize::MAX  // Self-hosted; no external doc limit.
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extracts the text to rank from a candidate.
/// Priority: candidate.text → metadata["text"] field (or configured rank_field) → candidate id.
fn extract_texts(candidates: &[RerankCandidate]) -> Vec<String> {
    candidates
        .iter()
        .map(|c| {
            c.text.clone().unwrap_or_else(|| {
                c.metadata
                    .as_ref()
                    .and_then(|m| m.get("text"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| c.id.clone())
            })
        })
        .collect()
}

/// Converts provider-returned (index, score) pairs into `RerankResult` by
/// looking up the original candidate at the returned index.
fn map_indexed_results(
    pairs: impl Iterator<Item = (usize, f32)>,
    candidates: &[RerankCandidate],
) -> Vec<RerankResult> {
    pairs
        .filter_map(|(idx, score)| {
            candidates.get(idx).map(|c| RerankResult {
                id: c.id.clone(),
                rerank_score: score,
                metadata: c.metadata.clone(),
                values: c.values.clone(),
            })
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Factory: build the right reranker from AppConfig.
// ─────────────────────────────────────────────────────────────────────────────

use crate::config::AppConfig;

pub fn build_reranker(config: &AppConfig) -> Arc<dyn Reranker> {
    let timeout = config.rerank_http_timeout_secs;
    let retries = config.rerank_max_retries;

    match config.rerank_backend.as_str() {
        "cohere" => {
            let key = config
                .rerank_cohere_api_key
                .clone()
                .expect("ONECORTEX_VECTOR_RERANK_COHERE_API_KEY must be set when rerank_backend=cohere");
            Arc::new(CohereReranker::new(key, config.rerank_cohere_model.clone(), timeout, retries))
        }
        "voyage" => {
            let key = config
                .rerank_voyage_api_key
                .clone()
                .expect("ONECORTEX_VECTOR_RERANK_VOYAGE_API_KEY must be set when rerank_backend=voyage");
            Arc::new(VoyageReranker::new(key, config.rerank_voyage_model.clone(), timeout, retries))
        }
        "jina" => {
            let key = config
                .rerank_jina_api_key
                .clone()
                .expect("ONECORTEX_VECTOR_RERANK_JINA_API_KEY must be set when rerank_backend=jina");
            Arc::new(JinaReranker::new(key, config.rerank_jina_model.clone(), timeout, retries))
        }
        "pinecone" => {
            let key = config
                .rerank_pinecone_api_key
                .clone()
                .expect("ONECORTEX_VECTOR_RERANK_PINECONE_API_KEY must be set when rerank_backend=pinecone");
            Arc::new(PineconeReranker::new(key, config.rerank_pinecone_model.clone(), timeout, retries))
        }
        "cross-encoder" => {
            let url = config
                .rerank_cross_encoder_url
                .clone()
                .expect("ONECORTEX_VECTOR_RERANK_CROSS_ENCODER_URL must be set when rerank_backend=cross-encoder");
            Arc::new(CrossEncoderReranker::new(url, timeout))
        }
        "none" | "" => Arc::new(NoopReranker),
        other => {
            warn!(
                backend = other,
                "Unknown ONECORTEX_VECTOR_RERANK_BACKEND value; falling back to NoopReranker"
            );
            Arc::new(NoopReranker)
        }
    }
}
