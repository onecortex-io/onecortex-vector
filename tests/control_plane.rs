mod common;

use reqwest::Client;
use serde_json::json;

fn client_with_key(api_key: &str) -> (Client, String) {
    (Client::new(), api_key.to_string())
}

#[tokio::test]
async fn create_index_success() {
    let server = common::start_test_server().await;
    let name = format!(
        "cp-create-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let client = Client::new();

    let resp = client
        .post(format!("{}/collections", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "name": name, "dimension": 3, "metric": "cosine" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], name);
    assert_eq!(body["dimension"], 3);
    assert_eq!(body["metric"], "cosine");
    assert_eq!(body["status"]["ready"], true);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn create_index_duplicate() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .post(format!("{}/collections", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "name": name, "dimension": 3, "metric": "cosine" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "ALREADY_EXISTS");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn create_index_bad_dimension() {
    let server = common::start_test_server().await;
    let client = Client::new();

    let resp = client
        .post(format!("{}/collections", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "name": "bad-dim-test", "dimension": 0, "metric": "cosine" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "INVALID_ARGUMENT");
}

#[tokio::test]
async fn create_index_bad_dimension_too_large() {
    let server = common::start_test_server().await;
    let client = Client::new();

    let resp = client
        .post(format!("{}/collections", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "name": "bad-dim-large", "dimension": 20001, "metric": "cosine" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn create_index_bad_metric() {
    let server = common::start_test_server().await;
    let client = Client::new();

    let resp = client
        .post(format!("{}/collections", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "name": "bad-metric", "dimension": 3, "metric": "hamming" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn list_indexes() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/collections", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let collections = body["collections"].as_array().unwrap();
    assert!(collections.iter().any(|i| i["name"] == name));

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn describe_index() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 5, "euclidean").await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/collections/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], name);
    assert_eq!(body["dimension"], 5);
    assert_eq!(body["metric"], "euclidean");

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn describe_index_not_found() {
    let server = common::start_test_server().await;
    let client = Client::new();

    let resp = client
        .get(format!("{}/collections/nonexistent-index", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn delete_index() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .delete(format!("{}/collections/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202);

    // Subsequent GET should return 404
    let resp = client
        .get(format!("{}/collections/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn configure_index() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    let resp = client
        .patch(format!("{}/collections/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "deletion_protection": "enabled" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    common::cleanup_index(&server, &name).await;
    // cleanup will fail silently because deletion protection is on, so force-clean via DB
    sqlx::query("DELETE FROM _onecortex_vector.collections WHERE name = $1")
        .bind(&name)
        .execute(&server.pool)
        .await
        .ok();
}

#[tokio::test]
async fn deletion_protection_blocks_delete() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Enable deletion protection
    client
        .patch(format!("{}/collections/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "deletion_protection": "enabled" }))
        .send()
        .await
        .unwrap();

    // Try to delete -- should get 403
    let resp = client
        .delete(format!("{}/collections/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "PERMISSION_DENIED");

    // Disable protection and cleanup
    client
        .patch(format!("{}/collections/{name}", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({ "deletion_protection": "disabled" }))
        .send()
        .await
        .unwrap();
    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn describe_index_stats() {
    let server = common::start_test_server().await;
    let name = common::create_test_index(&server, 3, "cosine").await;
    let client = Client::new();

    // Upsert some records first
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

    // Give stats update time to complete (async fire-and-forget)
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let resp = client
        .post(format!(
            "{}/collections/{name}/describe_collection_stats",
            server.base_url
        ))
        .header("Api-Key", &server.api_key)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["dimension"], 3);
    assert!(body["totalRecordCount"].as_i64().unwrap() >= 2);

    common::cleanup_index(&server, &name).await;
}
