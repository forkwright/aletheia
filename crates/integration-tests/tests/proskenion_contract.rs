//! Proskenion live-server contract tests.
//!
//! These tests exercise the HTTP/SSE protocol surface consumed by the desktop
//! client without driving the GTK/WebKit UI.

#![expect(
    clippy::expect_used,
    reason = "contract tests fail fast on setup errors"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "JSON contract assertions intentionally show exact response bodies"
)]

use axum::http::StatusCode;
use integration_tests::harness::{TEST_NOUS_ID, TestHarness, body_json, body_string};
use serde_json::Value;
use tower::ServiceExt;

#[derive(Debug)]
struct SseDataEvent {
    event: String,
    data: Value,
}

fn parse_sse_data_events(body: &str) -> Vec<SseDataEvent> {
    let mut events = Vec::new();
    let mut event_name: Option<String> = None;
    let mut data_lines = Vec::new();

    for line in body.lines() {
        if line.is_empty() {
            if !data_lines.is_empty() {
                let raw_data = data_lines.join("\n");
                let data = serde_json::from_str(&raw_data).unwrap_or_else(|err| {
                    panic!(
                        "proskenion SSE contract mismatch: data line was not JSON: {err}; \
                         event={event_name:?}; data={raw_data:?}; full body={body}"
                    )
                });
                events.push(SseDataEvent {
                    event: event_name.take().unwrap_or_else(|| "message".to_owned()),
                    data,
                });
                data_lines.clear();
            } else {
                event_name = None;
            }
            continue;
        }

        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_owned());
        }
    }

    if !data_lines.is_empty() {
        let raw_data = data_lines.join("\n");
        let data = serde_json::from_str(&raw_data).unwrap_or_else(|err| {
            panic!(
                "proskenion SSE contract mismatch: trailing data line was not JSON: {err}; \
                 event={event_name:?}; data={raw_data:?}; full body={body}"
            )
        });
        events.push(SseDataEvent {
            event: event_name.unwrap_or_else(|| "message".to_owned()),
            data,
        });
    }

    events
}

fn string_field<'a>(json: &'a Value, field: &str, context: &str) -> &'a str {
    json.get(field).and_then(Value::as_str).unwrap_or_else(|| {
        panic!(
            "proskenion contract mismatch in {context}: expected string field `{field}`, got {json}"
        )
    })
}

fn array_field<'a>(json: &'a Value, field: &str, context: &str) -> &'a Vec<Value> {
    json.get(field).and_then(Value::as_array).unwrap_or_else(|| {
        panic!(
            "proskenion contract mismatch in {context}: expected array field `{field}`, got {json}"
        )
    })
}

fn assert_event_type(event: &SseDataEvent) {
    let event_type = string_field(&event.data, "type", "SSE data envelope");
    assert_eq!(
        event.event, event_type,
        "proskenion SSE contract mismatch: `event:` must match JSON `type`; \
         event line={}; data={}",
        event.event, event.data
    );
}

#[tokio::test]
async fn proskenion_contract_session_create_list_history_matches_desktop() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": TEST_NOUS_ID,
            "session_key": "proskenion-contract"
        })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("POST /api/v1/sessions");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "proskenion contract mismatch: session create must return 201"
    );
    let created = body_json(resp).await;
    let session_id = string_field(&created, "id", "session create");
    assert_eq!(
        string_field(&created, "nous_id", "session create"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: created session should keep requested nous_id; body={created}"
    );
    assert_eq!(
        string_field(&created, "session_key", "session create"),
        "proskenion-contract",
        "proskenion contract mismatch: created session should keep requested session_key; body={created}"
    );
    assert_eq!(
        string_field(&created, "status", "session create"),
        "active",
        "proskenion contract mismatch: created sessions should be active; body={created}"
    );
    assert!(
        created
            .get("message_count")
            .and_then(Value::as_i64)
            .is_some(),
        "proskenion contract mismatch: create response must expose numeric message_count; body={created}"
    );
    assert!(
        created.get("created_at").and_then(Value::as_str).is_some()
            && created.get("updated_at").and_then(Value::as_str).is_some(),
        "proskenion contract mismatch: create response must expose created_at and updated_at; body={created}"
    );

    let req = harness.authed_get("/api/v1/sessions?limit=25");
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("GET /api/v1/sessions");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: session list must return 200"
    );
    let listed = body_json(resp).await;
    let items = array_field(&listed, "items", "session list");
    assert!(
        listed.get("has_more").and_then(Value::as_bool).is_some(),
        "proskenion contract mismatch: session list must expose boolean has_more; body={listed}"
    );
    let listed_session = items
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(session_id))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: session list did not include created session \
                 id={session_id}; body={listed}"
            )
        });
    assert_eq!(
        string_field(listed_session, "session_key", "session list item"),
        "proskenion-contract",
        "proskenion contract mismatch: list item must expose session_key; item={listed_session}"
    );

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{session_id}/messages"),
        Some(serde_json::json!({ "content": "hello from proskenion contract" })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("POST /api/v1/sessions/{id}/messages");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: message send must return 200"
    );
    let _stream_body = body_string(resp).await;

    let req = harness.authed_get(&format!("/api/v1/sessions/{session_id}/history"));
    let resp = router
        .oneshot(req)
        .await
        .expect("GET /api/v1/sessions/{id}/history");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion contract mismatch: history must return 200"
    );
    let history = body_json(resp).await;
    let messages = array_field(&history, "messages", "history");
    assert!(
        messages.iter().any(|message| {
            message.get("role").and_then(Value::as_str) == Some("user")
                && message.get("content").and_then(Value::as_str)
                    == Some("hello from proskenion contract")
        }),
        "proskenion contract mismatch: history must contain the user message as \
         {{role, content}} strings; body={history}"
    );
    assert!(
        messages.iter().any(|message| {
            message.get("role").and_then(Value::as_str) == Some("assistant")
                && message.get("content").and_then(Value::as_str).is_some()
        }),
        "proskenion contract mismatch: history must contain an assistant message \
         with string content; body={history}"
    );
}

#[tokio::test]
async fn proskenion_contract_chat_stream_sse_matches_desktop() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "nous_id": TEST_NOUS_ID,
            "session_key": "proskenion-stream-contract",
            "message": "stream this for desktop"
        })),
    );
    let resp = router
        .oneshot(req)
        .await
        .expect("POST /api/v1/sessions/stream");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "proskenion SSE contract mismatch: stream endpoint must return 200"
    );
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        content_type.starts_with("text/event-stream"),
        "proskenion SSE contract mismatch: stream endpoint must return \
         text/event-stream content-type, got {content_type:?}"
    );
    let body = body_string(resp).await;
    let events = parse_sse_data_events(&body);
    assert!(
        !events.is_empty(),
        "proskenion SSE contract mismatch: stream returned no data events; body={body}"
    );
    for event in &events {
        assert_event_type(event);
    }

    let start_index = events
        .iter()
        .position(|event| event.event == "message_start")
        .unwrap_or_else(|| {
            panic!(
                "proskenion SSE contract mismatch: expected message_start event \
                 before deltas; events={events:?}; body={body}"
            )
        });
    let start = &events[start_index];
    assert!(
        start
            .data
            .get("session_id")
            .and_then(Value::as_str)
            .is_some()
            && start.data.get("nous_id").and_then(Value::as_str) == Some(TEST_NOUS_ID)
            && start.data.get("turn_id").and_then(Value::as_str).is_some(),
        "proskenion SSE contract mismatch: message_start must expose \
         session_id, nous_id, and turn_id strings; data={}",
        start.data
    );

    let complete_index = events
        .iter()
        .position(|event| event.event == "message_complete")
        .unwrap_or_else(|| {
            panic!(
                "proskenion SSE contract mismatch: expected terminal message_complete; \
                 events={events:?}; body={body}"
            )
        });
    assert_eq!(
        complete_index,
        events.len() - 1,
        "proskenion SSE contract mismatch: message_complete must be the final \
         terminal event because desktop clients stop after terminal events; \
         events={events:?}; body={body}"
    );
    assert!(
        start_index < complete_index,
        "proskenion SSE contract mismatch: message_start must precede \
         message_complete; events={events:?}; body={body}"
    );
    let complete = &events[complete_index];
    let outcome = complete
        .data
        .get("outcome")
        .and_then(Value::as_object)
        .unwrap_or_else(|| {
            panic!(
                "proskenion SSE contract mismatch: message_complete must carry \
                 object outcome; data={}",
                complete.data
            )
        });
    for field in [
        "text",
        "nous_id",
        "session_id",
        "tool_calls",
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
    ] {
        assert!(
            outcome.contains_key(field),
            "proskenion SSE contract mismatch: outcome missing `{field}`; \
             outcome={outcome:?}; complete={}",
            complete.data
        );
    }
    assert!(
        outcome
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty()),
        "proskenion SSE contract mismatch: mock stream may complete without \
         intermediate text_delta, but terminal outcome.text must be non-empty; \
         outcome={outcome:?}; events={events:?}; body={body}"
    );
}
