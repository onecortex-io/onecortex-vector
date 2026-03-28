use std::net::SocketAddr;

/// Test server handle with base URL and pool.
pub struct TestServer {
    pub base_url: String,
    pub pool: sqlx::PgPool,
    pub api_key: String,
}

/// Start a test server on a random port with a seeded API key.
pub async fn start_test_server() -> TestServer {
    dotenvy::dotenv().ok();
    let mut config = onecortex_vector_api::config::AppConfig::from_env().unwrap();
    config.api_port = 0; // OS-assigned
    config.admin_port = 0;

    let pool = onecortex_vector_api::db::pool::create_pool(&config).await.unwrap();

    // Seed test API key
    let api_key = onecortex_vector_api::middleware::auth::seed_test_key(&pool).await;

    let reranker = onecortex_vector_api::planner::reranker::build_reranker(&config);
    let state = onecortex_vector_api::state::AppState { pool: pool.clone(), config: config.clone(), reranker };

    // Build public router
    let router = build_test_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    TestServer {
        base_url: format!("http://{addr}"),
        pool,
        api_key,
    }
}

fn build_test_router(state: onecortex_vector_api::state::AppState) -> axum::Router {
    use axum::{extract::DefaultBodyLimit, Router, routing::{get, post, delete, patch}};
    use onecortex_vector_api::handlers::{indexes, vectors, query, namespaces, health};

    Router::new()
        .route("/health",  get(health::health))
        .route("/ready",   get(health::ready))
        .route("/version", get(health::version))
        .route("/indexes",        get(indexes::list_indexes).post(indexes::create_index))
        .route("/indexes/:name",  get(indexes::describe_index)
                                  .delete(indexes::delete_index)
                                  .patch(indexes::configure_index))
        .route("/indexes/:name/describe_index_stats",
               post(indexes::describe_index_stats))
        .route("/indexes/:name/vectors/upsert",           post(vectors::upsert_vectors))
        .route("/indexes/:name/vectors/fetch",            post(vectors::fetch_vectors))
        .route("/indexes/:name/vectors/fetch_by_metadata",post(vectors::fetch_by_metadata))
        .route("/indexes/:name/vectors/delete",           post(vectors::delete_vectors))
        .route("/indexes/:name/vectors/update",           post(vectors::update_vector))
        .route("/indexes/:name/vectors/list",             get(vectors::list_vectors))
        .route("/indexes/:name/query",        post(query::query_vectors))
        .route("/indexes/:name/query/hybrid", post(query::query_hybrid))
        .route("/indexes/:name/namespaces",
               get(namespaces::list_namespaces).post(namespaces::create_namespace))
        .route("/indexes/:name/namespaces/:ns",
               get(namespaces::describe_namespace).delete(namespaces::delete_namespace))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            onecortex_vector_api::middleware::auth::auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(state)
}

/// Create a test index with a unique name. Returns the index name.
pub async fn create_test_index(server: &TestServer, dimension: i32, metric: &str) -> String {
    let name = format!("test-{}", uuid::Uuid::new_v4().simple());
    let name = &name[..name.len().min(45)]; // Ensure <= 45 chars

    let client = reqwest::Client::new();
    let resp = client.post(format!("{}/indexes", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&serde_json::json!({
            "name": name,
            "dimension": dimension,
            "metric": metric,
        }))
        .send()
        .await
        .unwrap();

    let status = resp.status();
    if status != 201 {
        let body = resp.text().await.unwrap_or_default();
        panic!("Failed to create test index (status={status}): {body}");
    }
    name.to_string()
}

/// Create a test index with BM25 enabled. Returns the index name.
pub async fn create_test_index_with_bm25(server: &TestServer, dimension: i32, metric: &str) -> String {
    let name = format!("test-{}", uuid::Uuid::new_v4().simple());
    let name = &name[..name.len().min(45)];

    let client = reqwest::Client::new();
    let resp = client.post(format!("{}/indexes", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&serde_json::json!({
            "name": name,
            "dimension": dimension,
            "metric": metric,
            "bm25_enabled": true,
        }))
        .send()
        .await
        .unwrap();

    let status = resp.status();
    if status != 201 {
        let body = resp.text().await.unwrap_or_default();
        panic!("Failed to create BM25 test index (status={status}): {body}");
    }
    name.to_string()
}

/// Delete a test index (cleanup).
pub async fn cleanup_index(server: &TestServer, name: &str) {
    let client = reqwest::Client::new();
    let _ = client.delete(format!("{}/indexes/{}", server.base_url, name))
        .header("Api-Key", &server.api_key)
        .send()
        .await;
}
