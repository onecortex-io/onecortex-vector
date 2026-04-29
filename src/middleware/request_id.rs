//! Bridges the `X-Request-Id` carried in request extensions (set by
//! `tower_http::request_id::SetRequestIdLayer`) into a `tokio::task_local!`
//! so that `IntoResponse` impls — which never see the request — can read
//! the id and embed it in error bodies.

use axum::http::Request;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tower_http::request_id::RequestId;

tokio::task_local! {
    pub static REQUEST_ID: String;
}

/// Returns the current request id if the calling future is running inside a
/// request scope. Returns `None` for background tasks, tests that call
/// `into_response()` directly, etc.
pub fn current() -> Option<String> {
    REQUEST_ID.try_with(|id| id.clone()).ok()
}

#[derive(Clone, Default)]
pub struct RequestIdTaskLocalLayer;

impl RequestIdTaskLocalLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestIdTaskLocalLayer {
    type Service = RequestIdTaskLocal<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestIdTaskLocal { inner }
    }
}

#[derive(Clone)]
pub struct RequestIdTaskLocal<S> {
    inner: S,
}

impl<S, B> Service<Request<B>> for RequestIdTaskLocal<S>
where
    S: Service<Request<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let id = req
            .extensions()
            .get::<RequestId>()
            .and_then(|r| r.header_value().to_str().ok())
            .map(String::from);

        // Standard "ready service was polled, swap and call" pattern: the
        // cloned `self.inner` is not yet ready; we keep the polled-ready
        // instance in `inner`.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let fut = inner.call(req);

        Box::pin(async move {
            match id {
                Some(id) => REQUEST_ID.scope(id, fut).await,
                None => fut.await,
            }
        })
    }
}
