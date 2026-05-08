//! Cohere embeddings.
//!
//! Endpoint: POST https://api.cohere.com/v2/embed
//! Auth:     Authorization: Bearer <key>
//! Models:   embed-v4.0 (multimodal), embed-multilingual-v3.0, embed-english-v3.0
//! Notes:    Cohere requires `input_type` ("search_query" | "search_document" | …).
//!           Batch limit: 96 inputs. Embeddings are nested under `embeddings.float`.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use super::{EmbedInputType, Embedder, EmbedderError};
use crate::http_retry::send_with_retry;

pub struct CohereEmbedder {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_retries: u32,
}

impl CohereEmbedder {
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
struct CohereRequest<'a> {
    texts: &'a [String],
    model: &'a str,
    input_type: &'a str,
    embedding_types: [&'a str; 1],
}

#[derive(Deserialize)]
struct CohereResponse {
    embeddings: CohereEmbeddings,
}

#[derive(Deserialize)]
struct CohereEmbeddings {
    float: Vec<Vec<f32>>,
}

#[async_trait::async_trait]
impl Embedder for CohereEmbedder {
    async fn embed(
        &self,
        texts: &[String],
        input_type: EmbedInputType,
    ) -> Result<Vec<Vec<f32>>, EmbedderError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let it = match input_type {
            EmbedInputType::Document => "search_document",
            EmbedInputType::Query => "search_query",
        };
        let api_key = self.api_key.clone();
        let body = CohereRequest {
            texts,
            model: &self.model,
            input_type: it,
            embedding_types: ["float"],
        };

        let response = send_with_retry(
            || {
                self.client
                    .post("https://api.cohere.com/v2/embed")
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
                format!("Cohere error {status}: {text}"),
            ));
        }

        let parsed: CohereResponse = response
            .json()
            .await
            .map_err(|e| EmbedderError::Parse(e.to_string()))?;

        if parsed.embeddings.float.len() != texts.len() {
            return Err(EmbedderError::Parse(format!(
                "Cohere returned {} embeddings for {} inputs",
                parsed.embeddings.float.len(),
                texts.len()
            )));
        }

        debug!(
            embedder = "cohere",
            model = %self.model,
            input_type = it,
            inputs = texts.len()
        );
        Ok(parsed.embeddings.float)
    }

    fn max_batch(&self) -> usize {
        96
    }

    fn backend(&self) -> &str {
        "cohere"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
