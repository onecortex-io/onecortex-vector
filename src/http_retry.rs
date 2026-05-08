use std::time::Duration;
use tracing::warn;

/// Classification of an upstream HTTP failure, used to pick the right
/// HTTP status when bubbling out to the API edge.
#[derive(Debug, Clone)]
pub enum HttpKind {
    /// Request timed out (we never heard back from the upstream).
    Timeout,
    /// TCP/TLS connect failure or DNS error before the request was sent.
    Connect,
    /// We got a response, but the upstream returned a non-2xx status.
    Status(u16),
    /// Anything else — body decode error, redirect loop, etc.
    Other,
}

/// Classify a reqwest transport error into an `HttpKind`.
pub fn classify(e: &reqwest::Error) -> HttpKind {
    if e.is_timeout() {
        HttpKind::Timeout
    } else if e.is_connect() {
        HttpKind::Connect
    } else if let Some(status) = e.status() {
        HttpKind::Status(status.as_u16())
    } else {
        HttpKind::Other
    }
}

/// Shared retry-error type for outbound HTTP calls (rerankers + embedders).
/// Each subsystem maps this to its own concrete error enum via `From`.
#[derive(Debug, thiserror::Error)]
pub enum HttpRetryError {
    #[error("HTTP error ({kind:?}): {message}")]
    Transport { kind: HttpKind, message: String },
    #[error("Upstream rate limited after {0} retries")]
    RateLimited(u32),
}

impl HttpRetryError {
    pub fn from_reqwest(e: reqwest::Error) -> Self {
        HttpRetryError::Transport {
            kind: classify(&e),
            message: e.to_string(),
        }
    }
}

/// Sends an HTTP request, retrying on 429 with exponential backoff.
/// `build_request`: closure that returns a fresh `RequestBuilder` each attempt
/// (required because `RequestBuilder` is not `Clone`).
///
/// Backoff: 1s start, ×2 each retry, capped at 30s. Retries 429 only.
pub async fn send_with_retry(
    build_request: impl Fn() -> reqwest::RequestBuilder,
    max_retries: u32,
) -> Result<reqwest::Response, HttpRetryError> {
    let mut delay_ms = 1_000u64;
    for attempt in 0..=max_retries {
        let response = build_request()
            .send()
            .await
            .map_err(HttpRetryError::from_reqwest)?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if attempt == max_retries {
                return Err(HttpRetryError::RateLimited(max_retries));
            }
            warn!(
                attempt = attempt + 1,
                delay_ms, "Upstream rate limited (429); retrying"
            );
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms = (delay_ms * 2).min(30_000);
            continue;
        }
        return Ok(response);
    }
    // Unreachable: the loop always returns inside the body.
    Err(HttpRetryError::RateLimited(max_retries))
}
