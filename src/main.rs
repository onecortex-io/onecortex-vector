use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;

// The library crate (src/lib.rs) is the canonical home for these modules.
// The binary re-uses them via the `onecortex_vector` crate name.
use onecortex_vector::{config, db, embedding, handlers, planner, state, with_observability};
use std::sync::Arc;

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

    let embedder_factory = Arc::new(embedding::EmbedderFactory::new(
        embedding::EmbedderFactoryConfig {
            openai_api_key: config.embed_openai_api_key.clone(),
            voyage_api_key: config.embed_voyage_api_key.clone(),
            cohere_api_key: config.embed_cohere_api_key.clone(),
            jina_api_key: config.embed_jina_api_key.clone(),
            tei_url: config.embed_tei_url.clone(),
            http_timeout_secs: config.embed_http_timeout_secs,
            max_retries: config.embed_max_retries,
        },
    ));
    let embed_cache = Arc::new(embedding::QueryEmbedCache::new(
        config.embed_query_cache_capacity,
        config.embed_query_cache_ttl_secs,
    ));

    let state = state::AppState {
        pool,
        config: config.clone(),
        reranker,
        embedder_factory,
        embed_cache,
    };

    let router = with_observability(build_router(state));

    let addr: SocketAddr = format!("0.0.0.0:{}", config.api_port).parse().unwrap();
    tracing::info!("Listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, router).await.unwrap();
}

fn build_router(state: state::AppState) -> Router {
    use handlers::{aliases, collections, health, namespaces, query, records, search};

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
        // Collection maintenance
        .route("/v1/collections/:name/vacuum", post(collections::vacuum))
        .route("/v1/collections/:name/reindex", post(collections::reindex))
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
        .route("/v1/collections/:name/search", post(search::search))
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
