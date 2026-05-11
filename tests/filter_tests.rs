mod common;

use reqwest::Client;
use serde_json::json;

// ── helpers ──────────────────────────────────────────────────────────────────

async fn fetch_by_metadata(
    client: &Client,
    base: &str,
    name: &str,
    filter: serde_json::Value,
) -> (u16, serde_json::Value) {
    let resp = client
        .post(format!(
            "{base}/v1/collections/{name}/records/fetch_by_metadata"
        ))
        .json(&json!({ "filter": filter }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp.json().await.unwrap();
    (status, body)
}

fn record_ids(body: &serde_json::Value) -> Vec<String> {
    body["records"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

// ── $in ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn in_filter_returns_matching_records() {
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
                {"id":"v1","values":[1.0,0.0,0.0],"metadata":{"tag":"a"}},
                {"id":"v2","values":[0.0,1.0,0.0],"metadata":{"tag":"b"}},
                {"id":"v3","values":[0.0,0.0,1.0],"metadata":{"tag":"c"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"tag": {"$in": ["a", "b"]}}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    let ids: std::collections::HashSet<_> = record_ids(&body).into_iter().collect();
    assert_eq!(ids, ["v1", "v2"].iter().map(|s| s.to_string()).collect());

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn in_filter_with_injection_string_is_literal() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let injection = "a'; DROP TABLE r --";
    client
        .post(format!(
            "{}/v1/collections/{name}/records/upsert",
            server.base_url
        ))
        .json(&json!({
            "records": [
                {"id":"v1","values":[1.0,0.0,0.0],"metadata":{"tag": injection}},
                {"id":"v2","values":[0.0,1.0,0.0],"metadata":{"tag":"safe"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"tag": {"$in": [injection]}}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    let ids = record_ids(&body);
    assert_eq!(ids, vec!["v1"]);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn nin_filter_excludes_matching_records() {
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
                {"id":"v1","values":[1.0,0.0,0.0],"metadata":{"tag":"a"}},
                {"id":"v2","values":[0.0,1.0,0.0],"metadata":{"tag":"b"}},
                {"id":"v3","values":[0.0,0.0,1.0],"metadata":{"tag":"c"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"tag": {"$nin": ["a", "b"]}}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    let ids = record_ids(&body);
    assert_eq!(ids, vec!["v3"]);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn in_empty_array_returns_filter_malformed() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"tag": {"$in": []}}),
    )
    .await;

    assert_eq!(status, 400, "body: {body}");
    assert_eq!(body["error"]["code"], "FILTER_MALFORMED");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn nin_empty_array_returns_filter_malformed() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"tag": {"$nin": []}}),
    )
    .await;

    assert_eq!(status, 400, "body: {body}");
    assert_eq!(body["error"]["code"], "FILTER_MALFORMED");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn in_combined_with_eq_in_and() {
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
                {"id":"v1","values":[1.0,0.0,0.0],"metadata":{"tag":"a","score":"high"}},
                {"id":"v2","values":[0.0,1.0,0.0],"metadata":{"tag":"a","score":"low"}},
                {"id":"v3","values":[0.0,0.0,1.0],"metadata":{"tag":"b","score":"high"}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"$and": [{"tag": {"$in": ["a"]}}, {"score": {"$eq": "high"}}]}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    let ids = record_ids(&body);
    assert_eq!(ids, vec!["v1"]);

    common::cleanup_index(&server, &name).await;
}

// ── $exists ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn exists_true_returns_records_with_field() {
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
                {"id":"v1","values":[1.0,0.0,0.0],"metadata":{"source":"web"}},
                {"id":"v2","values":[0.0,1.0,0.0],"metadata":{}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"source": {"$exists": true}}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    let ids = record_ids(&body);
    assert!(ids.contains(&"v1".to_string()), "v1 should be returned");
    assert!(
        !ids.contains(&"v2".to_string()),
        "v2 should not be returned"
    );

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn exists_false_returns_records_without_field() {
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
                {"id":"v1","values":[1.0,0.0,0.0],"metadata":{"source":"web"}},
                {"id":"v2","values":[0.0,1.0,0.0],"metadata":{}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"source": {"$exists": false}}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    let ids = record_ids(&body);
    assert!(ids.contains(&"v2".to_string()), "v2 should be returned");
    assert!(
        !ids.contains(&"v1".to_string()),
        "v1 should not be returned"
    );

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn exists_nested_field() {
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
                {"id":"v1","values":[1.0,0.0,0.0],"metadata":{"meta":{"origin":"pdf"}}},
                {"id":"v2","values":[0.0,1.0,0.0],"metadata":{"meta":{}}},
            ]
        }))
        .send()
        .await
        .unwrap();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"meta.origin": {"$exists": true}}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    let ids = record_ids(&body);
    assert!(ids.contains(&"v1".to_string()), "v1 should be returned");
    assert!(
        !ids.contains(&"v2".to_string()),
        "v2 should not be returned"
    );

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn exists_non_bool_returns_filter_malformed() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let (status, body) = fetch_by_metadata(
        &client,
        &server.base_url,
        &name,
        json!({"source": {"$exists": "yes"}}),
    )
    .await;

    assert_eq!(status, 400, "body: {body}");
    assert_eq!(body["error"]["code"], "FILTER_MALFORMED");

    common::cleanup_index(&server, &name).await;
}
