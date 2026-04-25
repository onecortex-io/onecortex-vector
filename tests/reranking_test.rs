mod common;

use reqwest::Client;
use serde_json::json;

// ─── Basic behavior ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_query_without_rerank_unchanged() {
    // Baseline: no rerank field → behavior identical to Phase 1.
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "v1", "values": [0.1, 0.2, 0.3], "metadata": {"text": "quick fox"}}
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({"vector": [0.1, 0.2, 0.3], "topK": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"].as_array().unwrap().len(), 1);
    let score = body["matches"][0]["score"].as_f64().unwrap();
    assert!(score > 0.0, "ANN score should be positive (cosine)");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn test_rerank_top_n_truncates_results() {
    // topK=3, topN=1 → exactly 1 result returned after reranking (NoopReranker).
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "v1", "values": [0.1, 0.2, 0.3], "metadata": {"text": "fox"}},
                {"id": "v2", "values": [0.4, 0.5, 0.6], "metadata": {"text": "dog"}},
                {"id": "v3", "values": [0.7, 0.8, 0.9], "metadata": {"text": "cat"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "topK": 3,
            "includeMetadata": true,
            "rerank": { "query": "fox", "topN": 1, "rankField": "text" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"].as_array().unwrap().len(), 1);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn test_rerank_top_n_defaults_to_top_k() {
    // When topN is absent, returns topK results.
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "v1", "values": [0.1, 0.2, 0.3], "metadata": {"text": "a"}},
                {"id": "v2", "values": [0.4, 0.5, 0.6], "metadata": {"text": "b"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "topK": 2,
            "includeMetadata": true,
            "rerank": { "query": "a" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"].as_array().unwrap().len(), 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn test_rerank_missing_rank_field_falls_back_to_id() {
    // If rankField metadata key is absent, the candidate id is used as text.
    // Must not 500 — graceful degradation.
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "v1", "values": [0.1, 0.2, 0.3], "metadata": {"category": "animals"}}
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "topK": 1,
            "includeMetadata": true,
            "rerank": { "query": "animals", "rankField": "text" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    common::cleanup_index(&server, &name).await;
}

// ─── Custom rankField ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_rerank_custom_rank_field() {
    // rankField can point to any metadata key — not just "text".
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client.post(format!("{}/v1/collections/{name}/records/upsert", server.base_url))
        .json(&json!({
            "records": [
                {"id": "v1", "values": [0.1, 0.2, 0.3], "metadata": {"content": "machine learning models"}},
                {"id": "v2", "values": [0.4, 0.5, 0.6], "metadata": {"content": "gardening tips"}},
            ]
        }))
        .send().await.unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "topK": 2,
            "includeMetadata": true,
            "rerank": { "query": "deep learning", "topN": 1, "rankField": "content" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"].as_array().unwrap().len(), 1);

    common::cleanup_index(&server, &name).await;
}

// ─── Hybrid query with reranking ──────────────────────────────────────────────

#[tokio::test]
async fn test_hybrid_query_with_rerank() {
    let server = common::start_test_server().await;
    let name = common::create_test_index_with_bm25(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert records with text content for BM25
    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "v1", "values": [0.1, 0.2, 0.3], "text": "the quick brown fox"},
                {"id": "v2", "values": [0.4, 0.5, 0.6], "text": "lazy dog"},
                {"id": "v3", "values": [0.7, 0.8, 0.9], "text": "fox hunting in autumn"},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/query/hybrid",
            server.base_url
        ))
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "text": "fox",
            "topK": 3,
            "alpha": 0.5,
            "rerank": { "query": "fox", "topN": 2, "rankField": "text" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"].as_array().unwrap().len(), 2);

    common::cleanup_index(&server, &name).await;
}

// ─── Edge cases ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_rerank_top_n_larger_than_results_returns_all() {
    // topN > actual number of records → returns however many exist.
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "v1", "values": [0.1, 0.2, 0.3], "metadata": {"text": "only one"}}
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "topK": 10,
            "rerank": { "query": "one", "topN": 10 }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"].as_array().unwrap().len(), 1);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn test_rerank_empty_index_returns_empty_matches() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "topK": 5,
            "rerank": { "query": "anything" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"].as_array().unwrap().len(), 0);

    common::cleanup_index(&server, &name).await;
}
