//! Integration tests for ops tool endpoints.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::{collections::HashSet, path::PathBuf};

use axum::http::StatusCode;
use koina::id::{NousId, SessionId, ToolName};
use mneme::store::{FinalizeMessage, FinalizeToolAuditRecord, FinalizeTurnRequest};
use mneme::types::{Role as MnemeRole, UsageRecord};
use organon::error::Result;
use organon::registry::ToolExecutor;
use organon::registry::ToolRegistry;
use organon::types::{InputSchema, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolResult};
use taxis::config::ToolLimitsConfig;
use tower::ServiceExt;

use super::helpers::*;

const OPS_TOOL_HISTORY_SESSION_ID: &str = "ops-tool-history-session";

struct ProbeExecutor;

impl ToolExecutor for ProbeExecutor {
    fn execute<'a>(
        &'a self,
        _input: &'a organon::types::ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async { Ok(ToolResult::text("ok")) })
    }
}

fn probe_tool_def(name: ToolName) -> ToolDef {
    ToolDef {
        name,
        description: "Probe tool for ops tests.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: vec![].into_iter().collect(),
            required: Vec::new(),
        },
        category: ToolCategory::Workspace,
        reversibility: organon::types::Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![organon::types::ToolTag::Recon],
    }
}

fn tool_history_usage() -> UsageRecord {
    UsageRecord {
        session_id: OPS_TOOL_HISTORY_SESSION_ID.to_owned(),
        turn_seq: 11,
        input_tokens: 1,
        output_tokens: 1,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        model: Some("test-model".to_owned()),
    }
}

fn tool_history_messages() -> Vec<FinalizeMessage<'static>> {
    vec![FinalizeMessage {
        role: MnemeRole::Assistant,
        content: "tool audit turn",
        tool_call_id: None,
        tool_name: None,
        token_estimate: 1,
    }]
}

fn tool_history_audits() -> Vec<FinalizeToolAuditRecord<'static>> {
    vec![
        FinalizeToolAuditRecord {
            turn_seq: 11,
            tool_call_id: "toolu_failed",
            tool_name: "probe_tool",
            duration_ms: 12,
            is_error: true,
            outcome: "error",
            result: Some("Tool error: failed"),
            approval: Some("auto_approved"),
            receipt: None,
        },
        FinalizeToolAuditRecord {
            turn_seq: 11,
            tool_call_id: "toolu_approved",
            tool_name: "probe_tool",
            duration_ms: 18,
            is_error: false,
            outcome: "success",
            result: Some("ok"),
            approval: Some("approved"),
            receipt: None,
        },
        FinalizeToolAuditRecord {
            turn_seq: 11,
            tool_call_id: "toolu_receipt",
            tool_name: "probe_tool",
            duration_ms: 20,
            is_error: false,
            outcome: "success",
            result: Some("ok\n\n[receipt:receipt-token]"),
            approval: Some("auto_approved"),
            receipt: Some("receipt-token"),
        },
    ]
}

async fn seed_tool_history(state: &crate::state::AppState) {
    let store = state.session_store.lock().await;
    let usage = tool_history_usage();
    let messages = tool_history_messages();
    let audits = tool_history_audits();
    store
        .finalize_turn(&FinalizeTurnRequest {
            session_id: OPS_TOOL_HISTORY_SESSION_ID,
            nous_id: "alice",
            session_key: "ops-tool-history",
            model: Some("test-model"),
            parent_session_id: None,
            messages: &messages,
            usage: Some(&usage),
            tool_audit_records: &audits,
            completion_note: None,
        })
        .expect("finalize tool audit records");
}

#[tokio::test]
async fn get_ops_tools_returns_registry_and_metrics() {
    let (state, _dir) = test_state().await;
    let mut state = Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique app state"));

    let tool_name = ToolName::new("probe_tool").expect("valid tool name");

    let mut registry = ToolRegistry::new();
    registry
        .register(probe_tool_def(tool_name.clone()), Box::new(ProbeExecutor))
        .expect("register tool");
    state.tool_registry = Arc::new(registry);
    let state = Arc::new(state);

    let ctx = ToolContext {
        nous_id: NousId::new("alice").expect("valid nous id"),
        session_id: SessionId::new(),
        turn_number: 0,
        workspace: PathBuf::from("/tmp/aletheia-test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(std::sync::RwLock::new(HashSet::new())),
        tool_config: Arc::new(ToolLimitsConfig::default()),
    };
    let input = organon::types::ToolInput {
        name: tool_name,
        tool_use_id: "tu_test_00000".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = state
        .tool_registry
        .execute(&input, &ctx)
        .await
        .expect("tool execution");
    assert!(!result.is_error);

    seed_tool_history(&state).await;

    let app = build_router(state, &test_security_config());
    let resp = app.oneshot(authed_get("/api/v1/ops/tools")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let catalog = body["catalog"].as_array().expect("catalog array");
    assert!(catalog.iter().any(|tool| tool["id"] == "probe_tool"));
    assert!(
        body["live_invocations"].as_array().is_some(),
        "live invocation list must be present"
    );
    assert!(
        body["total_calls"].as_u64().expect("total_calls as u64") >= 1,
        "total calls should include the probe execution"
    );
    let total_errors = body["total_errors"].as_u64().expect("total_errors as u64");
    assert!(
        total_errors <= body["total_calls"].as_u64().expect("total_calls as u64"),
        "error calls cannot exceed total calls"
    );
    assert!(
        !body["history_unavailable"]
            .as_bool()
            .expect("history_unavailable bool"),
        "tool history should be available when audit records can be read"
    );
    let history = body["history"].as_array().expect("history array");
    assert!(
        history
            .iter()
            .any(|entry| entry["tool_call_id"] == "toolu_failed"
                && entry["is_error"] == true
                && entry["outcome"] == "error"),
        "ops history should include failed tool calls; body={body}"
    );
    assert!(
        history
            .iter()
            .any(|entry| entry["tool_call_id"] == "toolu_approved"
                && entry["approval"] == "approved"),
        "ops history should include approved tool calls; body={body}"
    );
    assert!(
        history
            .iter()
            .any(|entry| entry["tool_call_id"] == "toolu_receipt"
                && entry["receipt_state"] == "present"
                && entry["receipt"] == "receipt-token"),
        "ops history should include receipt-bearing tool calls; body={body}"
    );
}
