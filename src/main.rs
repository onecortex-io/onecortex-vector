use axum::{
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;

mod config;
mod db;
mod error;
mod handlers;
mod middleware;
mod planner;
mod state;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let config = config::AppConfig::from_env().expect("Failed to load configuration");

    // Initialize structured JSON logging
    tracing_subscriber::fmt()
        .with_env_filter(&config.log_level)
        .json()
        .init();

    let pool = db::pool::create_pool(&config)
        .await
        .expect("Failed to connect to database and run migrations");

    let reranker = planner::reranker::build_reranker(&config);

    let state = state::AppState {
        pool,
        config: config.clone(),
        reranker,
    };

    // Public API router -- all endpoints except /metrics
    let public_router = build_public_router(state.clone());

    // Admin router -- metrics + admin endpoints, internal port only
    let admin_router = build_admin_router(state.clone());

    let public_addr: SocketAddr = format!("0.0.0.0:{}", config.api_port).parse().unwrap();
    let admin_addr: SocketAddr = format!("0.0.0.0:{}", config.admin_port).parse().unwrap();

    tracing::info!("Public API listening on {public_addr}");
    tracing::info!("Admin API listening on {admin_addr}");

    let public_listener = tokio::net::TcpListener::bind(public_addr).await.unwrap();
    let admin_listener = tokio::net::TcpListener::bind(admin_addr).await.unwrap();

    tokio::select! {
        _ = axum::serve(public_listener, public_router) => {},
        _ = axum::serve(admin_listener, admin_router)  => {},
    }
}

fn build_public_router(state: state::AppState) -> Router {
    use handlers::{health, indexes, namespaces, query, vectors};

    Router::new()
        // Health (exempt from auth)
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/version", get(health::version))
        // Control plane -- index management
        .route(
            "/indexes",
            get(indexes::list_indexes).post(indexes::create_index),
        )
        .route(
            "/indexes/:name",
            get(indexes::describe_index)
                .delete(indexes::delete_index)
                .patch(indexes::configure_index),
        )
        .route(
            "/indexes/:name/describe_index_stats",
            post(indexes::describe_index_stats),
        )
        // Data plane -- vector operations
        .route(
            "/indexes/:name/vectors/upsert",
            post(vectors::upsert_vectors),
        )
        .route("/indexes/:name/vectors/fetch", post(vectors::fetch_vectors))
        .route(
            "/indexes/:name/vectors/fetch_by_metadata",
            post(vectors::fetch_by_metadata),
        )
        .route(
            "/indexes/:name/vectors/delete",
            post(vectors::delete_vectors),
        )
        .route(
            "/indexes/:name/vectors/update",
            post(vectors::update_vector),
        )
        .route("/indexes/:name/vectors/list", get(vectors::list_vectors))
        // Query
        .route("/indexes/:name/query", post(query::query_vectors))
        .route("/indexes/:name/query/hybrid", post(query::query_hybrid))
        // Namespace CRUD
        .route(
            "/indexes/:name/namespaces",
            get(namespaces::list_namespaces).post(namespaces::create_namespace),
        )
        .route(
            "/indexes/:name/namespaces/:ns",
            get(namespaces::describe_namespace).delete(namespaces::delete_namespace),
        )
        // Apply auth middleware to all routes
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::auth::auth_middleware,
        ))
        .with_state(state)
}

fn build_admin_router(state: state::AppState) -> Router {
    use handlers::{admin, health};
    Router::new()
        .route("/metrics", get(health::metrics))
        .route("/admin/indexes/:name/reindex", post(admin::reindex))
        .route("/admin/indexes/:name/vacuum", post(admin::vacuum))
        .route("/admin/api_keys", post(admin::create_api_key))
        .route("/admin/api_keys/:id", delete(admin::revoke_api_key))
        .route("/admin/config", get(admin::dump_config))
        .with_state(state)
}
