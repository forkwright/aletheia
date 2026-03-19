#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;

/// Helper: build a POST send-message request with an optional Idempotency-Key header.
fn send_message_req(session_id: &str, idempotency_key: Option<&str>) -> Request<Body> {
    let token = default_token();
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{session_id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    if let Some(key) = idempotency_key {
        builder = builder.header("idempotency-key", key);
    }
    builder
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({ "content": "Hello!" })).unwrap(),
        ))
        .unwrap()
}

#[tokio::test]
async fn idempotency_key_absent_works_normally() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = send_message_req(id, None);
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn idempotency_key_first_request_succeeds() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = send_message_req(id, Some("unique-key-001"));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("event: message_complete"),
        "first request should stream events normally"
    );
}

#[tokio::test]
async fn idempotency_key_replay_returns_cached_completion() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req1 = send_message_req(id, Some("replay-key-001"));
    let resp1 = router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let body1 = body_string(resp1).await;

    let original_stop_reason = body1
        .lines()
        .find(|l| l.starts_with("data:"))
        .and_then(|l| serde_json::from_str::<serde_json::Value>(l.trim_start_matches("data:")).ok())
        .and_then(|v| v["stop_reason"].as_str().map(str::to_owned))
        .unwrap_or_default();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let req2 = send_message_req(id, Some("replay-key-001"));
    let resp2 = router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = body_string(resp2).await;
    assert!(
        body2.contains("message_complete"),
        "replayed response should contain message_complete event, got: {body2}"
    );
    if !original_stop_reason.is_empty() {
        assert!(
            body2.contains(&original_stop_reason),
            "replayed stop_reason should match original '{original_stop_reason}', got: {body2}"
        );
    }
}

#[tokio::test]
async fn idempotency_key_in_flight_returns_409() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let key = "inflight-key-001";
    state.idempotency_cache.check_or_insert(key);

    let req = send_message_req(id, Some(key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn idempotency_key_too_long_returns_400() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let long_key = "x".repeat(65);
    let req = send_message_req(id, Some(&long_key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn idempotency_key_empty_returns_400() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = send_message_req(id, Some(""));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn idempotency_key_64_chars_accepted() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let key = "a".repeat(64);
    let req = send_message_req(id, Some(&key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
