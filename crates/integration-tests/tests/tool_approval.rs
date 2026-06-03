// kanon:ignore RUST/file-too-long — approval gate e2e scenarios require setup + approve + deny + none-gate coverage
//! End-to-end integration tests for the tool approval gate (#3958, ADR-005).
//!
//! Exercises the full HTTP path:
//!   POST /sessions/{id}/messages (SSE stream, blocks at approval gate)
//!   POST /sessions/{id}/approvals (operator decision → gate unblocked)
//!
//! Each scenario uses a real TCP server so both SSE and the approvals POST
//! can be concurrent HTTP requests. Synthetic identities only.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: indices are valid after asserting event counts"
)]

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hermeneus::provider::LlmProvider;
use hermeneus::types::{
    CompletionRequest, CompletionResponse, ContentBlock, Role, StopReason, Usage,
};
use integration_tests::harness::TestHarness;
use koina::id::ToolName;
use organon::registry::{ToolExecutor, ToolRegistry};
use organon::testing::install_crypto_provider;
use organon::types::{
    InputSchema, Reversibility, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput,
    ToolResult,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// ---------------------------------------------------------------------------
// Synthetic tool definitions
// ---------------------------------------------------------------------------

/// Minimal irreversible tool executor for the approval gate path.
struct ConfirmingExecutor {
    executed: Arc<Mutex<bool>>,
}

impl ToolExecutor for ConfirmingExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        let flag = Arc::clone(&self.executed);
        Box::pin(async move {
            *flag.lock().expect("lock") = true;
            Ok(ToolResult::text("irreversible-op completed"))
        })
    }
}

fn irreversible_tool_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("test_irreversible").expect("valid tool name"),
        description: "Synthetic irreversible tool for approval-gate e2e tests".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: indexmap::IndexMap::default(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Edit],
        tags: vec![],
    }
}

// ---------------------------------------------------------------------------
// Mock LLM provider helpers
// ---------------------------------------------------------------------------

/// Request-aware mock provider for the approval gate e2e path.
///
/// Returns `tool_use_response` for calls that contain a user message in the
/// conversation history (the main execute turn), and `text_response` for all
/// other calls (recall side-queries, skill ranking, etc.).  After one
/// `tool_use` turn it switches to always returning `text_response` so the
/// post-tool assistant turn completes normally.
struct ApprovalTestProvider {
    tool_use_resp: CompletionResponse,
    text_resp: CompletionResponse,
    tool_use_sent: Mutex<bool>,
    tool_result_seen: Mutex<bool>,
}

impl ApprovalTestProvider {
    fn new(tool_id: &str) -> Self {
        Self {
            tool_use_resp: tool_use_response(tool_id),
            text_resp: text_response("operation complete"),
            tool_use_sent: Mutex::new(false),
            tool_result_seen: Mutex::new(false),
        }
    }
}

impl LlmProvider for ApprovalTestProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        // Return tool_use on the first request that contains a user message,
        // then switch to text for subsequent calls (post-tool assistant turn).
        let already_sent = *self.tool_use_sent.lock().expect("lock");
        let tool_result_in_request = request.messages.iter().any(|m| {
            matches!(
                &m.content,
                hermeneus::types::Content::Blocks(blocks)
                    if blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
            )
        });
        let has_user_message = request.messages.iter().any(|m| m.role == Role::User);

        let response = if tool_result_in_request || already_sent {
            // Post-tool call: return text to complete the turn.
            *self.tool_result_seen.lock().expect("lock") = true;
            self.text_resp.clone()
        } else if has_user_message {
            // First main turn: return tool_use.
            *self.tool_use_sent.lock().expect("lock") = true;
            self.tool_use_resp.clone()
        } else {
            // Side query (recall ranking, etc.): return minimal text.
            self.text_resp.clone()
        };
        Box::pin(std::future::ready(Ok(response)))
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    // WHY: must return true so execute_streaming (which wires the approval gate)
    // is used instead of falling back to execute() which has no gate.
    fn supports_streaming(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "approval-test-mock"
    }
}

fn tool_use_response(tool_id: &str) -> CompletionResponse {
    CompletionResponse {
        id: "msg_tool".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::ToolUse,
        content: vec![ContentBlock::ToolUse {
            id: tool_id.to_owned(),
            name: "test_irreversible".to_owned(),
            input: serde_json::json!({}),
        }],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

fn text_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        id: "msg_text".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 15,
            output_tokens: 8,
            ..Usage::default()
        },
        cost_usd: None,
        duration_ms: None,
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// Read from a `TcpStream` until `session_id` is found in a `message_start`
/// SSE event, then send it over the channel and continue reading the rest.
///
/// WHY: `stream_turn` resolves the `session_id` internally from `(nous_id,
/// session_key)`. To know the exact ID registered in the `approval_registry`,
/// we must read the first SSE event (`message_start` carries `session_id`).
async fn read_sse_and_extract_session_id(
    stream: &mut TcpStream,
    session_id_tx: tokio::sync::oneshot::Sender<String>,
) -> (u16, String) {
    // Read the full response — we need it all.  The approval gate blocks the
    // turn, so the connection stays open until we POST the approval.  The
    // session_id_tx is sent as soon as we parse message_start from the
    // buffered bytes; the main task receives it and sends the approval POST.
    let mut buf = Vec::new();
    let mut sent_id = false;
    let mut session_id_tx = Some(session_id_tx);

    loop {
        let mut chunk = [0u8; 1024];
        let n = stream.read(&mut chunk).await.expect("stream read");
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);

        // Try to extract session_id once we have the header + first SSE event.
        if !sent_id {
            let so_far = String::from_utf8_lossy(&buf);
            if let Some((_head, body)) = so_far.split_once("\r\n\r\n") {
                // Look for session_id in message_start event data.
                for line in body.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        let parsed =
                            serde_json::from_str::<serde_json::Value>(data).ok().and_then(
                                |val| {
                                    val.get("session_id")
                                        .and_then(serde_json::Value::as_str)
                                        .map(ToOwned::to_owned)
                                },
                            );
                        if let Some(sid) = parsed {
                            if let Some(tx) = session_id_tx.take() {
                                let _ = tx.send(sid);
                            }
                            sent_id = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    let raw = String::from_utf8_lossy(&buf);
    let status = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();

    (status, body)
}

/// Parse SSE body into (`event_type`, `data_json_str`) pairs.
fn parse_sse_events(body: &str) -> Vec<(String, String)> {
    let mut events = Vec::new();
    let mut current_event = String::new();
    let mut current_data = String::new();

    for line in body.lines() {
        if let Some(ev) = line.strip_prefix("event: ") {
            ev.clone_into(&mut current_event);
        } else if let Some(data) = line.strip_prefix("data: ") {
            data.clone_into(&mut current_data);
        } else if line.is_empty() && !current_event.is_empty() {
            events.push((current_event.clone(), current_data.clone()));
            current_event.clear();
            current_data.clear();
        }
    }
    if !current_event.is_empty() {
        events.push((current_event, current_data));
    }
    events
}

fn event_types(events: &[(String, String)]) -> Vec<&str> {
    events.iter().map(|(t, _)| t.as_str()).collect()
}

/// Build a test harness wired with the synthetic irreversible tool and return
/// the executed flag so the test can assert whether the tool ran.
async fn build_approval_harness(
    tool_id: &str,
) -> (TestHarness, Arc<Mutex<bool>>) {
    // WHY: reqwest uses rustls-no-provider; the crypto provider must be
    // installed before the first TLS client is constructed.
    install_crypto_provider();
    let executed = Arc::new(Mutex::new(false));
    let exec_clone = Arc::clone(&executed);

    let provider = ApprovalTestProvider::new(tool_id);

    let mut registry = ToolRegistry::new();
    registry
        .register(
            irreversible_tool_def(),
            Box::new(ConfirmingExecutor { executed: exec_clone }),
        )
        .expect("register irreversible tool");

    let harness = TestHarness::build_with_provider_and_registry(Box::new(provider), registry).await;
    (harness, executed)
}

// ---------------------------------------------------------------------------
// Test scenarios
// ---------------------------------------------------------------------------

/// Irreversible tool call → operator approves → tool executes.
///
/// Asserts:
/// - `tool_approval_required` appears on the SSE stream before `tool_start`
/// - After `POST .../approvals` with `approved`, the stream proceeds to `tool_result`
/// - The executor flag is set (tool actually ran)
/// - `message_complete` terminates the stream
#[expect(clippy::too_many_lines, reason = "SSE e2e: setup + stream + poll-approve + SSE assertions")]
#[tokio::test]
async fn approval_required_approved_tool_executes() {
    let tool_id = "tu-approve-001";
    let (harness, executed) = build_approval_harness(tool_id).await;
    let (base_url, token, harness) = harness.start_tcp_server().await;
    let addr = &base_url; // e.g. "http://127.0.0.1:PORT"

    // WHY: Use a oneshot channel to receive the session_id from the SSE
    // stream's `message_start` event. `stream_turn` resolves the session_id
    // internally; we must read the first SSE event to know which id was
    // registered in the approval_registry.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest client");
    let (session_id_tx, session_id_rx) = tokio::sync::oneshot::channel::<String>();

    let stream_addr = addr.strip_prefix("http://").unwrap_or(addr).to_owned();
    let stream_token = token.clone();
    let nous_id = integration_tests::harness::TEST_NOUS_ID;
    let body_json = serde_json::json!({
        "nous_id": nous_id,
        "message": "run the thing",
        "session_key": "approval-approve-test"
    })
    .to_string();
    let body_len = body_json.len();
    // Spawn SSE stream task using `stream_turn` (POST /api/v1/sessions/stream).
    // WHY: /sessions/stream (not /sessions/{id}/messages) is the endpoint
    // that wires the approval gate — stream_turn registers the session_id in
    // the approval_registry before spawning the turn task.
    let sse_task = tokio::spawn(async move {
        let mut stream = TcpStream::connect(&stream_addr).await.expect("connect");
        let request = format!(
            "POST /api/v1/sessions/stream HTTP/1.1\r\n\
             Host: {stream_addr}\r\n\
             Authorization: Bearer {stream_token}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {body_len}\r\n\
             Connection: close\r\n\r\n\
             {body_json}",
        );
        stream.write_all(request.as_bytes()).await.expect("write");
        read_sse_and_extract_session_id(&mut stream, session_id_tx).await
    });

    // Wait for session_id from the first SSE event (`message_start`).
    let session_id = tokio::time::timeout(Duration::from_secs(10), session_id_rx)
        .await
        .expect("timeout waiting for session_id from SSE stream")
        .expect("session_id oneshot closed");

    // Poll until the approval gate blocks the turn, then approve.
    // WHY: The gate is registered before the turn task starts, but the pipeline
    // must run bootstrap/recall/execute before reaching `gate.await_decision`.
    // We retry the approval POST until `routed=true` or give up.
    let approve_body = {
        let mut result = None;
        for _ in 0..20u8 {
            let resp = client
                .post(format!("{addr}/api/v1/sessions/{session_id}/approvals"))
                .bearer_auth(&token)
                .json(&serde_json::json!({
                    "tool_id": tool_id,
                    "decision": "approved"
                }))
                .send()
                .await
                .expect("send approval");
            if resp.status().as_u16() == 200 {
                let body: serde_json::Value = resp.json().await.expect("approval json");
                if body["routed"].as_bool() == Some(true) {
                    result = Some(body);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        result.expect("approval not routed within timeout")
    };
    assert_eq!(
        approve_body["routed"], true,
        "approval must report routed=true when session is active"
    );

    // Wait for the SSE stream to complete (pipeline resumes after approval).
    let (status, sse_body) = tokio::time::timeout(Duration::from_secs(15), sse_task)
        .await
        .expect("sse task timeout")
        .expect("sse task join");

    assert_eq!(status, 200, "send_message must return 200");

    let events = parse_sse_events(&sse_body);
    let types = event_types(&events);

    assert!(
        types.contains(&"tool_approval_required"),
        "SSE stream must emit tool_approval_required; got: {types:?}"
    );
    assert!(
        types.contains(&"tool_result"),
        "SSE stream must emit tool_result after approval; got: {types:?}"
    );
    assert!(
        types.contains(&"message_complete"),
        "SSE stream must complete after approval; got: {types:?}"
    );

    // tool_approval_required must precede tool_start / tool_result.
    let req_pos = types
        .iter()
        .position(|&t| t == "tool_approval_required")
        .expect("approval_required position");
    let result_pos = types
        .iter()
        .position(|&t| t == "tool_result")
        .expect("tool_result position");
    assert!(
        req_pos < result_pos,
        "tool_approval_required must precede tool_result; order: {types:?}"
    );

    // Executor must have run.
    assert!(
        *executed.lock().expect("lock"),
        "approved tool must have executed"
    );

    // Harness kept alive until here so the server doesn't shut down.
    drop(harness);
}

/// Irreversible tool call → operator denies → tool does NOT execute.
///
/// Asserts:
/// - `tool_approval_required` appears on the SSE stream
/// - After `POST .../approvals` with `denied`, the stream emits a denial `tool_result`
/// - The executor flag is NOT set (tool did not run)
/// - `message_complete` terminates the stream
#[tokio::test]
async fn approval_required_denied_tool_does_not_execute() {
    let tool_id = "tu-deny-002";
    let (harness, executed) = build_approval_harness(tool_id).await;
    let (base_url, token, harness) = harness.start_tcp_server().await;
    let addr = &base_url;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest client");
    let (session_id_tx, session_id_rx) = tokio::sync::oneshot::channel::<String>();

    let stream_addr = addr.strip_prefix("http://").unwrap_or(addr).to_owned();
    let stream_token = token.clone();
    let nous_id = integration_tests::harness::TEST_NOUS_ID;
    let body_json = serde_json::json!({
        "nous_id": nous_id,
        "message": "run the thing",
        "session_key": "approval-deny-test"
    })
    .to_string();
    let body_len = body_json.len();
    let sse_task = tokio::spawn(async move {
        let mut stream = TcpStream::connect(&stream_addr).await.expect("connect");
        let request = format!(
            "POST /api/v1/sessions/stream HTTP/1.1\r\n\
             Host: {stream_addr}\r\n\
             Authorization: Bearer {stream_token}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {body_len}\r\n\
             Connection: close\r\n\r\n\
             {body_json}",
        );
        stream.write_all(request.as_bytes()).await.expect("write");
        read_sse_and_extract_session_id(&mut stream, session_id_tx).await
    });

    // Wait for session_id from the first SSE event, then deny.
    let session_id = tokio::time::timeout(Duration::from_secs(10), session_id_rx)
        .await
        .expect("timeout waiting for session_id from SSE stream")
        .expect("session_id oneshot closed");

    // Poll until the approval gate blocks, then deny.
    // WHY: same retry logic as approve — pipeline must reach gate before we can
    // route the decision. See approval_required_approved_tool_executes for rationale.
    for _ in 0..20u8 {
        let resp = client
            .post(format!("{addr}/api/v1/sessions/{session_id}/approvals"))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "tool_id": tool_id,
                "decision": "denied"
            }))
            .send()
            .await
            .expect("send denial");
        if resp.status().as_u16() == 200 {
            let body: serde_json::Value = resp.json().await.expect("denial json");
            if body["routed"].as_bool() == Some(true) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let (status, sse_body) = tokio::time::timeout(Duration::from_secs(15), sse_task)
        .await
        .expect("sse task timeout")
        .expect("sse task join");

    assert_eq!(status, 200);
    let events = parse_sse_events(&sse_body);
    let types = event_types(&events);

    assert!(
        types.contains(&"tool_approval_required"),
        "SSE stream must emit tool_approval_required; got: {types:?}"
    );
    assert!(
        types.contains(&"tool_result"),
        "SSE stream must emit a denial tool_result; got: {types:?}"
    );
    assert!(
        types.contains(&"message_complete"),
        "SSE stream must complete after denial; got: {types:?}"
    );

    // Executor must NOT have run.
    assert!(
        !*executed.lock().expect("lock"),
        "denied tool must NOT have executed"
    );

    // The tool_result SSE event should carry a denial message.
    let tool_result_event = events
        .iter()
        .find(|(t, _)| t == "tool_result")
        .expect("tool_result event");
    let tool_result_data: serde_json::Value =
        serde_json::from_str(&tool_result_event.1).expect("parse tool_result data");
    let result_text = tool_result_data["content"]
        .as_str()
        .or_else(|| tool_result_data["result"].as_str())
        .unwrap_or("");
    assert!(
        result_text.contains("denied") || tool_result_data["is_error"].as_bool() == Some(true),
        "denial tool_result must carry denial marker; data={tool_result_data}"
    );

    drop(harness);
}

/// Approvals POST to a session with no active turn returns 404.
///
/// Asserts the endpoint is not a no-op when no gate is registered —
/// a stale approval cannot silently succeed.
#[tokio::test]
async fn approvals_with_no_active_turn_returns_404() {
    install_crypto_provider();
    let harness = TestHarness::build().await;
    let (base_url, token, harness) = harness.start_tcp_server().await;
    let addr = &base_url;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client");

    // Use a session id that was never registered with the approval registry.
    let resp = client
        .post(format!("{addr}/api/v1/sessions/no-such-session-id/approvals"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "tool_id": "tu-ghost",
            "decision": "approved"
        }))
        .send()
        .await
        .expect("send request");

    assert_eq!(
        resp.status().as_u16(),
        404,
        "approvals POST with no active turn must return 404"
    );

    drop(harness);
}

/// Invalid `decision` value returns 422.
///
/// Pylon maps validation failures to 422 Unprocessable Entity. The approvals
/// handler validates `decision` before looking up the session, so the
/// error is surfaced regardless of whether a session exists.
#[tokio::test]
async fn approvals_with_invalid_decision_returns_422() {
    install_crypto_provider();
    let harness = TestHarness::build().await;
    let (base_url, token, harness) = harness.start_tcp_server().await;
    let addr = &base_url;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client");

    let resp = client
        .post(format!("{addr}/api/v1/sessions/any-session/approvals"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "tool_id": "tu-bad",
            "decision": "maybe"
        }))
        .send()
        .await
        .expect("send request");

    assert_eq!(
        resp.status().as_u16(),
        422,
        "approvals POST with invalid decision must return 422 (Unprocessable Entity)"
    );

    drop(harness);
}

/// Unauthenticated request to the approvals endpoint returns 401.
///
/// NOTE(#3958): The approval endpoint must enforce authentication so an
/// unauthenticated client cannot approve or deny another user's tool call.
#[tokio::test]
async fn approvals_unauthenticated_returns_401() {
    install_crypto_provider();
    let harness = TestHarness::build().await;
    let (base_url, _token, harness) = harness.start_tcp_server().await;
    let addr = &base_url;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client");

    let resp = client
        .post(format!("{addr}/api/v1/sessions/any-session/approvals"))
        // NOTE: no bearer token
        .json(&serde_json::json!({
            "tool_id": "tu-unauth",
            "decision": "approved"
        }))
        .send()
        .await
        .expect("send request");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "approvals POST without auth must return 401"
    );

    drop(harness);
}
