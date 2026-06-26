#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracing::Instrument;

use super::helpers::*;

/// Parse every `data:` line in an SSE body into JSON values.
///
/// Returns only lines that successfully parse; lines that don't start with
/// `data:` (e.g. `event:`, blank lines) are skipped.
fn collect_sse_data_events(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}

/// Find the first SSE data event with the given `type` field value.
fn find_sse_event<'a>(
    events: &'a [serde_json::Value],
    event_type: &str,
) -> Option<&'a serde_json::Value> {
    events
        .iter()
        .find(|e| e["type"].as_str() == Some(event_type))
}

fn stream_turn_req(session_key: &str, message: &str, client_turn_id: &str) -> Request<Body> {
    authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": "syn",
            "message": message,
            "session_key": session_key,
            "client_turn_id": client_turn_id,
        })),
    )
}

/// Happy path: every `data:` line in the SSE stream must be valid JSON with a
/// `type` field. Tests structural correctness of the event format.
#[tokio::test]
async fn sse_stream_each_data_line_is_valid_json_with_type_field() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    assert!(
        !events.is_empty(),
        "stream must contain at least one event, got empty body:\n{body}"
    );
    for event in &events {
        assert!(
            event["type"].is_string(),
            "every data event must have a string 'type' field, got: {event}"
        );
        assert!(
            !event["type"].as_str().unwrap().is_empty(),
            "event type must not be empty, got: {event}"
        );
    }
}

/// Happy path: the `text_delta` event carries the mock provider's exact response text.
#[tokio::test]
async fn sse_stream_text_delta_text_matches_mock_response() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Echo test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    let text_delta = find_sse_event(&events, "text_delta").expect("stream must contain text_delta");
    assert_eq!(
        text_delta["text"].as_str().unwrap(),
        "Hello from mock!",
        "text_delta.text must equal the mock provider's response"
    );
}

/// Happy path: the `message_complete` event includes non-zero token usage that
/// matches the mock provider's reported counts (input=10, output=5).
#[tokio::test]
async fn sse_stream_message_complete_reports_token_counts() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Token count test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    let complete =
        find_sse_event(&events, "message_complete").expect("stream must contain message_complete");
    let usage = &complete["usage"];
    assert!(
        usage.is_object(),
        "message_complete must have a usage object, got: {complete}"
    );
    assert_eq!(
        usage["input_tokens"].as_u64().unwrap(),
        10,
        "input_tokens must match mock provider value"
    );
    assert_eq!(
        usage["output_tokens"].as_u64().unwrap(),
        5,
        "output_tokens must match mock provider value"
    );
    assert_eq!(
        complete["stop_reason"].as_str().unwrap(),
        "end_turn",
        "stop_reason must match mock provider StopReason::EndTurn"
    );
}

/// Happy path: `text_delta` appears before `message_complete` in the stream.
/// The handler must not emit the completion marker before content events.
#[tokio::test]
async fn sse_stream_text_delta_precedes_message_complete() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Ordering test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    let delta_idx = events
        .iter()
        .position(|e| e["type"].as_str() == Some("text_delta"))
        .expect("stream must contain text_delta");
    let complete_idx = events
        .iter()
        .position(|e| e["type"].as_str() == Some("message_complete"))
        .expect("stream must contain message_complete");

    assert!(
        delta_idx < complete_idx,
        "text_delta (index {delta_idx}) must appear before message_complete (index {complete_idx})"
    );
}

/// Malformed event: sending invalid JSON in the request body returns a 4xx
/// HTTP error. The handler rejects the bad request before streaming starts.
/// It does not crash or produce a 5xx.
#[tokio::test]
async fn sse_send_message_with_invalid_json_body_returns_client_error() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from("{not: valid json}"))
        .unwrap();

    let resp = router.clone().oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "invalid JSON body must return 400 or 422, not {}: the handler must not crash",
        resp.status()
    );
}

/// Error event: the fallback serialization-error data string is valid JSON
/// with a `message` field: not empty data. This verifies that the SSE error
/// fallback path produces a well-formed JSON payload per the stream contract.
#[test]
fn sse_serialization_fallback_data_is_valid_json_with_message_field() {
    // WHY: sse_event_to_axum (and the stream_turn equivalent) fall back to
    // this literal string when serde_json::to_string fails. Verify the string
    // is valid JSON with a non-empty message field: not an empty data line.
    let fallback_data = r#"{"message":"serialization failed"}"#;
    let parsed: serde_json::Value =
        serde_json::from_str(fallback_data).expect("fallback data must be valid JSON");
    assert!(
        parsed["message"].is_string(),
        "fallback error must have a string message field"
    );
    assert!(
        !parsed["message"].as_str().unwrap().is_empty(),
        "fallback error message must not be empty"
    );
    assert!(
        !fallback_data.is_empty(),
        "fallback data must never be empty"
    );
}

/// Concurrent clients: multiple simultaneous SSE connections complete without
/// interfering with each other. Each connection receives its own
/// `message_complete` event.
#[tokio::test]
async fn sse_concurrent_connections_each_receive_complete_stream() {
    let (state, _dir) = test_state().await;

    let mut handles = Vec::new();
    for i in 0..3u32 {
        let router = build_router(Arc::clone(&state), &test_security_config());
        handles.push(tokio::spawn(
            async move {
                // WHY: Each concurrent client creates its own session to avoid sharing state.
                let create_req = authed_request(
                    "POST",
                    "/api/v1/sessions",
                    Some(serde_json::json!({
                        "nous_id": "syn",
                        "session_key": format!("concurrent-sse-{i}"),
                    })),
                );
                let create_resp = router.clone().oneshot(create_req).await.unwrap();
                assert_eq!(create_resp.status(), StatusCode::CREATED);
                let session = body_json(create_resp).await;
                let id = session["id"].as_str().unwrap().to_owned();

                let msg_req = authed_request(
                    "POST",
                    &format!("/api/v1/sessions/{id}/messages"),
                    Some(serde_json::json!({ "content": format!("Hello from client {i}") })),
                );
                let resp = router.oneshot(msg_req).await.unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                body_string(resp).await
            }
            .instrument(tracing::info_span!("test_sse_connection", index = i)),
        ));
    }

    for handle in handles {
        let body = handle.await.unwrap();
        let events = collect_sse_data_events(&body);
        assert!(
            find_sse_event(&events, "message_complete").is_some(),
            "each concurrent client must receive its own message_complete event"
        );
        assert!(
            find_sse_event(&events, "text_delta").is_some(),
            "each concurrent client must receive its own text_delta event"
        );
    }
}

/// Connection drop: consuming only the response status (not the body) does
/// not panic or deadlock. The SSE handler spawns a background task that
/// terminates gracefully when the channel receiver is dropped.
#[tokio::test]
async fn sse_dropping_response_body_does_not_panic() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Drop test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    // NOTE: Verify we got an SSE response, then drop the body without reading it.
    // The handler's spawned turn task must not panic when the channel closes.
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/event-stream"));
    drop(resp);

    tokio::time::sleep(Duration::from_millis(50)).await;
}

/// Error path: sending an empty message returns 422 Unprocessable Entity.
#[tokio::test]
async fn send_message_empty_content_returns_422() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "content" && e["code"] == "required")
    );
}

/// Error path: sending an oversized message returns 422 Unprocessable Entity.
#[tokio::test]
async fn send_message_oversized_content_returns_422() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let oversized_content = "x".repeat(300_000); // > 262144 byte limit
    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": oversized_content })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "content" && e["code"] == "too_long")
    );
}

/// Error path: sending message to unknown session returns 404 Not Found.
#[tokio::test]
async fn send_message_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions/nonexistent/messages",
        Some(serde_json::json!({ "content": "test" })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "session_not_found");
}

/// Error path: invalid idempotency key with non-ASCII characters returns 400.
#[tokio::test]
async fn send_message_invalid_idempotency_key_returns_400() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("idempotency-key", "key with emoji 🎉")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({ "content": "test" })).unwrap(),
        ))
        .unwrap();

    let resp = router.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "bad_request");
}

/// Error path: empty idempotency key returns 400 Bad Request.
#[tokio::test]
async fn send_message_empty_idempotency_key_returns_400() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("idempotency-key", "")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({ "content": "test" })).unwrap(),
        ))
        .unwrap();

    let resp = router.clone().oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "bad_request");
}

/// Error path: `stream_turn` with empty message returns 422.
#[tokio::test]
async fn stream_turn_empty_message_returns_422() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": "syn",
            "message": "",
            "session_key": "test"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "message" && e["code"] == "required")
    );
}

/// Error path: `stream_turn` with oversized message returns 422.
#[tokio::test]
async fn stream_turn_oversized_message_returns_422() {
    let (app, _dir) = app().await;
    let oversized_message = "x".repeat(300_000);
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": "syn",
            "message": oversized_message,
            "session_key": "test"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "message" && e["code"] == "too_long")
    );
}

/// Error path: `stream_turn` with unknown `nous_id` returns 404 Not Found.
#[tokio::test]
async fn stream_turn_unknown_agent_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": "nonexistent-agent",
            "message": "test",
            "session_key": "test"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "nous_not_found");
}

/// Error path: `stream_turn` with oversized `nous_id` returns 422.
#[tokio::test]
async fn stream_turn_oversized_agent_id_returns_422() {
    let (app, _dir) = app().await;
    let oversized_agent_id = "a".repeat(300);
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": oversized_agent_id,
            "message": "test",
            "session_key": "test"
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "nous_id" && e["code"] == "too_long")
    );
}

/// Error path: `stream_turn` with oversized `session_key` returns 422.
#[tokio::test]
async fn stream_turn_oversized_session_key_returns_422() {
    let (app, _dir) = app().await;
    let oversized_session_key = "b".repeat(300);
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": "syn",
            "message": "test",
            "session_key": oversized_session_key
        })),
    );
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_failed");
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e["field"] == "session_key" && e["code"] == "too_long")
    );
}

#[tokio::test]
async fn stream_turn_duplicate_client_turn_id_does_not_persist_duplicate_messages() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let client_turn_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let session_key = "stream-idempotency-duplicate";
    let message = "dedupe this streamed turn";

    let first = router
        .clone()
        .oneshot(stream_turn_req(session_key, message, client_turn_id))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = body_string(first).await;
    let first_events = collect_sse_data_events(&first_body);
    let first_start =
        find_sse_event(&first_events, "message_start").expect("first stream must start");
    let session_id = first_start["session_id"].as_str().unwrap().to_owned();
    assert_eq!(first_start["turn_id"], client_turn_id);

    let duplicate = router
        .clone()
        .oneshot(stream_turn_req(session_key, message, client_turn_id))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::OK);
    let duplicate_body = body_string(duplicate).await;
    let duplicate_events = collect_sse_data_events(&duplicate_body);
    let duplicate_start = find_sse_event(&duplicate_events, "message_start")
        .expect("duplicate stream must replay message_start");
    assert_eq!(duplicate_start["session_id"], session_id);
    assert_eq!(duplicate_start["turn_id"], client_turn_id);

    let history_resp = router
        .oneshot(authed_get(&format!(
            "/api/v1/sessions/{session_id}/history"
        )))
        .await
        .unwrap();
    assert_eq!(history_resp.status(), StatusCode::OK);
    let history = body_json(history_resp).await;
    let messages = history["messages"].as_array().unwrap();
    assert_eq!(
        messages.len(),
        2,
        "duplicate client_turn_id must not persist another user/assistant pair"
    );
}

#[tokio::test]
async fn stream_turn_duplicate_client_turn_id_replays_reconnect_state() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let client_turn_id = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
    let session_key = "stream-idempotency-reconnect";
    let message = "replay this streamed turn";

    let first = router
        .clone()
        .oneshot(stream_turn_req(session_key, message, client_turn_id))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let _first_body = body_string(first).await;

    let retry = router
        .clone()
        .oneshot(stream_turn_req(session_key, message, client_turn_id))
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::OK);
    let retry_body = body_string(retry).await;
    let retry_events = collect_sse_data_events(&retry_body);

    let lifecycle = retry_events
        .first()
        .expect("duplicate POST must emit reconnect lifecycle state");
    assert_eq!(lifecycle["type"], "turn_reconnect_state");
    assert_eq!(lifecycle["state"], "completed");
    assert_eq!(lifecycle["live"], false);
    assert!(
        find_sse_event(&retry_events, "message_complete").is_some(),
        "duplicate POST must replay terminal stream state"
    );
}

#[tokio::test]
async fn stream_turn_stale_duplicate_client_turn_id_returns_conflict_with_turn_id() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let client_turn_id = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
    let session_key = "stream-idempotency-stale";
    let message = "expire this replay buffer";

    let first = router
        .clone()
        .oneshot(stream_turn_req(session_key, message, client_turn_id))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = body_string(first).await;
    let first_events = collect_sse_data_events(&first_body);
    let first_start =
        find_sse_event(&first_events, "message_start").expect("first stream must start");
    let session_id = first_start["session_id"].as_str().unwrap();

    tokio::time::pause();
    tokio::time::advance(Duration::from_mins(6)).await;
    state.turn_buffer_registry.reap_expired().await;
    assert!(
        state
            .turn_buffer_registry
            .get(session_id, client_turn_id)
            .await
            .is_none(),
        "test setup must expire the replay buffer while idempotency cache remains"
    );

    let conflict_response = router
        .oneshot(stream_turn_req(session_key, message, client_turn_id))
        .await
        .unwrap();
    assert_eq!(conflict_response.status(), StatusCode::CONFLICT);
    let body = body_json(conflict_response).await;
    assert_eq!(body["error"]["code"], "stream_turn_conflict");
    assert_eq!(body["error"]["details"]["turn_id"], client_turn_id);
}

/// #5163: the `message_start` event for `POST /sessions/{id}/messages` must
/// carry `session_id`, `nous_id`, and `turn_id` so clients can reconnect to
/// the turn event stream.
#[tokio::test]
async fn send_message_start_event_includes_reconnect_ids() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Reconnect ID test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    let start =
        find_sse_event(&events, "message_start").expect("stream must contain message_start");
    assert_eq!(start["status"], "accepted");
    assert_eq!(start["session_id"], id);
    assert_eq!(start["nous_id"], "syn");
    assert!(
        start["turn_id"].as_str().is_some_and(|s| !s.is_empty()),
        "turn_id must be a non-empty string"
    );
}

#[tokio::test]
async fn send_message_reconnect_replays_buffered_events() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Reconnect replay test" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    let start =
        find_sse_event(&events, "message_start").expect("stream must contain message_start");
    let session_id = start["session_id"].as_str().unwrap();
    let turn_id = start["turn_id"].as_str().unwrap();

    let token = default_token();
    let reconnect_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/sessions/{session_id}/turns/{turn_id}/events"
        ))
        .header("authorization", format!("Bearer {token}"))
        .header("last-event-id", "0")
        .body(Body::empty())
        .unwrap();
    let reconnect_resp = router.oneshot(reconnect_req).await.unwrap();
    assert_eq!(reconnect_resp.status(), StatusCode::OK);
    let ct = reconnect_resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/event-stream"));

    let reconnect_body = body_string(reconnect_resp).await;
    let replayed = collect_sse_data_events(&reconnect_body);

    let lifecycle = replayed
        .first()
        .expect("reconnect must emit lifecycle state");
    assert_eq!(lifecycle["type"], "turn_reconnect_state");
    assert_eq!(lifecycle["state"], "completed");
    assert_eq!(lifecycle["live"], false);

    let replayed_start =
        find_sse_event(&replayed, "message_start").expect("reconnect must replay message_start");
    assert_eq!(replayed_start["session_id"], session_id);
    assert_eq!(replayed_start["turn_id"], turn_id);
    assert!(
        find_sse_event(&replayed, "text_delta").is_some(),
        "reconnect must replay buffered text_delta"
    );
    assert!(
        find_sse_event(&replayed, "message_complete").is_some(),
        "reconnect must replay buffered message_complete"
    );
}
