#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sha2::{Digest as _, Sha256};
use tower::ServiceExt;

use super::helpers::*;

const HEX_HIGH_NIBBLE_SHIFT: u8 = 4;
const HEX_LOW_NIBBLE_MASK: u8 = 0x0f;
const HEX_DECIMAL_DIGITS: u8 = 10;
const ASCII_DIGIT_ZERO: u8 = b'0';
const ASCII_LOWER_A: u8 = b'a';

/// Helper: build a POST send-message request with an optional Idempotency-Key header.
fn send_message_req(session_id: &str, idempotency_key: Option<&str>) -> Request<Body> {
    send_message_req_with_content(session_id, idempotency_key, "Hello!")
}

fn send_message_req_with_content(
    session_id: &str,
    idempotency_key: Option<&str>,
    content: &str,
) -> Request<Body> {
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
            serde_json::to_vec(&serde_json::json!({ "content": content })).unwrap(),
        ))
        .unwrap()
}

async fn create_session_with_key(router: &axum::Router, session_key: &str) -> serde_json::Value {
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": session_key
        })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    body_json(resp).await
}

fn body_fingerprint(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"send_message\0content\0");
    hasher.update(content.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(content.as_bytes());

    let digest = hasher.finalize();
    let mut hex = String::from("sha256:");
    for byte in digest {
        hex.push(lower_hex_char(byte >> HEX_HIGH_NIBBLE_SHIFT));
        hex.push(lower_hex_char(byte & HEX_LOW_NIBBLE_MASK));
    }
    hex
}

fn lower_hex_char(nibble: u8) -> char {
    if nibble < HEX_DECIMAL_DIGITS {
        char::from(ASCII_DIGIT_ZERO + nibble)
    } else {
        char::from(ASCII_LOWER_A + (nibble - HEX_DECIMAL_DIGITS))
    }
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
async fn idempotency_key_reuse_across_sessions_returns_409() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created_a = create_session_with_key(&router, "idempotency-cross-a").await;
    let created_b = create_session_with_key(&router, "idempotency-cross-b").await;
    let session_a = created_a["id"].as_str().unwrap();
    let session_b = created_b["id"].as_str().unwrap();
    let key = "cross-session-key-5157";

    let req1 = send_message_req_with_content(session_a, Some(key), "same body");
    let resp1 = router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let _body1 = body_string(resp1).await;

    let req2 = send_message_req_with_content(session_b, Some(key), "same body");
    let resp2 = router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::CONFLICT);
    let body = body_json(resp2).await;
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn idempotency_key_reuse_with_different_body_returns_409() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();
    let key = "body-mismatch-key-5157";

    let req1 = send_message_req_with_content(id, Some(key), "first body");
    let resp1 = router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let _body1 = body_string(resp1).await;

    let req2 = send_message_req_with_content(id, Some(key), "changed body");
    let resp2 = router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::CONFLICT);
    let body = body_json(resp2).await;
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn idempotency_key_in_flight_returns_409() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    // WHY: Pre-seeding the cache with the test principal ("test-user") and the
    // raw key simulates an in-flight request and triggers the 409 Conflict path.
    let key = "inflight-key-001";
    state
        .idempotency_cache
        .check_or_insert("test-user", key, id, &body_fingerprint("Hello!"));

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

/// #5453: dropping the SSE response (client disconnect) must not leave the
/// idempotency key in `InFlight` until TTL. The key may become `Completed` if
/// the turn finished before the drop, but it must not remain `Conflict`.
#[tokio::test]
async fn idempotency_key_not_stranded_after_disconnect() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let key = "disconnect-key-001";
    let req = send_message_req(id, Some(key));
    let resp = router.clone().oneshot(req).await.unwrap();

    // WHY: Simulate a client that receives the HTTP response headers and then
    // disconnects without consuming the SSE body.
    assert_eq!(resp.status(), StatusCode::OK);
    drop(resp);

    tokio::time::sleep(Duration::from_millis(50)).await;

    let lookup = state.idempotency_cache.check_or_insert("test-user", key);
    assert!(
        !matches!(lookup, crate::idempotency::LookupResult::Conflict),
        "in-flight idempotency key must not be stranded after disconnect"
    );
}
