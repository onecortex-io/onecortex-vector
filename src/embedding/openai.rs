//! OpenAI embeddings.
//!
//! Endpoint: POST https://api.openai.com/v1/embeddings
//! Auth:     Authorization: Bearer <key>
//! Models:   text-embedding-3-small (1536), text-embedding-3-large (3072),
//!           text-embedding-ada-002 (1536)
//! Notes:    OpenAI does not distinguish query/document input types.
//!           Per-request batch limit: 2048 inputs.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use super::{EmbedInputType, Embedder, EmbedderError};
use crate::http_retry::send_with_retry;

pub struct OpenAiEmbedder {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_retries: u32,
}

impl OpenAiEmbedder {
    pub fn new(api_key: String, model: String, timeout_secs: u64, max_retries: u32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            api_key,
            model,
            max_retries,
        }
    }
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    input: &'a [String],
    model: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    data: Vec<OpenAiItem>,
}

#[derive(Deserialize)]
struct OpenAiItem {
    embedding: Vec<f32>,
    index: usize,
}

#[async_trait::async_trait]
impl Embedder for OpenAiEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        _input_type: EmbedInputType,
    ) -> Result<Vec<Vec<f32>>, EmbedderError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let api_key = self.api_key.clone();
        let body = OpenAiRequest {
            input: texts,
            model: &self.model,
        };

        let response = send_with_retry(
            || {
                self.client
                    .post("https://api.openai.com/v1/embeddings")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("Content-Type", "application/json")
                    .json(&body)
            },
            self.max_retries,
        )
        .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(EmbedderError::http_status(
                status,
                format!("OpenAI error {status}: {text}"),
            ));
        }

        let parsed: OpenAiResponse = response
            .json()
            .await
            .map_err(|e| EmbedderError::Parse(e.to_string()))?;

        let mut out = vec![Vec::new(); texts.len()];
        for item in parsed.data {
            if item.index >= out.len() {
                return Err(EmbedderError::Parse(format!(
                    "OpenAI returned index {} for batch of {}",
                    item.index,
                    texts.len()
                )));
            }
            out[item.index] = item.embedding;
        }
        if out.iter().any(|v| v.is_empty()) {
            return Err(EmbedderError::Parse(
                "OpenAI response missing embeddings for some inputs".into(),
            ));
        }

        debug!(
            embedder = "openai",
            model = %self.model,
            inputs = texts.len()
        );
        Ok(out)
    }

    fn max_batch(&self) -> usize {
        2048
    }

    fn backend(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
