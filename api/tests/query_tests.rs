mod common;

use reqwest::Client;
use serde_json::json;

#[tokio::test]
async fn query_cosine() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
                {"id": "v3", "values": [0.0, 0.0, 1.0]},
            ]
        }))
        .send().await.unwrap();

    let resp = client.post(format!("{}/indexes/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 3,
            "includeValues": true,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 3);
    // First match should be v1 (identical vector), score close to 1.0
    assert_eq!(matches[0]["id"], "v1");
    let score = matches[0]["score"].as_f64().unwrap();
    assert!(score > 0.99, "Cosine score for identical vector should be ~1.0, got {score}");
    assert!(score <= 1.0);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_euclidean() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "euclidean").await;
    let client = Client::new();

    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send().await.unwrap();

    let resp = client.post(format!("{}/indexes/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 2,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    // Score for identical vector: 1/(1+0) = 1.0
    let score = matches[0]["score"].as_f64().unwrap();
    assert!((score - 1.0).abs() < 0.01, "Euclidean score for identical vector should be 1.0, got {score}");
    // Score for distance=sqrt(2): 1/(1+1.414) ~= 0.414
    let score2 = matches[1]["score"].as_f64().unwrap();
    assert!(score2 > 0.0 && score2 < 1.0, "Euclidean score should be in (0,1), got {score2}");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_dotproduct() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "dotproduct").await;
    let client = Client::new();

    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.5, 0.5, 0.0]},
            ]
        }))
        .send().await.unwrap();

    let resp = client.post(format!("{}/indexes/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 2,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    // v1 dot [1,0,0] = 1.0, v2 dot [1,0,0] = 0.5
    let score1 = matches[0]["score"].as_f64().unwrap();
    let score2 = matches[1]["score"].as_f64().unwrap();
    assert!(score1 > score2, "Higher dot product should have higher score");
    assert!((score1 - 1.0).abs() < 0.01, "Dotproduct score for [1,0,0] . [1,0,0] should be 1.0, got {score1}");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_with_metadata_filter() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"category": "news"}},
                {"id": "v2", "values": [0.9, 0.1, 0.0], "metadata": {"category": "sports"}},
                {"id": "v3", "values": [0.8, 0.2, 0.0], "metadata": {"category": "news"}},
            ]
        }))
        .send().await.unwrap();

    let resp = client.post(format!("{}/indexes/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "filter": {"category": {"$eq": "news"}},
            "includeMetadata": true,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2);
    for m in matches {
        assert_eq!(m["metadata"]["category"], "news");
    }

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_top_k_too_large() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client.post(format!("{}/indexes/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10001,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_by_id() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vectors": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send().await.unwrap();

    let resp = client.post(format!("{}/indexes/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "id": "v1",
            "topK": 2,
        }))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert!(!matches.is_empty());
    // v1 should match itself with highest score
    assert_eq!(matches[0]["id"], "v1");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_namespace_isolation() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert to ns1
    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "namespace": "ns1",
            "vectors": [{"id": "v1", "values": [1.0, 0.0, 0.0]}]
        }))
        .send().await.unwrap();

    // Upsert to ns2
    client.post(format!("{}/indexes/{name}/vectors/upsert", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "namespace": "ns2",
            "vectors": [{"id": "v2", "values": [0.0, 1.0, 0.0]}]
        }))
        .send().await.unwrap();

    // Query ns1 -- should only get v1
    let resp = client.post(format!("{}/indexes/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "namespace": "ns1",
        }))
        .send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["id"], "v1");

    // Query ns2 -- should only get v2
    let resp = client.post(format!("{}/indexes/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "namespace": "ns2",
        }))
        .send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["id"], "v2");

    common::cleanup_index(&server, &name).await;
}
