use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;

mod config;
mod db;
mod error;
mod handlers;
mod planner;
mod state;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let config = config::AppConfig::from_env().expect("Failed to load configuration");

    // Initialize simple text logging
    tracing_subscriber::fmt()
        .with_env_filter(&config.log_level)
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
    use handlers::{aliases, collections, health, namespaces, query, records};

    Router::new()
        // Health (exempt from auth)
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/version", get(health::version))
        // Control plane -- collection management
        .route(
            "/v1/collections",
            get(collections::list_collections).post(collections::create_collection),
        )
        .route(
            "/v1/collections/:name",
            get(collections::describe_collection)
                .delete(collections::delete_collection)
                .patch(collections::configure_collection),
        )
        .route(
            "/v1/collections/:name/describe_collection_stats",
            post(collections::describe_collection_stats),
        )
        // Data plane -- record operations
        .route(
            "/v1/collections/:name/records/upsert",
            post(records::upsert_records),
        )
        .route(
            "/v1/collections/:name/records/fetch",
            post(records::fetch_records),
        )
        .route(
            "/v1/collections/:name/records/fetch_by_metadata",
            post(records::fetch_by_metadata),
        )
        .route(
            "/v1/collections/:name/records/delete",
            post(records::delete_records),
        )
        .route(
            "/v1/collections/:name/records/update",
            post(records::update_record),
        )
        .route(
            "/v1/collections/:name/records/list",
            get(records::list_records),
        )
        .route(
            "/v1/collections/:name/records/scroll",
            post(records::scroll_records),
        )
        .route(
            "/v1/collections/:name/sample",
            post(records::sample_records),
        )
        // Query
        .route("/v1/collections/:name/query", post(query::query_vectors))
        .route(
            "/v1/collections/:name/query/hybrid",
            post(query::query_hybrid),
        )
        .route(
            "/v1/collections/:name/query/batch",
            post(query::query_batch),
        )
        .route("/v1/collections/:name/recommend", post(query::recommend))
        .route("/v1/collections/:name/facets", post(query::facets))
        // Namespace CRUD
        .route(
            "/v1/collections/:name/namespaces",
            get(namespaces::list_namespaces).post(namespaces::create_namespace),
        )
        .route(
            "/v1/collections/:name/namespaces/:ns",
            get(namespaces::describe_namespace).delete(namespaces::delete_namespace),
        )
        // Aliases
        .route(
            "/v1/aliases",
            get(aliases::list_aliases).post(aliases::create_alias),
        )
        .route(
            "/v1/aliases/:alias",
            get(aliases::describe_alias).delete(aliases::delete_alias),
        )
        .with_state(state)
}

fn build_admin_router(state: state::AppState) -> Router {
    use handlers::{admin, health};
    Router::new()
        .route("/metrics", get(health::metrics))
        .route("/admin/collections/:name/reindex", post(admin::reindex))
        .route("/admin/collections/:name/vacuum", post(admin::vacuum))
        .route("/admin/config", get(admin::dump_config))
        .with_state(state)
}
