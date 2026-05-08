//! Integration tests for `/v1/collections/:name/search` and the Plan AST.
//!
//! Two sections:
//!   - Tests that need no external services (vector-driven, default).
//!   - Tests gated on `ONECORTEX_VECTOR_EMBED_OPENAI_API_KEY` (text-driven).

mod common;

use reqwest::Client;
use serde_json::json;

const OPENAI_KEY_VAR: &str = "ONECORTEX_VECTOR_EMBED_OPENAI_API_KEY";
const OPENAI_DIM: i32 = 1536;

fn openai_configured() -> bool {
    let _ = dotenvy::dotenv();
    std::env::var(OPENAI_KEY_VAR)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

async fn upsert_three_vectors(server: &common::TestServer, name: &str) {
    let client = Client::new();
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "a", "values": [1.0, 0.0, 0.0], "metadata": {"topic": "x"}},
                {"id": "b", "values": [0.0, 1.0, 0.0], "metadata": {"topic": "y"}},
                {"id": "c", "values": [0.0, 0.0, 1.0], "metadata": {"topic": "x"}},
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "{}", resp.text().await.unwrap());
}

// ── No external services ──────────────────────────────────────────────────

#[tokio::test]
async fn search_with_vector_returns_matches() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    upsert_three_vectors(&server, &name).await;

    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections/{name}/search", server.base_url))
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 3,
            "includeMetadata": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0]["id"], "a"); // identical vector ⇒ top match

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn search_with_filter_honored() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    upsert_three_vectors(&server, &name).await;

    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections/{name}/search", server.base_url))
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 5,
            "filter": {"topic": {"$eq": "x"}},
            "includeMetadata": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2); // a, c — b is filtered out
    for m in matches {
        assert_eq!(m["metadata"]["topic"], "x");
    }

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn search_with_group_by_returns_grouped_shape() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    upsert_three_vectors(&server, &name).await;

    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections/{name}/search", server.base_url))
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 5,
            "includeMetadata": true,
            "groupBy": {"field": "topic", "limit": 5, "groupSize": 5},
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["grouped"], true);
    let groups = body["groups"].as_array().unwrap();
    // Two distinct topics: x (a, c) and y (b).
    assert_eq!(groups.len(), 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn search_explain_returns_plan_without_executing() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    upsert_three_vectors(&server, &name).await;

    let client = Client::new();
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/search?explain=true",
            server.base_url
        ))
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "topK": 3,
            "scoreThreshold": 0.5,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("matches").is_none(),
        "explain must not execute: {body}"
    );
    assert_eq!(body["plan"]["source"]["kind"], "dense");
    assert_eq!(body["plan"]["topK"], 3);
    let stages = body["plan"]["stages"].as_array().unwrap();
    assert!(stages.iter().any(|s| s["kind"] == "scoreThreshold"));

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn search_hybrid_block_on_bm25_disabled_collection_rejected() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections/{name}/search", server.base_url))
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "text": "foo",
            "hybrid": {"alpha": 0.5},
            "topK": 3,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "HYBRID_REQUIRES_BM25");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn search_hybrid_with_alpha_routes_through_hybrid_path() {
    // Asserts the explain output reflects the Hybrid source — the actual
    // ranking semantics are covered by tests/hybrid_test.rs.
    let server = common::start_test_server().await;
    let name = common::create_test_index_with_bm25(&server, 3, "cosine").await;
    upsert_three_vectors(&server, &name).await;

    let client = Client::new();
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/search?explain=true",
            server.base_url
        ))
        .json(&json!({
            "vector": [1.0, 0.0, 0.0],
            "text": "foo",
            "hybrid": {"alpha": 0.7},
            "topK": 3,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["plan"]["source"]["kind"], "hybrid");
    let alpha = body["plan"]["source"]["alpha"].as_f64().unwrap();
    assert!((alpha - 0.7).abs() < 1e-6);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn new_collection_defaults_bm25_enabled_true() {
    // v0.3 behavioural change: bm25Enabled now defaults to true. Existing
    // tests that explicitly want bm25=false use `create_test_index` (which
    // sends `bm25Enabled: false`); this test creates a vanilla collection
    // with no override and verifies the new default.
    let server = common::start_test_server().await;
    let name = format!("dflt-{}", uuid::Uuid::new_v4().simple());
    let name = name[..name.len().min(45)].to_string();

    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections", server.base_url))
        .json(&json!({
            "name": name,
            "dimension": 3,
            "metric": "cosine",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["bm25Enabled"], true);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn search_no_inputs_rejected() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;

    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections/{name}/search", server.base_url))
        .json(&json!({"topK": 3}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &name).await;
}

// ── Require OpenAI key (text-driven paths) ────────────────────────────────

async fn create_collection_with_openai_embedder(server: &common::TestServer, bm25: bool) -> String {
    let name = format!("emb-{}", uuid::Uuid::new_v4().simple());
    let name = name[..name.len().min(45)].to_string();
    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections", server.base_url))
        .json(&json!({
            "name": name,
            "dimension": OPENAI_DIM,
            "metric": "cosine",
            "bm25Enabled": bm25,
            "embedder": {
                "backend": "openai",
                "model": "text-embedding-3-small",
                "inputType": "document",
            }
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    assert!(
        status.is_success(),
        "create failed (status={status}): {body}"
    );
    name
}

#[tokio::test]
async fn search_with_text_only_on_dense_collection() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    let name = create_collection_with_openai_embedder(&server, false).await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "key", "text": "How to rotate an API key"},
                {"id": "bill", "text": "Billing FAQ"},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/search", server.base_url))
        .json(&json!({
            "text": "I forgot my key, how do I rotate it?",
            "topK": 2,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches[0]["id"], "key");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn search_text_on_bm25_collection_auto_detects_hybrid() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    let name = create_collection_with_openai_embedder(&server, true).await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "key", "text": "How to rotate an API key"},
                {"id": "bill", "text": "Billing FAQ"},
            ]
        }))
        .send()
        .await
        .unwrap();

    // explain to confirm hybrid was selected.
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/search?explain=true",
            server.base_url
        ))
        .json(&json!({"text": "rotate api key", "topK": 2}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["plan"]["source"]["kind"], "hybrid");

    // Real run.
    let resp = client
        .post(format!("{}/v1/collections/{name}/search", server.base_url))
        .json(&json!({"text": "rotate api key", "topK": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches[0]["id"], "key");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn search_hybrid_false_forces_dense_on_bm25_collection() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    let name = create_collection_with_openai_embedder(&server, true).await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/search?explain=true",
            server.base_url
        ))
        .json(&json!({
            "text": "rotate api key",
            "hybrid": false,
            "topK": 2
        }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["plan"]["source"]["kind"], "dense");

    common::cleanup_index(&server, &name).await;
}
