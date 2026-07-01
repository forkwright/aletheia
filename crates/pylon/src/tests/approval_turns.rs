use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hermeneus::test_utils::MockProvider;
use hermeneus::types::{CompletionResponse, ContentBlock, StopReason, Usage};
use koina::id::ToolName;
use organon::registry::{ToolExecutor, ToolRegistry};
use organon::types::{
    InputSchema, Reversibility, ToolCategory, ToolDef, ToolGroupId, ToolInput, ToolResult,
};
use tokio_stream::StreamExt;
use tower::ServiceExt;

use super::helpers::*;

const APPROVAL_TEST_TOOL: &str = "approval_test_tool";
const APPROVAL_TEST_TOOL_ID: &str = "toolu_approval_required";

struct CountingApprovalExecutor {
    executions: Arc<AtomicUsize>,
}

impl CountingApprovalExecutor {
    fn new(executions: Arc<AtomicUsize>) -> Self {
        Self { executions }
    }
}

impl ToolExecutor for CountingApprovalExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a organon::types::ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(ToolResult::text(format!(
                "executed {}",
                input.name.as_str()
            )))
        })
    }
}

fn collect_sse_data_events(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}

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

fn provider_tool_usage() -> Usage {
    Usage {
        input_tokens: 6,
        output_tokens: 3,
        cache_read_tokens: 2,
        cache_write_tokens: 1,
    }
}

fn provider_text_usage() -> Usage {
    Usage {
        input_tokens: 4,
        output_tokens: 2,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    }
}

fn approval_tool_response() -> CompletionResponse {
    CompletionResponse {
        id: "msg_approval_tool".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::ToolUse,
        content: vec![ContentBlock::ToolUse {
            id: APPROVAL_TEST_TOOL_ID.to_owned(),
            name: APPROVAL_TEST_TOOL.to_owned(),
            input: serde_json::json!({ "command": "side-effect" }),
        }],
        usage: provider_tool_usage(),
        cost_usd: None,
        duration_ms: None,
    }
}

fn provider_text_response() -> CompletionResponse {
    CompletionResponse {
        id: "msg_text_lifecycle".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: "Done.".to_owned(),
            citations: None,
        }],
        usage: provider_text_usage(),
        cost_usd: None,
        duration_ms: None,
    }
}

fn approval_tool_def() -> ToolDef {
    ToolDef {
        name: ToolName::new(APPROVAL_TEST_TOOL).expect("valid approval test tool name"),
        description: "Approval regression test tool".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: indexmap::IndexMap::default(),
            required: vec![],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::Irreversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Command],
        tags: vec![],
    }
}

fn approval_tool_registry(executions: Arc<AtomicUsize>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(
            approval_tool_def(),
            Box::new(CountingApprovalExecutor::new(executions)),
        )
        .expect("register approval test tool");
    registry
}

async fn approval_test_app(executions: Arc<AtomicUsize>) -> (axum::Router, tempfile::TempDir) {
    let provider =
        MockProvider::with_responses(vec![approval_tool_response(), provider_text_response()])
            .models(&["mock-model", "claude-opus-4-20250514"]);
    let registry = approval_tool_registry(executions);
    let (state, dir) = test_state_with_approval_test_tool(Some(Box::new(provider)), registry).await;
    (
        build_router(Arc::clone(&state), &test_security_config()),
        dir,
    )
}

fn pop_sse_data_event(buffer: &mut String, event_type: &str) -> Option<serde_json::Value> {
    loop {
        let frame_end = buffer.find("\n\n")?;
        let rest = buffer.split_off(frame_end);
        let frame = std::mem::take(buffer);
        *buffer = rest.chars().skip(2).collect();

        let mut data = String::new();
        for line in frame.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(value) = line.strip_prefix("data:") {
                data.push_str(value.trim_start());
            }
        }

        if data.is_empty() {
            continue;
        }

        let event = serde_json::from_str::<serde_json::Value>(data.trim())
            .expect("SSE data event must be JSON");
        if event["type"].as_str() == Some(event_type) {
            return Some(event);
        }
    }
}

async fn read_sse_data_event<S, E>(
    stream: &mut S,
    buffer: &mut String,
    event_type: &str,
) -> serde_json::Value
where
    S: tokio_stream::Stream<Item = Result<axum::body::Bytes, E>> + Unpin,
    E: std::fmt::Debug,
{
    loop {
        if let Some(event) = pop_sse_data_event(buffer, event_type) {
            return event;
        }

        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {event_type}; buffer={buffer}"))
            .expect("SSE stream ended before expected event")
            .expect("SSE body chunk");
        buffer.push_str(&String::from_utf8_lossy(&chunk));
    }
}

#[tokio::test]
async fn send_message_policy_denies_irreversible_tool_without_approval_gate() {
    let executions = Arc::new(AtomicUsize::new(0));
    let (router, _dir) = approval_test_app(Arc::clone(&executions)).await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "run approval test tool" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);
    let tool_use = find_sse_event(&events, "tool_use")
        .unwrap_or_else(|| panic!("legacy stream must record tool_use; body={body}"));
    assert_eq!(tool_use["id"], APPROVAL_TEST_TOOL_ID);
    assert_eq!(tool_use["name"], APPROVAL_TEST_TOOL);

    let tool_result = find_sse_event(&events, "tool_result")
        .unwrap_or_else(|| panic!("legacy stream must record tool_result; body={body}"));
    assert_eq!(tool_result["tool_use_id"], APPROVAL_TEST_TOOL_ID);
    assert_eq!(tool_result["is_error"], true);
    assert!(
        tool_result["content"]
            .as_str()
            .is_some_and(|content| content.contains("approval policy")),
        "legacy no-gate denial must explain approval policy; body={body}"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "legacy no-gate approval denial must skip execution"
    );
}

#[tokio::test]
async fn stream_turn_requires_operator_approval_for_irreversible_tool() {
    let executions = Arc::new(AtomicUsize::new(0));
    let (router, _dir) = approval_test_app(Arc::clone(&executions)).await;

    let resp = router
        .clone()
        .oneshot(stream_turn_req(
            "stream-approval-required",
            "run approval test tool",
            "01ARZ3NDEKTSV4RRFFQ69G5FBD",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let mut stream = resp.into_body().into_data_stream();
    let mut buffer = String::new();
    let start = read_sse_data_event(&mut stream, &mut buffer, "message_start").await;
    let session_id = start["session_id"]
        .as_str()
        .expect("message_start.session_id")
        .to_owned();
    let turn_id = start["turn_id"]
        .as_str()
        .expect("message_start.turn_id")
        .to_owned();

    let required = read_sse_data_event(&mut stream, &mut buffer, "tool_approval_required").await;
    assert_eq!(required["turn_id"], turn_id);
    assert_eq!(required["tool_name"], APPROVAL_TEST_TOOL);
    assert_eq!(required["tool_id"], APPROVAL_TEST_TOOL_ID);
    assert_eq!(required["risk"], "critical");
    assert!(
        required["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("mandatory approval")),
        "irreversible tool approval reason must name mandatory approval: {required}"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "tool must not execute before operator approval"
    );

    let approve_req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{session_id}/approvals"),
        Some(serde_json::json!({
            "turn_id": turn_id,
            "tool_id": APPROVAL_TEST_TOOL_ID,
            "decision": "approved",
        })),
    );
    let approve_resp = router.clone().oneshot(approve_req).await.unwrap();
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_body = body_json(approve_resp).await;
    assert_eq!(approve_body["routed"], true);

    let resolved = read_sse_data_event(&mut stream, &mut buffer, "tool_approval_resolved").await;
    assert_eq!(resolved["tool_id"], APPROVAL_TEST_TOOL_ID);
    assert_eq!(resolved["decision"], "approved");

    let tool_use = read_sse_data_event(&mut stream, &mut buffer, "tool_use").await;
    assert_eq!(tool_use["tool_name"], APPROVAL_TEST_TOOL);
    assert_eq!(tool_use["tool_id"], APPROVAL_TEST_TOOL_ID);

    let tool_result = read_sse_data_event(&mut stream, &mut buffer, "tool_result").await;
    assert_eq!(tool_result["tool_name"], APPROVAL_TEST_TOOL);
    assert_eq!(tool_result["tool_id"], APPROVAL_TEST_TOOL_ID);
    assert_eq!(tool_result["is_error"], false);
    assert!(
        tool_result["result"]
            .as_str()
            .is_some_and(|result| result.contains(APPROVAL_TEST_TOOL)),
        "approved tool result should include executor output: {tool_result}"
    );

    let complete = read_sse_data_event(&mut stream, &mut buffer, "message_complete").await;
    assert_eq!(complete["outcome"]["tool_calls"], 1);
    assert_eq!(complete["outcome"]["stop_reason"], "end_turn");
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "approved irreversible tool should execute exactly once"
    );
}
