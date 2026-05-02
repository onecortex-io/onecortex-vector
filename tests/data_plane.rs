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
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
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
        .post(format!(
            "{}/v1/collections/{name}/records/fetch",
            server.base_url
        ))
        .json(&json!({ "ids": ["v1", "v2"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    let v1 = records.iter().find(|r| r["id"] == "v1").unwrap();
    let v2 = records.iter().find(|r| r["id"] == "v2").unwrap();
    assert!(v1.is_object());
    assert!(v2.is_object());
    assert_eq!(v1["metadata"]["color"], "red");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn upsert_batch_too_large() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let records: Vec<serde_json::Value> = (0..1001)
        .map(|i| json!({"id": format!("v{i}"), "values": [1.0, 0.0, 0.0]}))
        .collect();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({ "records": records }))
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
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [{"id": "v1", "values": [1.0, 0.0]}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn upsert_sparse_values_rejected() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // sparseValues are not supported and should be rejected with a typed code.
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [{
                "id": "v1",
                "values": [1.0, 0.0, 0.0],
                "sparseValues": {"indices": [0, 1], "values": [0.5, 0.3]}
            }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "SPARSE_NOT_SUPPORTED");
    assert_eq!(body["error"]["details"]["recordId"], "v1");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn query_with_mismatched_vector_dimension_rejected() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .post(format!("{}/v1/collections/{name}/query", server.base_url))
        .json(&json!({
            "vector": [1.0, 0.0, 0.0, 0.0, 0.0],
            "topK": 5
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "DIMENSION_MISMATCH");
    assert_eq!(body["error"]["details"]["expected"], 3);
    assert_eq!(body["error"]["details"]["got"], 5);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn upsert_with_mismatched_vector_dimension_rejected() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [{ "id": "x", "values": [0.1, 0.2] }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "DIMENSION_MISMATCH");
    assert_eq!(body["error"]["details"]["recordId"], "x");
    assert_eq!(body["error"]["details"]["expected"], 3);
    assert_eq!(body["error"]["details"]["got"], 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn fetch_by_metadata_eq() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert records with metadata
    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"category": "news"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"category": "sports"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({ "filter": {"category": {"$eq": "news"}} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "v1");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn fetch_by_metadata_in() {
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
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({ "filter": {"tag": {"$in": ["a", "c"]}} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn delete_by_ids() {
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
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    // Delete v1
    client
        .post(format!(
            "{}/v1/collections/{name}/records/delete",
            server.base_url
        ))
        .json(&json!({ "ids": ["v1"] }))
        .send()
        .await
        .unwrap();

    // Fetch -- v1 should be gone
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch",
            server.base_url
        ))
        .json(&json!({ "ids": ["v1", "v2"] }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert!(!records.iter().any(|r| r["id"] == "v1"));
    assert!(records.iter().any(|r| r["id"] == "v2"));

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn delete_by_filter() {
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
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"keep": "no"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"keep": "yes"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/delete",
            server.base_url
        ))
        .json(&json!({ "filter": {"keep": {"$eq": "no"}} }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch",
            server.base_url
        ))
        .json(&json!({ "ids": ["v1", "v2"] }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert!(!records.iter().any(|r| r["id"] == "v1"));
    assert!(records.iter().any(|r| r["id"] == "v2"));

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn delete_all() {
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
                {"id": "v1", "values": [1.0, 0.0, 0.0]},
                {"id": "v2", "values": [0.0, 1.0, 0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/delete",
            server.base_url
        ))
        .json(&json!({ "deleteAll": true }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch",
            server.base_url
        ))
        .json(&json!({ "ids": ["v1", "v2"] }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert!(records.is_empty());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn update_metadata() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [{"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"a": 1}}]
        }))
        .send()
        .await
        .unwrap();

    // Update: merge metadata
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/update",
            server.base_url
        ))
        .json(&json!({ "id": "v1", "setMetadata": {"b": 2} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Fetch and verify merge
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch",
            server.base_url
        ))
        .json(&json!({ "ids": ["v1"] }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    let v1 = records.iter().find(|r| r["id"] == "v1").unwrap();
    assert_eq!(v1["metadata"]["a"], 1);
    assert_eq!(v1["metadata"]["b"], 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn list_vectors_with_prefix() {
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
            "{}/v1/collections/{name}/records/list?prefix=doc-",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn list_vectors_pagination() {
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
            "{}/v1/collections/{name}/records/list?limit=2",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    let next = body["pagination"]["next"].as_str().unwrap();

    // Second page
    let resp = client
        .get(format!(
            "{}/v1/collections/{name}/records/list?limit=2&paginationToken={next}",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn scroll_returns_full_vector_data() {
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
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"color": "red"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"color": "blue"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/scroll",
            server.base_url
        ))
        .json(&json!({
            "includeValues": true,
            "includeMetadata": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    // Verify values and metadata are present
    assert!(records[0]["values"].is_array());
    assert!(records[0]["metadata"].is_object());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn scroll_pagination_with_cursor() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert 5 records with predictable IDs (alphabetical ordering matters for cursor)
    let records: Vec<serde_json::Value> = (1..=5)
        .map(|i| json!({"id": format!("v{i}"), "values": [1.0, 0.0, 0.0]}))
        .collect();
    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({"records": records}))
        .send()
        .await
        .unwrap();

    // Page 1: limit=2
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/scroll",
            server.base_url
        ))
        .json(&json!({"limit": 2}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["records"].as_array().unwrap().len(), 2);
    assert!(body["nextCursor"].is_string());

    // Page 2: use cursor
    let cursor = body["nextCursor"].as_str().unwrap();
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/scroll",
            server.base_url
        ))
        .json(&json!({"limit": 2, "cursor": cursor}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["records"].as_array().unwrap().len(), 2);

    // Page 3: last page — 1 record remains, no nextCursor
    let cursor = body["nextCursor"].as_str().unwrap();
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/scroll",
            server.base_url
        ))
        .json(&json!({"limit": 2, "cursor": cursor}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["records"].as_array().unwrap().len(), 1);
    assert!(body["nextCursor"].is_null() || body.get("nextCursor").is_none());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn scroll_with_filter() {
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
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"status": "active"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"status": "archived"}},
                {"id": "v3", "values": [0.0, 0.0, 1.0], "metadata": {"status": "active"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/scroll",
            server.base_url
        ))
        .json(&json!({
            "filter": {"status": {"$eq": "active"}},
            "includeMetadata": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    for r in records {
        assert_eq!(r["metadata"]["status"], "active");
    }

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn sample_returns_random_vectors() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let records: Vec<serde_json::Value> = (1..=20)
        .map(|i| json!({"id": format!("v{i}"), "values": [1.0, 0.0, 0.0]}))
        .collect();
    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({"records": records}))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/sample", server.base_url))
        .json(&json!({"size": 5, "includeMetadata": true}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert!(records.len() <= 5);
    assert!(!records.is_empty());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn sample_with_filter() {
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
                {"id": "v1", "values": [1.0, 0.0, 0.0], "metadata": {"type": "a"}},
                {"id": "v2", "values": [0.0, 1.0, 0.0], "metadata": {"type": "b"}},
                {"id": "v3", "values": [0.0, 0.0, 1.0], "metadata": {"type": "a"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/sample", server.base_url))
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
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    for r in records {
        assert_eq!(r["metadata"]["type"], "a");
    }

    common::cleanup_index(&server, &name).await;
}

// --- Advanced metadata filter integration tests ---

#[tokio::test]
async fn filter_gte_datetime() {
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
                {"id": "r1", "values": [1.0, 0.0, 0.0], "metadata": {"created_at": "2025-01-15T00:00:00Z"}},
                {"id": "r2", "values": [0.0, 1.0, 0.0], "metadata": {"created_at": "2025-07-01T00:00:00Z"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"created_at": {"$gte": "2025-06-01T00:00:00Z"}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "r2");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_lt_datetime() {
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
                {"id": "r1", "values": [1.0, 0.0, 0.0], "metadata": {"created_at": "2025-01-15T00:00:00Z"}},
                {"id": "r2", "values": [0.0, 1.0, 0.0], "metadata": {"created_at": "2025-07-01T00:00:00Z"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"created_at": {"$lt": "2025-06-01T00:00:00Z"}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "r1");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_geo_radius() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // New York City and Los Angeles
    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "nyc", "values": [1.0, 0.0, 0.0], "metadata": {"location": {"lat": 40.7128, "lon": -74.0060}}},
                {"id": "la",  "values": [0.0, 1.0, 0.0], "metadata": {"location": {"lat": 34.0522, "lon": -118.2437}}},
            ]
        }))
        .send()
        .await
        .unwrap();

    // 1 km radius around NYC — should only match nyc
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({
            "filter": {
                "location": {"$geoRadius": {"lat": 40.7128, "lon": -74.0060, "radiusMeters": 1000.0}}
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "nyc");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_geo_bbox() {
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
                {"id": "nyc", "values": [1.0, 0.0, 0.0], "metadata": {"location": {"lat": 40.7128, "lon": -74.0060}}},
                {"id": "la",  "values": [0.0, 1.0, 0.0], "metadata": {"location": {"lat": 34.0522, "lon": -118.2437}}},
            ]
        }))
        .send()
        .await
        .unwrap();

    // Bounding box covering the NYC metro area only
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({
            "filter": {
                "location": {"$geoBBox": {
                    "minLat": 40.0, "maxLat": 41.5,
                    "minLon": -75.0, "maxLon": -73.0
                }}
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "nyc");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_elem_match_hit() {
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
                {"id": "r1", "values": [1.0, 0.0, 0.0], "metadata": {"tags": [{"type": "premium"}, {"type": "basic"}]}},
                {"id": "r2", "values": [0.0, 1.0, 0.0], "metadata": {"tags": [{"type": "basic"}]}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"tags": {"$elemMatch": {"type": "premium"}}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "r1");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_elem_match_no_results() {
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
                {"id": "r1", "values": [1.0, 0.0, 0.0], "metadata": {"tags": [{"type": "premium"}]}},
                {"id": "r2", "values": [0.0, 1.0, 0.0], "metadata": {"tags": [{"type": "basic"}]}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"tags": {"$elemMatch": {"type": "enterprise"}}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 0);

    common::cleanup_index(&server, &name).await;
}

// --- $contains / $containsAny / $containsAll (arrays of scalars) ---

async fn seed_array_metadata(server: &common::TestServer) -> String {
    let name = common::create_test_index(server, 3, "cosine").await;
    let client = Client::new();
    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "arxiv", "values": [1.0, 0.0, 0.0],
                 "metadata": {"authors": ["Lewis", "Perez"], "year": 2024}},
                {"id": "blog-eval", "values": [0.0, 1.0, 0.0],
                 "metadata": {"authors": ["Cortex Team"], "year": 2025}},
                {"id": "paper", "values": [0.0, 0.0, 1.0],
                 "metadata": {"authors": ["Smith", "Johnson"], "year": 2024}},
                {"id": "scalar-author", "values": [0.5, 0.5, 0.0],
                 "metadata": {"authors": "Cortex Team", "year": 2025}},
            ]
        }))
        .send()
        .await
        .unwrap();
    name
}

#[tokio::test]
async fn filter_contains_hit() {
    let server = common::start_test_server().await;
    let name = seed_array_metadata(&server).await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"authors": {"$contains": "Cortex Team"}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    let ids: Vec<&str> = records.iter().map(|r| r["id"].as_str().unwrap()).collect();
    // Only the array-valued metadata matches; scalar-valued "authors": "Cortex Team" must NOT.
    assert_eq!(ids, vec!["blog-eval"]);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_contains_no_results() {
    let server = common::start_test_server().await;
    let name = seed_array_metadata(&server).await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"authors": {"$contains": "Nobody"}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["records"].as_array().unwrap().len(), 0);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_contains_any_hit() {
    let server = common::start_test_server().await;
    let name = seed_array_metadata(&server).await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"authors": {"$containsAny": ["Cortex Team", "Lewis"]}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let mut ids: Vec<String> = body["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["arxiv".to_string(), "blog-eval".to_string()]);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_contains_all_hit() {
    let server = common::start_test_server().await;
    let name = seed_array_metadata(&server).await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"authors": {"$containsAll": ["Smith", "Johnson"]}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], "paper");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn filter_contains_any_empty_rejected() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch_by_metadata",
            server.base_url
        ))
        .json(&json!({"filter": {"tags": {"$containsAny": []}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "FILTER_MALFORMED");

    common::cleanup_index(&server, &name).await;
}

/// Regression: a batch with the same id repeated multiple times must succeed
/// (200) instead of triggering Postgres "ON CONFLICT DO UPDATE command cannot
/// affect row a second time". The last occurrence wins (last-write-wins) and
/// `upsertedCount` reports the number of distinct ids.
#[tokio::test]
async fn upsert_dedupes_duplicate_ids_within_batch() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // 5 records, but only 3 distinct ids: r1 appears 3x with different values.
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id": "r1", "values": [1.0, 0.0, 0.0], "metadata": {"v": "first"}},
                {"id": "r2", "values": [0.0, 1.0, 0.0], "metadata": {"v": "r2"}},
                {"id": "r1", "values": [0.5, 0.5, 0.0], "metadata": {"v": "second"}},
                {"id": "r3", "values": [0.0, 0.0, 1.0], "metadata": {"v": "r3"}},
                {"id": "r1", "values": [0.1, 0.2, 0.3], "metadata": {"v": "last"}},
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["upsertedCount"], 3, "expected 3 distinct ids");

    // Fetch r1: must reflect the LAST occurrence in the batch.
    let resp = client
        .post(format!(
            "{}/v1/collections/{name}/records/fetch",
            server.base_url
        ))
        .json(&json!({ "ids": ["r1", "r2", "r3"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), 3, "all 3 distinct ids must be present");

    let r1 = records.iter().find(|r| r["id"] == "r1").unwrap();
    assert_eq!(
        r1["metadata"]["v"], "last",
        "r1 must reflect the LAST occurrence (last-write-wins)"
    );

    common::cleanup_index(&server, &name).await;
}
