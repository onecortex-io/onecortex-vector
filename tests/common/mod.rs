// Each integration-test binary independently includes this module; not every
// binary uses every helper, so per-binary dead-code lints are noisy and
// pre-existing.
#![allow(dead_code)]

/// Test server handle with base URL and pool.
pub struct TestServer {
    pub base_url: String,
    pub pool: sqlx::PgPool,
}

/// Start a test server on a random port.
pub async fn start_test_server() -> TestServer {
    dotenvy::dotenv().ok();
    let mut config = onecortex_vector::config::AppConfig::from_env().unwrap();
    config.api_port = 0; // OS-assigned

    let pool = onecortex_vector::db::pool::create_pool(&config)
        .await
        .unwrap();

    let reranker = onecortex_vector::planner::reranker::build_reranker(&config);
    let embedder_factory = std::sync::Arc::new(onecortex_vector::embedding::EmbedderFactory::new(
        onecortex_vector::embedding::EmbedderFactoryConfig {
            openai_api_key: config.embed_openai_api_key.clone(),
            voyage_api_key: config.embed_voyage_api_key.clone(),
            cohere_api_key: config.embed_cohere_api_key.clone(),
            jina_api_key: config.embed_jina_api_key.clone(),
            tei_url: config.embed_tei_url.clone(),
            http_timeout_secs: config.embed_http_timeout_secs,
            max_retries: config.embed_max_retries,
        },
    ));
    let embed_cache = std::sync::Arc::new(onecortex_vector::embedding::QueryEmbedCache::new(
        config.embed_query_cache_capacity,
        config.embed_query_cache_ttl_secs,
    ));
    let state = onecortex_vector::state::AppState {
        pool: pool.clone(),
        config: config.clone(),
        reranker,
        embedder_factory,
        embed_cache,
    };

    let router = onecortex_vector::with_observability(build_test_router(state));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    TestServer {
        base_url: format!("http://{addr}"),
        pool,
    }
}

fn build_test_router(state: onecortex_vector::state::AppState) -> axum::Router {
    use axum::{
        extract::DefaultBodyLimit,
        routing::{get, post},
        Router,
    };
    use onecortex_vector::handlers::{
        aliases, collections, health, namespaces, query, records, search,
    };

    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/version", get(health::version))
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
        .route(
            "/v1/collections/:name/namespaces",
            get(namespaces::list_namespaces).post(namespaces::create_namespace),
        )
        .route(
            "/v1/collections/:name/namespaces/:ns",
            get(namespaces::describe_namespace).delete(namespaces::delete_namespace),
        )
        .route(
            "/v1/aliases",
            get(aliases::list_aliases).post(aliases::create_alias),
        )
        .route(
            "/v1/aliases/:alias",
            get(aliases::describe_alias).delete(aliases::delete_alias),
        )
        .route("/v1/collections/:name/vacuum", post(collections::vacuum))
        .route("/v1/collections/:name/reindex", post(collections::reindex))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(state)
}

/// Create a test collection with a unique name. Returns the collection name.
pub async fn create_test_index(server: &TestServer, dimension: i32, metric: &str) -> String {
    let name = format!("test-{}", uuid::Uuid::new_v4().simple());
    let name = &name[..name.len().min(45)]; // Ensure <= 45 chars

    let client = reqwest::Client::new();
    // Explicitly disable BM25 here so tests that need a non-BM25 collection
    // keep working after the v0.3 default flip. Tests that *want* BM25 should
    // call `create_test_index_with_bm25` instead.
    let resp = client
        .post(format!("{}/v1/collections", server.base_url))
        .json(&serde_json::json!({
            "name": name,
            "dimension": dimension,
            "metric": metric,
            "bm25Enabled": false,
        }))
        .send()
        .await
        .unwrap();

    let status = resp.status();
    if status != 201 {
        let body = resp.text().await.unwrap_or_default();
        panic!("Failed to create test collection (status={status}): {body}");
    }
    name.to_string()
}

/// Create a test collection with BM25 enabled. Returns the collection name.
pub async fn create_test_index_with_bm25(
    server: &TestServer,
    dimension: i32,
    metric: &str,
) -> String {
    let name = format!("test-{}", uuid::Uuid::new_v4().simple());
    let name = &name[..name.len().min(45)];

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/collections", server.base_url))
        .json(&serde_json::json!({
            "name": name,
            "dimension": dimension,
            "metric": metric,
            "bm25Enabled": true,
        }))
        .send()
        .await
        .unwrap();

    let status = resp.status();
    if status != 201 {
        let body = resp.text().await.unwrap_or_default();
        panic!("Failed to create BM25 test collection (status={status}): {body}");
    }
    name.to_string()
}

/// Delete a test collection (cleanup).
pub async fn cleanup_index(server: &TestServer, name: &str) {
    let client = reqwest::Client::new();
    let _ = client
        .delete(format!("{}/v1/collections/{}", server.base_url, name))
        .send()
        .await;
}
