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

/// Parse the `data:` lines of an SSE response into JSON values.
fn parse_sse_events(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
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
    let original_events = parse_sse_events(&body1);
    let original_types: Vec<_> = original_events
        .iter()
        .map(|e| e["type"].as_str().unwrap_or("").to_owned())
        .collect();
    let original_complete = original_events
        .iter()
        .find(|e| e["type"].as_str() == Some("message_complete"));
    let original_turn_id = original_events
        .iter()
        .find(|e| e["type"].as_str() == Some("message_start"))
        .and_then(|e| e["turn_id"].as_str());

    tokio::time::sleep(Duration::from_millis(50)).await;

    let req2 = send_message_req(id, Some("replay-key-001"));
    let resp2 = router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = body_string(resp2).await;
    let replayed_events = parse_sse_events(&body2);

    assert!(
        replayed_events
            .iter()
            .any(|e| e["type"].as_str() == Some("message_complete")),
        "replayed response should contain message_complete event, got: {body2}"
    );
    assert!(
        replayed_events
            .iter()
            .any(|e| e["type"].as_str() == Some("message_start")),
        "replayed response should contain message_start event, got: {body2}"
    );
    assert!(
        replayed_events
            .iter()
            .any(|e| e["type"].as_str() == Some("text_delta")),
        "replayed response should preserve text_delta event, got: {body2}"
    );

    // WHY(#4865): the replay must carry the same canonical turn identity and
    // event order as the original request.
    let replayed_types: Vec<_> = replayed_events
        .iter()
        .map(|e| e["type"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(
        replayed_types, original_types,
        "replayed event order and types must match original"
    );
    let replayed_turn_id = replayed_events
        .iter()
        .find(|e| e["type"].as_str() == Some("message_start"))
        .and_then(|e| e["turn_id"].as_str());
    assert_eq!(
        replayed_turn_id, original_turn_id,
        "replayed turn_id must match original"
    );

    if let Some(orig) = original_complete {
        let replayed_complete = replayed_events
            .iter()
            .find(|e| e["type"].as_str() == Some("message_complete"))
            .expect("message_complete in replay");
        assert_eq!(
            replayed_complete["stop_reason"], orig["stop_reason"],
            "replayed stop_reason must match original"
        );
        assert_eq!(
            replayed_complete["usage"], orig["usage"],
            "replayed usage must match original"
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
    assert_eq!(body["error"]["code"], "idempotency_conflict");
}

/// #4865: duplicate requests while the original turn is in flight must return
/// a typed conflict containing the canonical turn id and a replay endpoint.
#[tokio::test]
async fn idempotency_key_in_flight_returns_turn_id_and_replay_url() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let key = "inflight-key-turn-id-001";
    let turn_id = "turn-inflight-001";
    state
        .idempotency_cache
        .check_or_insert("test-user", key, id, &body_fingerprint("Hello!"));
    state.idempotency_cache.bind_turn_id(
        "test-user",
        key,
        id,
        &body_fingerprint("Hello!"),
        turn_id,
    );

    let req = send_message_req(id, Some(key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "idempotency_conflict");
    assert_eq!(body["error"]["details"]["turn_id"], turn_id);
    assert_eq!(
        body["error"]["details"]["replay_url"],
        format!("/api/v1/sessions/{id}/turns/{turn_id}/events")
    );
}

/// #4865: idempotency replay must preserve the original event order and all
/// event types, including `tool_use/tool_result`, from the durable turn buffer.
#[expect(
    clippy::too_many_lines,
    reason = "WHY(#4865): seeds the durable turn-event store and asserts full event-sequence preservation — inherently linear setup"
)]
#[tokio::test]
async fn idempotency_key_replay_preserves_tool_events_and_usage() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let session_id = created["id"].as_str().unwrap();
    let turn_id = "turn-replay-tool-001";
    let key = "replay-tool-key-001";
    let fingerprint = body_fingerprint("Hello!");

    // WHY: seed the durable turn-event store with a canonical sequence so the
    // replay path can be exercised without relying on a tool-emitting provider.
    let buf = state
        .turn_buffer_registry
        .get_or_create(session_id, turn_id)
        .await;
    let handle = crate::turn_buffer::TurnBufferHandle::new(buf);
    handle
        .record(
            "message_start",
            &serde_json::json!({
                "type": "message_start",
                "status": "accepted",
                "session_id": session_id,
                "nous_id": "syn",
                "turn_id": turn_id,
                "request_id": "req-original"
            })
            .to_string(),
        )
        .await;
    handle
        .record(
            "text_delta",
            &serde_json::json!({"type": "text_delta", "text": "Let me check that."}).to_string(),
        )
        .await;
    handle
        .record(
            "tool_use",
            &serde_json::json!({
                "type": "tool_use",
                "id": "toolu_01",
                "name": "test_tool",
                "input": {"arg": "value"}
            })
            .to_string(),
        )
        .await;
    handle
        .record(
            "tool_result",
            &serde_json::json!({
                "type": "tool_result",
                "tool_use_id": "toolu_01",
                "content": "ok",
                "is_error": false
            })
            .to_string(),
        )
        .await;
    handle
        .record(
            "message_complete",
            &serde_json::json!({
                "type": "message_complete",
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 15,
                    "output_tokens": 10,
                    "cache_read_tokens": 5,
                    "cache_write_tokens": 2
                },
                "request_id": "req-original"
            })
            .to_string(),
        )
        .await;
    handle.mark_completed().await;

    // WHY(#4865): complete() is update-only; seed the cache entry first so the
    // subsequent HTTP request sees LookupResult::Hit and triggers the replay path.
    state
        .idempotency_cache
        .check_or_insert("test-user", key, session_id, &fingerprint);

    // Bind the idempotency key to the canonical turn identity.
    let replay_body = serde_json::json!({
        "session_id": session_id,
        "turn_id": turn_id,
        "nous_id": "syn",
        "summary": {
            "stop_reason": "end_turn",
            "input_tokens": 15,
            "output_tokens": 10,
            "cache_read_tokens": 5,
            "cache_write_tokens": 2
        }
    })
    .to_string();
    state.idempotency_cache.complete(
        "test-user",
        key,
        session_id,
        &fingerprint,
        crate::idempotency::CompletionRecord {
            turn_id: turn_id.to_owned(),
            status: StatusCode::OK,
            body: replay_body,
        },
    );

    let req = send_message_req_with_content(session_id, Some(key), "Hello!");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let events = parse_sse_events(&body);

    let types: Vec<_> = events
        .iter()
        .map(|e| e["type"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(
        types,
        vec![
            "message_start",
            "text_delta",
            "tool_use",
            "tool_result",
            "message_complete"
        ]
    );

    let start = events
        .iter()
        .find(|e| e["type"].as_str() == Some("message_start"))
        .expect("message_start in replay");
    assert_eq!(start["session_id"], session_id);
    assert_eq!(start["turn_id"], turn_id);
    assert_eq!(start["nous_id"], "syn");

    let tool_use = events
        .iter()
        .find(|e| e["type"].as_str() == Some("tool_use"))
        .expect("tool_use in replay");
    assert_eq!(tool_use["id"], "toolu_01");
    assert_eq!(tool_use["name"], "test_tool");

    let tool_result = events
        .iter()
        .find(|e| e["type"].as_str() == Some("tool_result"))
        .expect("tool_result in replay");
    assert_eq!(tool_result["tool_use_id"], "toolu_01");
    assert_eq!(tool_result["content"], "ok");

    let complete = events
        .iter()
        .find(|e| e["type"].as_str() == Some("message_complete"))
        .expect("message_complete in replay");
    assert_eq!(complete["stop_reason"], "end_turn");
    assert_eq!(complete["usage"]["input_tokens"], 15);
    assert_eq!(complete["usage"]["output_tokens"], 10);
    assert_eq!(complete["usage"]["cache_read_tokens"], 5);
    assert_eq!(complete["usage"]["cache_write_tokens"], 2);
}

/// #4865: when the original turn buffer has been reaped, idempotency replay
/// falls back to a synthetic event sequence that still carries the canonical
/// turn identity and persisted usage/cache summary.
#[tokio::test]
async fn idempotency_key_replay_fallback_includes_turn_id_and_usage() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let session_id = created["id"].as_str().unwrap();
    let turn_id = "turn-replay-fallback-001";
    let key = "replay-fallback-key-001";
    let fingerprint = body_fingerprint("Hello!");

    // WHY(#4865): complete() is update-only; seed the cache entry first.
    state
        .idempotency_cache
        .check_or_insert("test-user", key, session_id, &fingerprint);

    let replay_body = serde_json::json!({
        "session_id": session_id,
        "turn_id": turn_id,
        "nous_id": "syn",
        "summary": {
            "stop_reason": "end_turn",
            "input_tokens": 20,
            "output_tokens": 12,
            "cache_read_tokens": 8,
            "cache_write_tokens": 4
        }
    })
    .to_string();
    state.idempotency_cache.complete(
        "test-user",
        key,
        session_id,
        &fingerprint,
        crate::idempotency::CompletionRecord {
            turn_id: turn_id.to_owned(),
            status: StatusCode::OK,
            body: replay_body,
        },
    );

    let req = send_message_req_with_content(session_id, Some(key), "Hello!");
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let events = parse_sse_events(&body);

    let types: Vec<_> = events
        .iter()
        .map(|e| e["type"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(types, vec!["message_start", "message_complete"]);

    let start = events
        .iter()
        .find(|e| e["type"].as_str() == Some("message_start"))
        .expect("message_start in fallback replay");
    assert_eq!(start["session_id"], session_id);
    assert_eq!(start["turn_id"], turn_id);
    assert_eq!(start["nous_id"], "syn");

    let complete = events
        .iter()
        .find(|e| e["type"].as_str() == Some("message_complete"))
        .expect("message_complete in fallback replay");
    assert_eq!(complete["stop_reason"], "end_turn");
    assert_eq!(complete["usage"]["input_tokens"], 20);
    assert_eq!(complete["usage"]["output_tokens"], 12);
    assert_eq!(complete["usage"]["cache_read_tokens"], 8);
    assert_eq!(complete["usage"]["cache_write_tokens"], 4);
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
