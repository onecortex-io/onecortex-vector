//! HuggingFace Text Embeddings Inference (TEI) — self-hosted embeddings.
//!
//! Endpoint: POST {base_url}/embed
//! Auth:     none (deployer's responsibility)
//! Notes:    The model is baked into the TEI deployment; the `model` field on
//!           the EmbedderConfig is informational and used for cache keys.
//!           Self-hosted: no retry on 429 (single-tenant).

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use super::{EmbedInputType, Embedder, EmbedderError};

pub struct TeiEmbedder {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl TeiEmbedder {
    pub fn new(base_url: String, model: String, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
        }
    }
}

#[derive(Serialize)]
struct TeiRequest<'a> {
    inputs: &'a [String],
    truncate: bool,
}

/// TEI returns a bare JSON array: `[[...], [...]]`.
#[derive(Deserialize)]
#[serde(untagged)]
enum TeiResponse {
    Bare(Vec<Vec<f32>>),
}

#[async_trait::async_trait]
impl Embedder for TeiEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        _input_type: EmbedInputType,
    ) -> Result<Vec<Vec<f32>>, EmbedderError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let body = TeiRequest {
            inputs: texts,
            truncate: true,
        };

        let response = self
            .client
            .post(format!("{}/embed", self.base_url))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(EmbedderError::from_reqwest)?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(EmbedderError::http_status(
                status,
                format!("TEI error {status}: {text}"),
            ));
        }

        let parsed: TeiResponse = response
            .json()
            .await
            .map_err(|e| EmbedderError::Parse(e.to_string()))?;

        let TeiResponse::Bare(vectors) = parsed;
        if vectors.len() != texts.len() {
            return Err(EmbedderError::Parse(format!(
                "TEI returned {} embeddings for {} inputs",
                vectors.len(),
                texts.len()
            )));
        }

        debug!(
            embedder = "huggingface-tei",
            model = %self.model,
            inputs = texts.len()
        );
        Ok(vectors)
    }

    fn max_batch(&self) -> usize {
        32
    }

    fn backend(&self) -> &str {
        "huggingface-tei"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
