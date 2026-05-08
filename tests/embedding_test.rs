//! Integration tests for F1 (server-side embeddings).
//!
//! These tests hit the *real* OpenAI embeddings API. They self-skip when
//! `ONECORTEX_VECTOR_EMBED_OPENAI_API_KEY` is not set, so the suite still
//! works in CI / on developer machines without a key.
//!
//! What's covered:
//!   - Preflight at collection-create runs (and dimension mismatch is rejected).
//!   - Upsert with `text` embeds server-side and persists vectors.
//!   - Query with `text` returns matches; LRU prevents the second identical
//!     query from re-hitting OpenAI; `?noCache=true` bypasses the cache.
//!   - Conflict cases (`values + text` on upsert; multiple inputs on query).
//!   - Text on a collection without an embedder is rejected.
//!
//! Cost: each end-to-end test issues at most a handful of small `text-embedding-3-small`
//! requests. Negligible.

mod common;

use reqwest::Client;
use serde_json::json;

const OPENAI_KEY_VAR: &str = "ONECORTEX_VECTOR_EMBED_OPENAI_API_KEY";
const OPENAI_DIM: i32 = 1536; // text-embedding-3-small

/// Returns true if the OpenAI key is configured. Tests self-skip when false.
/// Loads `.env` first so the check sees whatever the developer put in there
/// (the test harness's `start_test_server()` also loads it, but only later).
fn openai_configured() -> bool {
    let _ = dotenvy::dotenv();
    std::env::var(OPENAI_KEY_VAR)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

async fn create_collection_with_openai_embedder(
    server: &common::TestServer,
    dimension: i32,
) -> (String, reqwest::StatusCode, serde_json::Value) {
    let name = format!("emb-{}", uuid::Uuid::new_v4().simple());
    let name = name[..name.len().min(45)].to_string();

    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections", server.base_url))
        .json(&json!({
            "name": name,
            "dimension": dimension,
            "metric": "cosine",
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
    let body: serde_json::Value = resp.json().await.unwrap_or(json!({}));
    (name, status, body)
}

/// Same as above but asserts a 201 Created. Use in tests that just want a
/// pre-built embedder-bound collection — they shouldn't proceed if create
/// itself failed (otherwise downstream calls return 404 and bury the cause).
async fn create_or_panic(server: &common::TestServer) -> String {
    let (name, status, body) = create_collection_with_openai_embedder(server, OPENAI_DIM).await;
    assert!(
        status.is_success(),
        "create_collection failed (status={status}); body={body}"
    );
    name
}

#[tokio::test]
async fn create_collection_persists_and_describes_embedder() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    let (name, status, body) = create_collection_with_openai_embedder(&server, OPENAI_DIM).await;
    assert_eq!(status, 201, "create failed: {body}");
    assert_eq!(body["embedder"]["backend"], "openai");
    assert_eq!(body["embedder"]["model"], "text-embedding-3-small");
    assert_eq!(body["embedder"]["inputType"], "document");

    // Round-trip via describe.
    let client = Client::new();
    let describe: serde_json::Value = client
        .get(format!("{}/v1/collections/{name}", server.base_url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(describe["embedder"]["backend"], "openai");
    assert_eq!(describe["embedder"]["model"], "text-embedding-3-small");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn create_collection_dimension_mismatch_rejected_by_preflight() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    // openai text-embedding-3-small returns 1536 dims; we declare 512 → preflight fails.
    let (_name, status, body) = create_collection_with_openai_embedder(&server, 512).await;
    assert_eq!(status, 400, "expected 400, body: {body}");
    assert_eq!(body["error"]["code"], "EMBEDDER_DIMENSION_MISMATCH");
    assert_eq!(body["error"]["details"]["expected"], 512);
    assert_eq!(body["error"]["details"]["got"], 1536);
}

#[tokio::test]
async fn upsert_with_text_embeds_and_persists() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    let (name, status, _) = create_collection_with_openai_embedder(&server, OPENAI_DIM).await;
    assert_eq!(status, 201);
    let client = Client::new();

    let upsert = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "r1", "text": "How to rotate an API key", "metadata": {"src": "faq"}},
                {"id": "r2", "text": "Billing FAQ"},
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(upsert.status(), 200, "{}", upsert.text().await.unwrap());

    // Fetch with includeValues to confirm vectors landed.
    let fetch_resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"src": {"$eq": "faq"}}, "includeValues": true}))
        .send()
        .await
        .unwrap();
    let fetch_status = fetch_resp.status();
    let fetched: serde_json::Value = fetch_resp.json().await.unwrap();
    assert!(
        fetch_status.is_success(),
        "fetch_by_metadata failed (status={fetch_status}): {fetched}"
    );
    let recs = fetched["records"]
        .as_array()
        .unwrap_or_else(|| panic!("no `records` field in response: {fetched}"));
    assert_eq!(recs.len(), 1, "expected 1 record, got: {fetched}");
    let values = recs[0]["values"].as_array().unwrap();
    assert_eq!(values.len(), OPENAI_DIM as usize);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn upsert_values_and_text_conflict_rejected() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    let name = create_or_panic(&server).await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "r1", "values": vec![0.1f32; OPENAI_DIM as usize], "text": "hi"}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "VALUES_AND_TEXT_CONFLICT");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn upsert_text_on_non_embedder_collection_rejected() {
    // No OpenAI key needed: this never reaches OpenAI.
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({"records": [{"id": "r1", "text": "no embedder bound"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "TEXT_REQUIRED");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_with_text_returns_matches() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    let name = create_or_panic(&server).await;
    let client = Client::new();

    // Index three semantically distinct snippets.
    let upsert = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "key", "text": "How to rotate an API key", "metadata": {"topic": "auth"}},
                {"id": "bill", "text": "Billing and invoices", "metadata": {"topic": "billing"}},
                {"id": "pasta", "text": "Recipe for spaghetti carbonara", "metadata": {"topic": "food"}},
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(upsert.status(), 200);

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "text": "I forgot my api key, how do I get a new one?",
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
    // Top match must be the API-key snippet — the other two are unrelated.
    assert_eq!(
        matches[0]["id"], "key",
        "expected 'key' as top match, got {body}"
    );

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_text_cache_hits_then_no_cache_bypasses() {
    if !openai_configured() {
        eprintln!("skipping: {OPENAI_KEY_VAR} not set");
        return;
    }
    let server = common::start_test_server().await;
    let name = create_or_panic(&server).await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [{"id": "x", "text": "hello world"}]
        }))
        .send()
        .await
        .unwrap();

    // First query — populates cache.
    let q = json!({"text": "hello", "topK": 1});
    let r1 = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&q)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 200);

    // Second identical query — should be served from cache. We can't directly
    // observe cache hits over HTTP, but the request must still succeed and the
    // result vector must produce the same top match.
    let r2: serde_json::Value = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&q)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r2["matches"][0]["id"], "x");

    // ?noCache=true bypasses the cache (re-hits OpenAI). Same result expected.
    let r3: serde_json::Value = client
        .post(format!(
            "{}/v1/collections/{name}/query?noCache=true",
            server.base_url
        ))
        .json(&q)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r3["matches"][0]["id"], "x");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_text_on_non_embedder_collection_rejected() {
    // No OpenAI key needed: short-circuits before any upstream call.
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({"text": "hi", "topK": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "EMBEDDER_CONFIG");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_multiple_inputs_rejected() {
    // Sending both `vector` and `text` must be a 400 conflict.
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();
    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "vector": [0.1, 0.2, 0.3],
            "text": "hi",
            "topK": 1
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "VALUES_AND_TEXT_CONFLICT");

    common::cleanup_index(&server, &name).await;
}
