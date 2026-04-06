/// Test server handle with base URL and pool.
pub struct TestServer {
    pub base_url: String,
    pub pool: sqlx::PgPool,
    pub api_key: String,
}

/// Start a test server on a random port with a seeded API key.
pub async fn start_test_server() -> TestServer {
    dotenvy::dotenv().ok();
    let mut config = onecortex_vector::config::AppConfig::from_env().unwrap();
    config.api_port = 0; // OS-assigned
    config.admin_port = 0;

    let pool = onecortex_vector::db::pool::create_pool(&config)
        .await
        .unwrap();

    // Seed test API key
    let api_key = onecortex_vector::middleware::auth::seed_test_key(&pool).await;

    let reranker = onecortex_vector::planner::reranker::build_reranker(&config);
    let state = onecortex_vector::state::AppState {
        pool: pool.clone(),
        config: config.clone(),
        reranker,
    };

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

fn build_test_router(state: onecortex_vector::state::AppState) -> axum::Router {
    use axum::{
        extract::DefaultBodyLimit,
        routing::{get, post},
        Router,
    };
    use onecortex_vector::handlers::{aliases, collections, health, namespaces, query, records};

    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/version", get(health::version))
        .route(
            "/collections",
            get(collections::list_collections).post(collections::create_collection),
        )
        .route(
            "/collections/:name",
            get(collections::describe_collection)
                .delete(collections::delete_collection)
                .patch(collections::configure_collection),
        )
        .route(
            "/collections/:name/describe_collection_stats",
            post(collections::describe_collection_stats),
        )
        .route(
            "/collections/:name/records/upsert",
            post(records::upsert_records),
        )
        .route(
            "/collections/:name/records/fetch",
            post(records::fetch_records),
        )
        .route(
            "/collections/:name/records/fetch_by_metadata",
            post(records::fetch_by_metadata),
        )
        .route(
            "/collections/:name/records/delete",
            post(records::delete_records),
        )
        .route(
            "/collections/:name/records/update",
            post(records::update_record),
        )
        .route(
            "/collections/:name/records/list",
            get(records::list_records),
        )
        .route(
            "/collections/:name/records/scroll",
            post(records::scroll_records),
        )
        .route("/collections/:name/sample", post(records::sample_records))
        .route("/collections/:name/query", post(query::query_vectors))
        .route("/collections/:name/query/hybrid", post(query::query_hybrid))
        .route("/collections/:name/query/batch", post(query::query_batch))
        .route("/collections/:name/recommend", post(query::recommend))
        .route(
            "/collections/:name/namespaces",
            get(namespaces::list_namespaces).post(namespaces::create_namespace),
        )
        .route(
            "/collections/:name/namespaces/:ns",
            get(namespaces::describe_namespace).delete(namespaces::delete_namespace),
        )
        .route(
            "/aliases",
            get(aliases::list_aliases).post(aliases::create_alias),
        )
        .route(
            "/aliases/:alias",
            get(aliases::describe_alias).delete(aliases::delete_alias),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            onecortex_vector::middleware::auth::auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(state)
}

/// Create a test collection with a unique name. Returns the collection name.
pub async fn create_test_index(server: &TestServer, dimension: i32, metric: &str) -> String {
    let name = format!("test-{}", uuid::Uuid::new_v4().simple());
    let name = &name[..name.len().min(45)]; // Ensure <= 45 chars

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/collections", server.base_url))
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
        .post(format!("{}/collections", server.base_url))
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
        panic!("Failed to create BM25 test collection (status={status}): {body}");
    }
    name.to_string()
}

/// Delete a test collection (cleanup).
pub async fn cleanup_index(server: &TestServer, name: &str) {
    let client = reqwest::Client::new();
    let _ = client
        .delete(format!("{}/collections/{}", server.base_url, name))
        .header("Api-Key", &server.api_key)
        .send()
        .await;
}
