mod common;

use reqwest::Client;

#[tokio::test]
async fn no_key_returns_401() {
    let server = common::start_test_server().await;
    let client = Client::new();
    let resp = client.get(format!("{}/indexes", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "UNAUTHENTICATED");
}

#[tokio::test]
async fn wrong_key_returns_401() {
    let server = common::start_test_server().await;
    let client = Client::new();
    let resp = client.get(format!("{}/indexes", server.base_url))
        .header("Api-Key", "totally-wrong-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "UNAUTHENTICATED");
}

#[tokio::test]
async fn valid_key_passes() {
    let server = common::start_test_server().await;
    let client = Client::new();
    let resp = client.get(format!("{}/indexes", server.base_url))
        .header("Api-Key", &server.api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn health_no_auth_needed() {
    let server = common::start_test_server().await;
    let client = Client::new();
    let resp = client.get(format!("{}/health", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn ready_no_auth_needed() {
    let server = common::start_test_server().await;
    let client = Client::new();
    let resp = client.get(format!("{}/ready", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
