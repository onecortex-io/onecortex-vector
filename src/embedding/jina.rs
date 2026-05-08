//! Jina AI embeddings.
//!
//! Endpoint: POST https://api.jina.ai/v1/embeddings
//! Auth:     Authorization: Bearer <key>
//! Models:   jina-embeddings-v3, jina-embeddings-v2-base-en
//! Notes:    Jina v3 supports `task` ("retrieval.query" | "retrieval.passage").
//!           Per-request batch limit: 2048 inputs.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use super::{EmbedInputType, Embedder, EmbedderError};
use crate::http_retry::send_with_retry;

pub struct JinaEmbedder {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_retries: u32,
}

impl JinaEmbedder {
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
struct JinaRequest<'a> {
    input: &'a [String],
    model: &'a str,
    task: &'a str,
}

#[derive(Deserialize)]
struct JinaResponse {
    data: Vec<JinaItem>,
}

#[derive(Deserialize)]
struct JinaItem {
    embedding: Vec<f32>,
    index: usize,
}

#[async_trait::async_trait]
impl Embedder for JinaEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        input_type: EmbedInputType,
    ) -> Result<Vec<Vec<f32>>, EmbedderError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let task = match input_type {
            EmbedInputType::Document => "retrieval.passage",
            EmbedInputType::Query => "retrieval.query",
        };
        let api_key = self.api_key.clone();
        let body = JinaRequest {
            input: texts,
            model: &self.model,
            task,
        };

        let response = send_with_retry(
            || {
                self.client
                    .post("https://api.jina.ai/v1/embeddings")
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
                format!("Jina error {status}: {text}"),
            ));
        }

        let parsed: JinaResponse = response
            .json()
            .await
            .map_err(|e| EmbedderError::Parse(e.to_string()))?;

        let mut out = vec![Vec::new(); texts.len()];
        for item in parsed.data {
            if item.index >= out.len() {
                return Err(EmbedderError::Parse(format!(
                    "Jina returned index {} for batch of {}",
                    item.index,
                    texts.len()
                )));
            }
            out[item.index] = item.embedding;
        }
        if out.iter().any(|v| v.is_empty()) {
            return Err(EmbedderError::Parse(
                "Jina response missing embeddings for some inputs".into(),
            ));
        }

        debug!(
            embedder = "jina",
            model = %self.model,
            task,
            inputs = texts.len()
        );
        Ok(out)
    }

    fn max_batch(&self) -> usize {
        2048
    }

    fn backend(&self) -> &str {
        "jina"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
