pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod middleware;
pub mod planner;
pub mod state;

use axum::http::header::HeaderName;
use axum::Router;
use tower::ServiceBuilder;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Wraps a router with the standard observability stack:
///   1. `SetRequestIdLayer` — assign an `X-Request-Id` if the client did not.
///   2. `PropagateRequestIdLayer` — echo it back on every response.
///   3. `TraceLayer` — bind the id into the per-request tracing span.
///   4. `RequestIdTaskLocalLayer` — make the id readable from `IntoResponse`.
pub fn with_observability<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::new(X_REQUEST_ID, MakeRequestUuid))
            .layer(PropagateRequestIdLayer::new(X_REQUEST_ID))
            .layer(
                TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
                    let request_id = req
                        .extensions()
                        .get::<tower_http::request_id::RequestId>()
                        .and_then(|r| r.header_value().to_str().ok())
                        .unwrap_or("");
                    tracing::info_span!(
                        "http.request",
                        method = %req.method(),
                        uri = %req.uri(),
                        request_id = %request_id,
                    )
                }),
            )
            .layer(middleware::request_id::RequestIdTaskLocalLayer::new()),
    )
}
