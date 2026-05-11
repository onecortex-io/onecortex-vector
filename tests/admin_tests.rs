mod common;

use reqwest::Client;
use serde_json::json;

#[tokio::test]
async fn vacuum_returns_200_ok() {
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
                {"id":"v1","values":[1.0,0.0,0.0]},
                {"id":"v2","values":[0.0,1.0,0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/vacuum", server.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["collection"], name);

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn reindex_returns_202_accepted() {
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
                {"id":"v1","values":[1.0,0.0,0.0]},
                {"id":"v2","values":[0.0,1.0,0.0]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/reindex", server.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 202);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "reindexing");
    assert_eq!(body["collection"], name);
    assert!(body["message"].as_str().unwrap().contains("background"));

    common::cleanup_index(&server, &name).await;
}

#[tokio::test]
async fn vacuum_unknown_collection_returns_404() {
    let server = common::start_test_server().await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/no-such-collection-xyz/vacuum",
            server.base_url
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "COLLECTION_NOT_FOUND");
}

#[tokio::test]
async fn reindex_unknown_collection_returns_404() {
    let server = common::start_test_server().await;
    let client = Client::new();

    let resp = client
        .post(format!(
            "{}/v1/collections/no-such-collection-xyz/reindex",
            server.base_url
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "COLLECTION_NOT_FOUND");
}

#[tokio::test]
async fn reindex_rebuilds_diskann_index_in_db() {
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
                {"id":"v1","values":[1.0,0.0,0.0]},
                {"id":"v2","values":[0.0,1.0,0.0]},
                {"id":"v3","values":[0.0,0.0,1.0]},
                {"id":"v4","values":[0.5,0.5,0.0]},
                {"id":"v5","values":[0.0,0.5,0.5]},
            ]
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{}/v1/collections/{name}/reindex", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202);

    // Allow the background task time to finish
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    let row: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM pg_indexes WHERE indexname LIKE $1)")
            .bind("%_diskann_idx")
            .fetch_one(&server.pool)
            .await
            .unwrap();

    assert!(row.0, "DiskANN index should exist after reindex");

    common::cleanup_index(&server, &name).await;
}
