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
use crate::idempotency::LookupResult;
use crate::turn_buffer::TurnBufferHandle;

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

#[derive(Debug, PartialEq, Eq)]
struct SseFrame {
    id: Option<String>,
    event: Option<String>,
    data: serde_json::Value,
}

fn collect_sse_frames(body: &str) -> Vec<SseFrame> {
    let mut frames = Vec::new();
    let mut id = None;
    let mut event = None;
    let mut data_lines = Vec::new();

    for line in body.lines() {
        if line.is_empty() {
            push_sse_frame(&mut frames, &mut id, &mut event, &mut data_lines);
            continue;
        }
        if let Some(value) = line.strip_prefix("id:") {
            id = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_owned());
        }
    }
    push_sse_frame(&mut frames, &mut id, &mut event, &mut data_lines);

    frames
}

fn push_sse_frame(
    frames: &mut Vec<SseFrame>,
    id: &mut Option<String>,
    event: &mut Option<String>,
    data_lines: &mut Vec<String>,
) {
    if data_lines.is_empty() {
        *id = None;
        *event = None;
        return;
    }
    let data = data_lines.join("\n");
    frames.push(SseFrame {
        id: id.take(),
        event: event.take(),
        data: serde_json::from_str(&data).expect("SSE data must be JSON"),
    });
    data_lines.clear();
}

fn frame_types(frames: &[SseFrame]) -> Vec<String> {
    frames
        .iter()
        .map(|frame| frame.data["type"].as_str().unwrap().to_owned())
        .collect()
}

fn find_frame<'a>(frames: &'a [SseFrame], event_type: &str) -> &'a SseFrame {
    frames
        .iter()
        .find(|frame| frame.data["type"].as_str() == Some(event_type))
        .unwrap_or_else(|| panic!("missing {event_type} in frames: {frames:?}"))
}

fn tool_replay_events(session_id: &str, turn_id: &str) -> [(&'static str, serde_json::Value); 4] {
    [
        (
            "message_start",
            serde_json::json!({
                "type": "message_start",
                "status": "accepted",
                "session_id": session_id,
                "nous_id": "syn",
                "turn_id": turn_id,
                "request_id": "req-tool-replay",
            }),
        ),
        (
            "tool_use",
            serde_json::json!({
                "type": "tool_use",
                "id": "toolu_4865",
                "name": "read_file",
                "input": {"path": "/tmp/alice.txt"},
            }),
        ),
        (
            "tool_result",
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": "toolu_4865",
                "content": "synthetic contents",
                "is_error": false,
            }),
        ),
        (
            "message_complete",
            serde_json::json!({
                "type": "message_complete",
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 5,
                    "cache_read_tokens": 2,
                    "cache_write_tokens": 1,
                },
                "request_id": "req-tool-replay",
            }),
        ),
    ]
}

async fn seed_completed_tool_replay(
    state: &Arc<AppState>,
    session_id: &str,
    key: &str,
    turn_id: &str,
) {
    let buffer = state
        .turn_buffer_registry
        .get_or_create(session_id, turn_id)
        .await;
    let handle = TurnBufferHandle::new(buffer);
    for (event_type, event) in tool_replay_events(session_id, turn_id) {
        handle.record(event_type, &event.to_string()).await;
    }
    handle.mark_completed().await;

    let fingerprint = body_fingerprint("Hello!");
    assert!(matches!(
        state
            .idempotency_cache
            .check_or_insert("test-user", key, session_id, &fingerprint),
        LookupResult::Miss
    ));
    state.idempotency_cache.complete(
        "test-user",
        key,
        session_id,
        &fingerprint,
        StatusCode::OK,
        serde_json::json!({
            "turn_id": turn_id,
            "status": "completed",
        })
        .to_string(),
    );
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
async fn idempotency_key_replay_returns_original_event_sequence() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req1 = send_message_req(id, Some("replay-key-001"));
    let resp1 = router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let body1 = body_string(resp1).await;
    let original_frames = collect_sse_frames(&body1);
    assert_eq!(
        frame_types(&original_frames),
        vec!["message_start", "text_delta", "message_complete"]
    );
    let original_start = find_frame(&original_frames, "message_start");
    let turn_id = original_start.data["turn_id"].as_str().unwrap().to_owned();
    assert_eq!(original_start.id.as_deref(), Some("1"));
    assert_eq!(original_start.data["session_id"], id);

    tokio::time::sleep(Duration::from_millis(50)).await;

    let req2 = send_message_req(id, Some("replay-key-001"));
    let resp2 = router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = body_string(resp2).await;
    let replay_frames = collect_sse_frames(&body2);

    assert_eq!(
        replay_frames, original_frames,
        "idempotency replay must return the recorded turn event sequence"
    );
    assert_eq!(
        find_frame(&replay_frames, "message_start").data["turn_id"],
        turn_id.as_str()
    );
    assert!(
        replay_frames
            .iter()
            .all(|frame| frame.id.as_deref() != Some("0")),
        "replay must preserve recorded event ids, not synthesize seq=0"
    );

    let complete = find_frame(&replay_frames, "message_complete");
    assert_eq!(complete.id.as_deref(), Some("3"));
    assert_eq!(complete.data["stop_reason"], "end_turn");
    assert_eq!(complete.data["usage"]["input_tokens"], 10);
    assert_eq!(complete.data["usage"]["output_tokens"], 5);
    assert!(complete.data["usage"]["cache_read_tokens"].is_u64());
    assert!(complete.data["usage"]["cache_write_tokens"].is_u64());
}

#[tokio::test]
async fn idempotency_key_replay_preserves_tool_events() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();
    let key = "tool-replay-key-4865";
    let turn_id = "turn-tool-replay-4865";
    seed_completed_tool_replay(&state, id, key, turn_id).await;

    let req = send_message_req(id, Some(key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let frames = collect_sse_frames(&body);

    assert_eq!(
        frame_types(&frames),
        vec![
            "message_start",
            "tool_use",
            "tool_result",
            "message_complete"
        ]
    );
    assert_eq!(
        find_frame(&frames, "message_start").data["turn_id"],
        turn_id
    );
    assert_eq!(find_frame(&frames, "tool_use").data["id"], "toolu_4865");
    assert_eq!(
        find_frame(&frames, "tool_result").data["tool_use_id"],
        "toolu_4865"
    );
    assert_eq!(
        find_frame(&frames, "message_complete").data["usage"]["cache_read_tokens"],
        2
    );
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
    let turn_id = "turn-inflight-4865";
    assert!(matches!(
        state.idempotency_cache.check_or_insert_with_in_flight_body(
            "test-user",
            key,
            id,
            &body_fingerprint("Hello!"),
            Some(
                serde_json::json!({
                    "turn_id": turn_id,
                    "status": "running",
                })
                .to_string(),
            ),
        ),
        LookupResult::Miss
    ));

    let req = send_message_req(id, Some(key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "stream_turn_conflict");
    assert_eq!(body["error"]["details"]["turn_id"], turn_id);
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

    // WHY(#5453): retry through the handler to prove the in-flight entry was
    // released when the first response was dropped.
    let retry = send_message_req(id, Some(key));
    let retry_resp = router.clone().oneshot(retry).await.unwrap();
    assert_ne!(
        retry_resp.status(),
        StatusCode::CONFLICT,
        "retry after disconnect must not return 409 Conflict"
    );
}
