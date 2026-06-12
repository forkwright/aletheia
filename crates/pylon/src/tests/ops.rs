//! Integration tests for ops tool endpoints.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::{collections::HashSet, path::PathBuf};

use axum::http::StatusCode;
use koina::id::{NousId, SessionId, ToolName};
use organon::error::Result;
use organon::registry::ToolExecutor;
use organon::registry::ToolRegistry;
use organon::types::{InputSchema, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolResult};
use taxis::config::ToolLimitsConfig;
use tower::ServiceExt;

use super::helpers::*;

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

#[tokio::test]
async fn get_ops_tools_returns_registry_and_metrics() {
    let (state, _dir) = test_state().await;
    let mut state = Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique app state"));

    let tool_name = ToolName::new("probe_tool").expect("valid tool name");
    let tool_def = ToolDef {
        name: tool_name.clone(),
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
    };

    let mut registry = ToolRegistry::new();
    registry
        .register(tool_def, Box::new(ProbeExecutor))
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
        body["history_unavailable"]
            .as_bool()
            .expect("history_unavailable bool"),
        "tool history must be marked unavailable until persisted history exists"
    );
}
