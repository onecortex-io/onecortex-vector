mod common;

use reqwest::Client;
use serde_json::json;

#[tokio::test]
async fn upsert_and_fetch() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"color": "red"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"color": "blue"}},
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["upsertedCount"], 2);

    // Fetch
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/fetch", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "ids": ["v1", "v2"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["vectors"]["v1"].is_object());
    assert!(body["vectors"]["v2"].is_object());
    assert_eq!(body["vectors"]["v1"]["metadata"]["color"], "red");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn upsert_batch_too_large() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let vectors: Vec<serde_json::Value> = (0..1001)
        .map(|i| json!({"id": format!("v{i}"), "values": [1.0, 0.0, 0.0]}))
        .collect();

    let resp = client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "vectors": vectors }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn upsert_dimension_mismatch() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [{"id": "v1", "values": [1.0, 0.0]}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn upsert_sparse_values_accepted() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // sparseValues should be accepted without error
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [{
                "id": "v1",
                "values": [1.0, 0.0, 0.0],
                "sparseValues": {"indices": [0, 1], "values": [0.5, 0.3]}
            }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn fetch_by_metadata_eq() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert vectors with metadata
    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"category": "news"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"category": "sports"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/indexes/{name}/vectors/fetch_by_metadata",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "filter": {"category": {"$eq": "news"}} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 1);
    assert_eq!(vectors[0]["id"], "v1");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn fetch_by_metadata_in() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"tag": "a"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"tag": "b"}},
                {"id": "v3", "values": [0.0, 0.0, 1.0], "metadata": {"tag": "c"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/indexes/{name}/vectors/fetch_by_metadata",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "filter": {"tag": {"$in": ["a", "c"]}} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn delete_by_ids() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    // Delete v1
    client
        .post(format!("{}/indexes/{name}/vectors/delete", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "ids": ["v1"] }))
        .send()
        .await
        .unwrap();

    // Fetch -- v1 should be gone
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/fetch", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "ids": ["v1", "v2"] }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["vectors"]["v1"].is_null());
    assert!(body["vectors"]["v2"].is_object());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn delete_by_filter() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"keep": "no"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"keep": "yes"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    client
        .post(format!("{}/indexes/{name}/vectors/delete", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "filter": {"keep": {"$eq": "no"}} }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/indexes/{name}/vectors/fetch", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "ids": ["v1", "v2"] }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["vectors"]["v1"].is_null());
    assert!(body["vectors"]["v2"].is_object());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn delete_all() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    client
        .post(format!("{}/indexes/{name}/vectors/delete", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "deleteAll": true }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/indexes/{name}/vectors/fetch", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "ids": ["v1", "v2"] }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_object().unwrap();
    assert!(vectors.is_empty());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn update_metadata() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [{"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"a": 1}}]
        }))
        .send()
        .await
        .unwrap();

    // Update: merge metadata
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/update", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "id": "v1", "setMetadata": {"b": 2} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Fetch and verify merge
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/fetch", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "ids": ["v1"] }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["vectors"]["v1"]["metadata"]["a"], 1);
    assert_eq!(body["vectors"]["v1"]["metadata"]["b"], 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn list_vectors_with_prefix() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "doc-1", "values": [1.0, 0.0, 0.0]},
                {"id": "doc-2", "values": [0.0, 1.0, 0.0]},
                {"id": "img-1", "values": [0.0, 0.0, 1.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .get(format!(
            "{}/indexes/{name}/vectors/list?prefix=doc-",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn list_vectors_pagination() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "a", "values": [1.0, 0.0, 0.0]},
                {"id": "b", "values": [0.0, 1.0, 0.0]},
                {"id": "c", "values": [0.0, 0.0, 1.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    // First page: limit=2
    let resp = client
        .get(format!(
            "{}/indexes/{name}/vectors/list?limit=2",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 2);
    let next = body["pagination"]["next"].as_str().unwrap();

    // Second page
    let resp = client
        .get(format!(
            "{}/indexes/{name}/vectors/list?limit=2&paginationToken={next}",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 1);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn scroll_returns_full_vector_data() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"color": "red"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"color": "blue"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/indexes/{name}/vectors/scroll", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "includeValues": true,
            "includeMetadata": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 2);
    // Verify values and metadata are present
    assert!(vectors[0]["values"].is_array());
    assert!(vectors[0]["metadata"].is_object());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn scroll_pagination_with_cursor() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert 5 vectors with predictable IDs (alphabetical ordering matters for cursor)
    let vectors: Vec<serde_json::Value> = (1..=5)
        .map(|i| json!({"id": format!("v{i}"), "values": [1.0, 0.0, 0.0]}))
        .collect();
    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"vectors": vectors}))
        .send()
        .await
        .unwrap();

    // Page 1: limit=2
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/scroll", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"limit": 2}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["vectors"].as_array().unwrap().len(), 2);
    assert!(body["nextCursor"].is_string());

    // Page 2: use cursor
    let cursor = body["nextCursor"].as_str().unwrap();
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/scroll", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"limit": 2, "cursor": cursor}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["vectors"].as_array().unwrap().len(), 2);

    // Page 3: last page — 1 vector remains, no nextCursor
    let cursor = body["nextCursor"].as_str().unwrap();
    let resp = client
        .post(format!("{}/indexes/{name}/vectors/scroll", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"limit": 2, "cursor": cursor}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["vectors"].as_array().unwrap().len(), 1);
    assert!(body["nextCursor"].is_null() || body.get("nextCursor").is_none());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn scroll_with_filter() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"status": "active"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"status": "archived"}},
                {"id": "v3", "values": [0.0, 0.0, 1.0], "metadata": {"status": "active"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/indexes/{name}/vectors/scroll", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "filter": {"status": {"$eq": "active"}},
            "includeMetadata": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 2);
    for v in vectors {
        assert_eq!(v["metadata"]["status"], "active");
    }

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn sample_returns_random_vectors() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let vectors: Vec<serde_json::Value> = (1..=20)
        .map(|i| json!({"id": format!("v{i}"), "values": [1.0, 0.0, 0.0]}))
        .collect();
    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"vectors": vectors}))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/indexes/{name}/sample", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"size": 5, "includeMetadata": true}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert!(vectors.len() <= 5);
    assert!(!vectors.is_empty());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn sample_with_filter() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"type": "a"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"type": "b"}},
                {"id": "v3", "values": [0.0, 0.0, 1.0], "metadata": {"type": "a"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/indexes/{name}/sample", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "size": 10,
            "filter": {"type": {"$eq": "a"}},
            "includeMetadata": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let vectors = body["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 2);
    for v in vectors {
        assert_eq!(v["metadata"]["type"], "a");
    }

    common::cleanup_index(&server, &name).await;
}
