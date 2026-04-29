mod common;

use common::start_test_server;

#[tokio::test]
async fn health_response_carries_request_id_header() {
    let server = start_test_server().await;
    let resp = reqwest::get(format!("{}/health", server.base_url))
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let id = resp
        .headers()
        .get("x-request-id")
        .expect("X-Request-Id missing on response")
        .to_str()
        .unwrap()
        .to_string();
    // Server-assigned IDs are uuid v4 (36 chars with dashes).
    assert_eq!(id.len(), 36, "expected uuid-shaped id, got: {id}");
    assert_eq!(id.matches('-').count(), 4);
}

#[tokio::test]
async fn client_supplied_request_id_is_echoed() {
    let server = start_test_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/health", server.base_url))
        .header("X-Request-Id", "client-supplied-abc-123")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let id = resp
        .headers()
        .get("x-request-id")
        .expect("X-Request-Id missing on response")
        .to_str()
        .unwrap();
    assert_eq!(id, "client-supplied-abc-123");
}
