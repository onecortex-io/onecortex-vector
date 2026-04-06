mod common;

use reqwest::Client;
use serde_json::json;

#[tokio::test]
async fn query_cosine() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/collections/{name}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
                {"id": "v3", "values": [0.0, 0.0, 1.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/collections/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 3,
            "includeValues": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 3);
    // First match should be v1 (identical vector), score close to 1.0
    assert_eq!(matches[0]["id"], "v1");
    let score = matches[0]["score"].as_f64().unwrap();
    assert!(
        score > 0.99,
        "Cosine score for identical vector should be ~1.0, got {score}"
    );
    assert!(score <= 1.0);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_euclidean() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "euclidean").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/collections/{name}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/collections/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 2,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    // Score for identical vector: 1/(1+0) = 1.0
    let score = matches[0]["score"].as_f64().unwrap();
    assert!(
        (score - 1.0).abs() < 0.01,
        "Euclidean score for identical vector should be 1.0, got {score}"
    );
    // Score for distance=sqrt(2): 1/(1+1.414) ~= 0.414
    let score2 = matches[1]["score"].as_f64().unwrap();
    assert!(
        score2 > 0.0 && score2 < 1.0,
        "Euclidean score should be in (0,1), got {score2}"
    );

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_dotproduct() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "dotproduct").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/collections/{name}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.5, 0.5, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/collections/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 2,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    // v1 dot [1,0,0] = 1.0, v2 dot [1,0,0] = 0.5
    let score1 = matches[0]["score"].as_f64().unwrap();
    let score2 = matches[1]["score"].as_f64().unwrap();
    assert!(
        score1 > score2,
        "Higher dot product should have higher score"
    );
    assert!(
        (score1 - 1.0).abs() < 0.01,
        "Dotproduct score for [1,0,0] . [1,0,0] should be 1.0, got {score1}"
    );

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_with_metadata_filter() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/collections/{name}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"category": "news"}},
                {"id": "v2", "values": [0.9, 0.1, 0.0], "metadata": {"category": "sports"}},
                {"id": "v3", "values": [0.8, 0.2, 0.0], "metadata": {"category": "news"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/collections/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "filter": {"category": {"$eq": "news"}},
            "includeMetadata": true,
        }))
        .send()
        .await
        .unwrap();
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

    let resp = client
        .post(format!("{}/collections/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10001,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_by_id() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/collections/{name}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/collections/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "id": "v1",
            "topK": 2,
        }))
        .send()
        .await
        .unwrap();
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
    client
        .post(format!(
            "{}/collections/{name}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "namespace": "ns1",
            "records": [{"id": "v1", "values": [1.0, 0.0, 0.0]}]
        }))
        .send()
        .await
        .unwrap();

    // Upsert to ns2
    client
        .post(format!(
            "{}/collections/{name}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "namespace": "ns2",
            "records": [{"id": "v2", "values": [0.0, 1.0, 0.0]}]
        }))
        .send()
        .await
        .unwrap();

    // Query ns1 -- should only get v1
    let resp = client
        .post(format!("{}/collections/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "namespace": "ns1",
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["id"], "v1");

    // Query ns2 -- should only get v2
    let resp = client
        .post(format!("{}/collections/{name}/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "namespace": "ns2",
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["id"], "v2");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_score_threshold_filters_low_scores() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    // v1 is identical to query (score ~1.0), v2 is orthogonal (score ~0.0)
    client
        .post(format!(
            "{}/collections/{collection}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    // High threshold — only v1 (score ~1.0) should pass
    let resp = client
        .post(format!(
            "{}/collections/{collection}/query",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "scoreThreshold": 0.9
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["id"], "v1");

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn query_score_threshold_zero_returns_all() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    client
        .post(format!(
            "{}/collections/{collection}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/query",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "scoreThreshold": 0.0
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"].as_array().unwrap().len(), 2);

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn query_score_threshold_out_of_range_returns_400() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/query",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "scoreThreshold": 1.5
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn batch_query_returns_multiple_results() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    client
        .post(format!(
            "{}/collections/{collection}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
                {"id": "v3", "values": [0.0, 0.0, 1.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/query/batch",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "queries": [
                {"vector": [1.0, 0.0, 0.0], "topK": 2},
                {"vector": [0.0, 1.0, 0.0], "topK": 1}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["matches"].as_array().unwrap().len(), 2);
    assert_eq!(results[1]["matches"].as_array().unwrap().len(), 1);
    // First query should have v1 as top match
    assert_eq!(results[0]["matches"][0]["id"], "v1");
    // Second query should have v2 as top match
    assert_eq!(results[1]["matches"][0]["id"], "v2");

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn batch_query_empty_returns_400() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/query/batch",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({"queries": []}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn batch_query_exceeds_limit_returns_400() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    let queries: Vec<serde_json::Value> = (0..11)
        .map(|_| json!({"vector": [1.0, 0.0, 0.0], "topK": 1}))
        .collect();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/query/batch",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({"queries": queries}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn query_group_by_metadata_field() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    client
        .post(format!(
            "{}/collections/{collection}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "n1-c1", "values": [0.9, 0.1, 0.0], "metadata": {"doc": "news-1"}},
                {"id": "n1-c2", "values": [0.8, 0.2, 0.0], "metadata": {"doc": "news-1"}},
                {"id": "n1-c3", "values": [0.7, 0.3, 0.0], "metadata": {"doc": "news-1"}},
                {"id": "s1-c1", "values": [0.6, 0.4, 0.0], "metadata": {"doc": "sports-1"}},
                {"id": "s1-c2", "values": [0.5, 0.5, 0.0], "metadata": {"doc": "sports-1"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/query",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "includeMetadata": true,
            "groupBy": {
                "field": "doc",
                "limit": 10,
                "groupSize": 2
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let groups = body["matches"].as_array().unwrap();
    // Should have 2 groups: news-1 and sports-1
    assert_eq!(groups.len(), 2);
    // First group should be news-1 (higher scores)
    assert_eq!(groups[0]["group"], "news-1");
    // Each group capped at groupSize=2
    assert!(groups[0]["matches"].as_array().unwrap().len() <= 2);
    assert!(groups[1]["matches"].as_array().unwrap().len() <= 2);

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn query_group_by_caps_per_group() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    // 5 records all in same group
    let records: Vec<serde_json::Value> = (1..=5)
        .map(|i| {
            json!({
                "id": format!("v{i}"),
                "values": [1.0 - (i as f32 * 0.1), i as f32 * 0.1, 0.0],
                "metadata": {"doc": "same-doc"}
            })
        })
        .collect();
    client
        .post(format!(
            "{}/collections/{collection}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({"records": records}))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/query",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 10,
            "includeMetadata": true,
            "groupBy": {"field": "doc", "limit": 10, "groupSize": 2}
        }))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    let groups = body["matches"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    // Capped at groupSize=2 even though 5 records match
    assert_eq!(groups[0]["matches"].as_array().unwrap().len(), 2);

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn query_without_group_by_returns_flat_matches() {
    // Ensure the return type change doesn't break standard queries
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    client
        .post(format!(
            "{}/collections/{collection}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [{"id": "v1", "values": [1.0, 0.0, 0.0]}]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/query",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({"vector": [1.0, 0.0, 0.0], "topK": 1}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    // Standard response: matches is array of {id, score, ...}
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["id"], "v1");
    assert!(matches[0]["score"].is_f64());

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn recommend_basic() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    client
        .post(format!(
            "{}/collections/{collection}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.9, 0.1, 0.0]},
                {"id": "v3", "values": [0.0, 1.0, 0.0]},
                {"id": "v4", "values": [0.0, 0.0, 1.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/recommend",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "positiveIds": ["v1"],
            "topK": 3
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    // v1 should NOT appear (excluded as input)
    assert!(matches.iter().all(|m| m["id"] != "v1"));
    // v2 should be top match (most similar to v1)
    assert_eq!(matches[0]["id"], "v2");

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn recommend_with_negatives() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    client
        .post(format!(
            "{}/collections/{collection}/records/upsert",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.5, 0.5, 0.0]},
                {"id": "v3", "values": [0.0, 1.0, 0.0]},
                {"id": "v4", "values": [0.9, 0.0, 0.1]},
            ]
        }))
        .send()
        .await
        .unwrap();

    // Positive: v1=[1,0,0], Negative: v3=[0,1,0]
    let resp = client
        .post(format!(
            "{}/collections/{collection}/recommend",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "positiveIds": ["v1"],
            "negativeIds": ["v3"],
            "topK": 2
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert!(!matches.is_empty());
    // v1 and v3 excluded (input IDs)
    assert!(matches.iter().all(|m| m["id"] != "v1" && m["id"] != "v3"));

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn recommend_empty_positives_returns_400() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/recommend",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "positiveIds": [],
            "topK": 5
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn recommend_nonexistent_positive_returns_404() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "{}/collections/{collection}/recommend",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({
            "positiveIds": ["nonexistent"],
            "topK": 5
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);

    common::cleanup_index(&server, &collection).await;
}
