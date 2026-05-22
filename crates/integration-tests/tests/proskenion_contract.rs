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
use hermeneus::test_utils::MockProvider;
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

fn object_field<'a>(
    json: &'a Value,
    field: &str,
    context: &str,
) -> &'a serde_json::Map<String, Value> {
    json.get(field).and_then(Value::as_object).unwrap_or_else(|| {
        panic!(
            "proskenion contract mismatch in {context}: expected object field `{field}`, got {json}"
        )
    })
}

fn numeric_field(json: &Value, field: &str, context: &str) {
    assert!(
        json.get(field).is_some_and(Value::is_number),
        "proskenion contract mismatch in {context}: expected numeric field `{field}`, got {json}"
    );
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

struct RawResponse {
    status: u16,
    body: Vec<u8>,
}

impl RawResponse {
    fn body_json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("proskenion contract JSON body")
    }
}

async fn raw_get(addr: std::net::SocketAddr, path: &str, token: &str) -> RawResponse {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect proskenion contract TCP server");
    let request = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Authorization: Bearer {token}\r\n\
         Connection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write proskenion contract HTTP request");

    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .expect("read proskenion contract HTTP response");

    parse_http_response(&buf)
}

fn parse_http_response(bytes: &[u8]) -> RawResponse {
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("HTTP response missing header terminator");
    let head =
        std::str::from_utf8(&bytes[..header_end]).expect("HTTP response headers must be UTF-8");
    let mut lines = head.lines();
    let status_line = lines.next().expect("HTTP response missing status line");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .expect("HTTP response missing status code")
        .parse::<u16>()
        .expect("HTTP response status code must be numeric");

    let encoded_body = &bytes[header_end + 4..];
    let body = if head
        .lines()
        .any(|line| line.eq_ignore_ascii_case("transfer-encoding: chunked"))
    {
        decode_chunked(encoded_body)
    } else {
        encoded_body.to_vec()
    };

    RawResponse { status, body }
}

fn decode_chunked(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let line_end = bytes[index..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .expect("chunk size line terminator");
        let size_str =
            std::str::from_utf8(&bytes[index..index + line_end]).expect("chunk size UTF-8");
        let size = usize::from_str_radix(size_str.trim(), 16).expect("chunk size hex");
        index += line_end + 2;
        if size == 0 {
            break;
        }
        out.extend_from_slice(&bytes[index..index + size]);
        index += size + 2;
    }
    out
}

fn server_addr(base_url: &str) -> std::net::SocketAddr {
    base_url
        .strip_prefix("http://")
        .unwrap_or(base_url)
        .parse()
        .expect("parse proskenion contract server address")
}

async fn authed_get_json(addr: std::net::SocketAddr, token: &str, path: &str) -> Value {
    let resp = raw_get(addr, path, token).await;
    assert_eq!(
        resp.status,
        StatusCode::OK.as_u16(),
        "proskenion contract mismatch: GET {path} must return 200"
    );
    resp.body_json()
}

#[tokio::test]
async fn proskenion_contract_nous_surfaces_match_desktop() {
    let harness = TestHarness::build_with_provider_and_tools(
        Box::new(MockProvider::new("Hello from mock!").models(&["mock-model"])),
        true,
    )
    .await;
    let (base_url, token, _harness) = harness.start_tcp_server().await;
    let addr = server_addr(&base_url);

    let listed = authed_get_json(addr, &token, "/api/v1/nous").await;
    let nous = array_field(&listed, "nous", "nous list");
    let agent = nous
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(TEST_NOUS_ID))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: nous list did not include {TEST_NOUS_ID}; \
                 body={listed}"
            )
        });
    assert_eq!(
        string_field(agent, "name", "nous list item"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: list item should expose display name; item={agent}"
    );
    assert_eq!(
        string_field(agent, "model", "nous list item"),
        "mock-model",
        "proskenion contract mismatch: list item should expose model; item={agent}"
    );
    assert_eq!(
        string_field(agent, "status", "nous list item"),
        "active",
        "proskenion contract mismatch: list item should expose active status; item={agent}"
    );

    let status = authed_get_json(addr, &token, &format!("/api/v1/nous/{TEST_NOUS_ID}")).await;
    assert_eq!(
        string_field(&status, "id", "nous status"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: status should keep requested nous id; body={status}"
    );
    assert_eq!(
        string_field(&status, "model", "nous status"),
        "mock-model",
        "proskenion contract mismatch: status should expose model; body={status}"
    );
    numeric_field(&status, "context_window", "nous status");
    numeric_field(&status, "max_output_tokens", "nous status");
    numeric_field(&status, "thinking_budget", "nous status");
    numeric_field(&status, "max_tool_iterations", "nous status");
    assert!(
        status
            .get("thinking_enabled")
            .and_then(Value::as_bool)
            .is_some(),
        "proskenion contract mismatch: status should expose boolean thinking_enabled; body={status}"
    );
    assert!(
        status.get("status").and_then(Value::as_str).is_some(),
        "proskenion contract mismatch: status should expose lifecycle status string; body={status}"
    );

    let tools = authed_get_json(addr, &token, &format!("/api/v1/nous/{TEST_NOUS_ID}/tools")).await;
    let tool_items = array_field(&tools, "tools", "nous tools");
    assert!(
        !tool_items.is_empty(),
        "proskenion contract mismatch: tools response should include registered built-in tools; body={tools}"
    );
    let first_tool = &tool_items[0];
    for field in ["name", "description", "category"] {
        assert!(
            first_tool.get(field).and_then(Value::as_str).is_some(),
            "proskenion contract mismatch: tool item missing string `{field}`; item={first_tool}"
        );
    }
    assert!(
        first_tool
            .get("auto_activate")
            .and_then(Value::as_bool)
            .is_some(),
        "proskenion contract mismatch: tool item should expose boolean auto_activate; item={first_tool}"
    );
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn proskenion_contract_knowledge_browse_surfaces_match_desktop() {
    let harness = TestHarness::build_with_knowledge_store().await;
    let (base_url, token, _harness) = harness.start_tcp_server().await;
    let addr = server_addr(&base_url);

    let facts = authed_get_json(addr, &token, "/api/v1/knowledge/facts?limit=25").await;
    array_field(&facts, "facts", "knowledge facts");
    numeric_field(&facts, "total", "knowledge facts");

    let entities = authed_get_json(addr, &token, "/api/v1/knowledge/entities?limit=25").await;
    array_field(&entities, "entities", "knowledge entities");
    numeric_field(&entities, "total", "knowledge entities");

    let timeline = authed_get_json(addr, &token, "/api/v1/knowledge/timeline?limit=25").await;
    array_field(&timeline, "events", "knowledge timeline");
    numeric_field(&timeline, "total", "knowledge timeline");

    let relationships = authed_get_json(
        addr,
        &token,
        "/api/v1/knowledge/entities/missing-entity/relationships",
    )
    .await;
    array_field(
        &relationships,
        "relationships",
        "knowledge entity relationships",
    );
}

#[tokio::test]
async fn proskenion_contract_metrics_surfaces_match_desktop() {
    let harness = TestHarness::build().await;
    let (base_url, token, _harness) = harness.start_tcp_server().await;
    let addr = server_addr(&base_url);

    let agents = authed_get_json(addr, &token, "/api/v1/metrics/agents").await;
    let agent_metrics = array_field(&agents, "agents", "agent metrics");
    assert!(
        agents.get("anomalies").and_then(Value::as_array).is_some(),
        "proskenion contract mismatch: agent metrics should expose anomalies array; body={agents}"
    );
    let test_agent = agent_metrics
        .iter()
        .find(|item| item.get("agent_id").and_then(Value::as_str) == Some(TEST_NOUS_ID))
        .unwrap_or_else(|| {
            panic!(
                "proskenion contract mismatch: metrics agents did not include {TEST_NOUS_ID}; \
                 body={agents}"
            )
        });
    assert_eq!(
        string_field(test_agent, "agent_name", "agent metrics item"),
        TEST_NOUS_ID,
        "proskenion contract mismatch: agent metrics should expose agent name; item={test_agent}"
    );
    for field in [
        "avg_tokens_per_response",
        "tool_calls_per_session",
        "tool_success_rate",
        "distillation_frequency",
        "avg_context_before_distill",
        "messages_per_session",
        "sessions_per_day",
        "errors_per_session",
    ] {
        numeric_field(test_agent, field, "agent metrics item");
    }

    let quality = authed_get_json(addr, &token, "/api/v1/metrics/quality").await;
    let series = object_field(&quality, "series", "quality metrics");
    for field in [
        "avg_turn_length",
        "response_to_question_ratio",
        "tool_call_density",
        "thinking_time_ratio",
    ] {
        assert!(
            series.get(field).and_then(Value::as_array).is_some(),
            "proskenion contract mismatch: quality series missing array `{field}`; body={quality}"
        );
    }
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
