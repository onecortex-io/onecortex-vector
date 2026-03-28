mod common;

use reqwest::Client;
use serde_json::json;

#[tokio::test]
async fn hybrid_query_requires_bm25_enabled() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert a vector
    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [0.1, 0.2, 0.3], "text": "the quick brown fox"}
            ]
        }))
        .send().await.unwrap();

    // Hybrid query on a non-BM25 index should return 400
    let resp = client.post(format!("{}/indexes/{name}/query/hybrid", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "text": "fox",
            "topK": 3,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_ARGUMENT");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn hybrid_query_returns_results() {
    let server = common::start_test_server().await;
    let name = common::create_test_index_with_bm25(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert vectors with text content
    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "text": "the quick brown fox"},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "text": "lazy dog sleeps all day"},
                {"id": "v3", "values": [0.0, 0.0, 1.0], "text": "quick fox jumps high"},
            ]
        }))
        .send().await.unwrap();

    let resp = client.post(format!("{}/indexes/{name}/query/hybrid", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "text": "quick fox",
            "topK": 3,
            "alpha": 0.5,
            "includeMetadata": true,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert!(!matches.is_empty(), "Expected non-empty matches");

    // v1 and v3 both mention "quick fox" — they should appear in top results
    let ids: Vec<&str> = matches.iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(ids.contains(&"v1") || ids.contains(&"v3"),
        "Expected v1 or v3 in hybrid results, got: {:?}", ids);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn hybrid_query_topk_max_enforced() {
    let server = common::start_test_server().await;
    let client = Client::new();

    // Create any index — the topK validation happens before the BM25 check
    let name = common::create_test_index_with_bm25(&server, 3, "cosine").await;

    let resp = client.post(format!("{}/indexes/{name}/query/hybrid", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "text": "fox",
            "topK": 10001,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn patch_index_enables_bm25() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Enable BM25 via PATCH
    let resp = client.patch(format!("{}/indexes/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "bm25Enabled": true }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // Describe should show bm25Enabled: true
    let resp = client.get(format!("{}/indexes/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["bm25Enabled"], true);

    common::cleanup_index(&server, &name).await;
}
