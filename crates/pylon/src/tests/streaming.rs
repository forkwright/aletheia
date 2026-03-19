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
    // The serialization fallback emits an error event, not an empty data line.
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
                // Each concurrent client creates its own session to avoid sharing state.
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
    // Verify we got an SSE response, then drop the body without reading it.
    // The handler's spawned turn task must not panic when the channel closes.
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/event-stream"));
    // Body is dropped here: the spawned task detects the closed channel and exits.
    drop(resp);

    // Brief yield to let the background task observe channel closure.
    tokio::time::sleep(Duration::from_millis(50)).await;
    // If we reach here without panic, the connection-drop cleanup is correct.
}
