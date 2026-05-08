//! Voyage AI embeddings.
//!
//! Endpoint: POST https://api.voyageai.com/v1/embeddings
//! Auth:     Authorization: Bearer <key>
//! Models:   voyage-3, voyage-3-lite, voyage-code-2
//! Notes:    Voyage distinguishes query vs document via `input_type`
//!           ("query" | "document"). Batch limit: 128 inputs.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use super::{EmbedInputType, Embedder, EmbedderError};
use crate::http_retry::send_with_retry;

pub struct VoyageEmbedder {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_retries: u32,
}

impl VoyageEmbedder {
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
struct VoyageRequest<'a> {
    input: &'a [String],
    model: &'a str,
    input_type: &'a str,
}

#[derive(Deserialize)]
struct VoyageResponse {
    data: Vec<VoyageItem>,
}

#[derive(Deserialize)]
struct VoyageItem {
    embedding: Vec<f32>,
    index: usize,
}

#[async_trait::async_trait]
impl Embedder for VoyageEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        input_type: EmbedInputType,
    ) -> Result<Vec<Vec<f32>>, EmbedderError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let it = match input_type {
            EmbedInputType::Document => "document",
            EmbedInputType::Query => "query",
        };
        let api_key = self.api_key.clone();
        let body = VoyageRequest {
            input: texts,
            model: &self.model,
            input_type: it,
        };

        let response = send_with_retry(
            || {
                self.client
                    .post("https://api.voyageai.com/v1/embeddings")
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
                format!("Voyage error {status}: {text}"),
            ));
        }

        let parsed: VoyageResponse = response
            .json()
            .await
            .map_err(|e| EmbedderError::Parse(e.to_string()))?;

        let mut out = vec![Vec::new(); texts.len()];
        for item in parsed.data {
            if item.index >= out.len() {
                return Err(EmbedderError::Parse(format!(
                    "Voyage returned index {} for batch of {}",
                    item.index,
                    texts.len()
                )));
            }
            out[item.index] = item.embedding;
        }
        if out.iter().any(|v| v.is_empty()) {
            return Err(EmbedderError::Parse(
                "Voyage response missing embeddings for some inputs".into(),
            ));
        }

        debug!(
            embedder = "voyage",
            model = %self.model,
            input_type = it,
            inputs = texts.len()
        );
        Ok(out)
    }

    fn max_batch(&self) -> usize {
        128
    }

    fn backend(&self) -> &str {
        "voyage"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
