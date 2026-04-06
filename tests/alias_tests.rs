mod common;

use serde_json::json;

#[tokio::test]
async fn create_and_query_via_alias() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    // Upsert a record into the real collection
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

    // Create alias "prod" -> collection
    let resp = client
        .post(format!("{}/aliases", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"alias": "prod", "collectionName": collection}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Query via alias — should work transparently
    let resp = client
        .post(format!("{}/collections/prod/query", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"vector": [1.0, 0.0, 0.0], "topK": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["matches"][0]["id"], "v1");

    // Cleanup
    client
        .delete(format!("{}/aliases/prod", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn list_aliases() {
    let server = common::start_test_server().await;
    let collection = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    // Create two aliases
    client
        .post(format!("{}/aliases", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"alias": "alias-a", "collectionName": collection}))
        .send()
        .await
        .unwrap();
    client
        .post(format!("{}/aliases", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"alias": "alias-b", "collectionName": collection}))
        .send()
        .await
        .unwrap();

    let resp = client
        .get(format!("{}/aliases", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let aliases = body["aliases"].as_array().unwrap();
    assert!(aliases.len() >= 2);

    // Cleanup
    client
        .delete(format!("{}/aliases/alias-a", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    client
        .delete(format!("{}/aliases/alias-b", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    common::cleanup_index(&server, &collection).await;
}

#[tokio::test]
async fn update_alias_target() {
    let server = common::start_test_server().await;
    let collection1 = common::create_test_index(&server, 3, "cosine").await;
    let collection2 = common::create_test_index(&server, 3, "cosine").await;
    let client = reqwest::Client::new();

    // Create alias pointing to collection1
    client
        .post(format!("{}/aliases", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"alias": "swap-test", "collectionName": collection1}))
        .send()
        .await
        .unwrap();

    // Update alias to point to collection2 (upsert semantics via POST)
    let resp = client
        .post(format!("{}/aliases", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"alias": "swap-test", "collectionName": collection2}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Describe alias — should now point to collection2
    let resp = client
        .get(format!("{}/aliases/swap-test", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["collectionName"], collection2);

    // Cleanup
    client
        .delete(format!("{}/aliases/swap-test", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    common::cleanup_index(&server, &collection1).await;
    common::cleanup_index(&server, &collection2).await;
}

#[tokio::test]
async fn delete_nonexistent_alias_returns_404() {
    let server = common::start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{}/aliases/nonexistent", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn alias_to_nonexistent_collection_returns_404() {
    let server = common::start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/aliases", server.base_url))
        .header("Api-Key", &server.api_key)
        .json(&json!({"alias": "bad", "collectionName": "nonexistent-col"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
