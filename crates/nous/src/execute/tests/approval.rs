//! Approval-guard integration tests for `dispatch_tools_streaming` (#3958, ADR-005).
//!
//! These exercise the gate at the dispatch layer directly because the streaming
//! `execute_streaming` entry point requires a streaming-capable provider, which
//! the workspace's `MockProvider` doesn't supply. The gate logic itself lives in
//! `dispatch_tools_streaming` and is the unit under test.
#![expect(
    clippy::indexing_slicing,
    reason = "test: indices valid after asserting `len`"
)]

use std::time::Duration;

use koina::id::ToolName;
use organon::registry::ToolRegistry;
use organon::types::{InputSchema, Reversibility, ToolCategory, ToolDef};
use tokio::sync::mpsc;

use super::*;
use crate::approval::{ApprovalChoice, ApprovalDecision, ApprovalGate};
use crate::execute::dispatch::dispatch_tools_streaming;
use crate::pipeline::LoopDetector;
use crate::stream::TurnStreamEvent;

fn tool_def_with(name: &str, rev: Reversibility) -> ToolDef {
    ToolDef {
        name: ToolName::new(name).expect("valid"),
        description: format!("Test tool: {name}"),
        extended_description: None,
        input_schema: InputSchema {
            properties: indexmap::IndexMap::default(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: rev,
        auto_activate: false,
        groups: vec![organon::types::ToolGroupId::Read],
        tags: vec![],
    }
}

fn make_registry_rev(name: &str, rev: Reversibility) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(tool_def_with(name, rev), Box::new(EchoExecutor))
        .expect("register");
    registry
}

fn drain_events(rx: &mut mpsc::Receiver<TurnStreamEvent>) -> Vec<TurnStreamEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

fn assert_event_kinds(events: &[TurnStreamEvent], expected: &[&str]) {
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            TurnStreamEvent::ToolApprovalRequired { .. } => "approval_required",
            TurnStreamEvent::ToolApprovalResolved { .. } => "approval_resolved",
            TurnStreamEvent::ToolStart { .. } => "tool_start",
            TurnStreamEvent::ToolResult { .. } => "tool_result",
            TurnStreamEvent::LlmDelta(_) => "llm_delta",
        })
        .collect();
    assert_eq!(
        kinds, expected,
        "event kind sequence mismatch — got {kinds:?}, expected {expected:?}"
    );
}

#[tokio::test]
async fn reversibility_class_call_blocks_until_approved() {
    // Mandatory tool (Reversibility::Irreversible) with an approval gate.
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));

    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Approved,
        })
        .await
        .expect("send approval");

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();

    let result = dispatch_tools_streaming(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        &event_tx,
        Some(&gate),
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1, "approved call produces one result");
    assert_eq!(all_calls.len(), 1);
    assert!(!all_calls[0].is_error, "approved call must not be error");

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    assert_event_kinds(
        &events,
        &[
            "approval_required",
            "approval_resolved",
            "tool_start",
            "tool_result",
        ],
    );
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[1] {
        assert_eq!(decision, "approved");
    } else {
        panic!("expected ToolApprovalResolved at idx 1");
    }
}

#[tokio::test]
async fn reversibility_class_call_denied_skips_execution() {
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));

    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Denied,
        })
        .await
        .expect("send denial");

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();

    let result = dispatch_tools_streaming(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        &event_tx,
        Some(&gate),
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(
        result.blocks.len(),
        1,
        "denied call produces a denial block"
    );
    assert_eq!(all_calls.len(), 1);
    assert!(all_calls[0].is_error, "denied call must be marked error");
    assert!(
        all_calls[0]
            .result
            .as_deref()
            .unwrap_or("")
            .contains("denied by user"),
        "denial message must be present"
    );

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    assert_event_kinds(
        &events,
        &["approval_required", "approval_resolved", "tool_result"],
    );
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[1] {
        assert_eq!(decision, "denied");
    } else {
        panic!("expected ToolApprovalResolved at idx 1");
    }
}

#[tokio::test]
async fn mandatory_without_gate_defaults_to_denial() {
    // No approval_gate wired + Mandatory requirement → must deny (ADR-005 step 4).
    // This is the contract that closes the v1.0.0 hole: a Mandatory tool can
    // never silently execute when there is no operator to ask.
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();

    let result = dispatch_tools_streaming(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        &event_tx,
        None,
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(all_calls.len(), 1);
    assert!(all_calls[0].is_error, "mandatory without gate must deny");

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    // approval_required → approval_resolved(denied) → tool_result(denial)
    assert_event_kinds(
        &events,
        &["approval_required", "approval_resolved", "tool_result"],
    );
}

#[tokio::test]
async fn safe_call_proceeds_without_gate() {
    // FullyReversible → ApprovalRequirement::None → auto-approve, execute.
    let tools = make_registry_rev("read", Reversibility::FullyReversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "read".to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();

    let result = dispatch_tools_streaming(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        &event_tx,
        None,
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(all_calls.len(), 1);
    assert!(!all_calls[0].is_error);

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    // No ToolApprovalRequired; just an auto-resolution then execution.
    assert_event_kinds(&events, &["approval_resolved", "tool_start", "tool_result"]);
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[0] {
        assert_eq!(decision, "auto_approved");
    } else {
        panic!("expected auto_approved");
    }
}

#[tokio::test]
async fn advisory_call_executes_without_approval_required_event() {
    // Reversible → ApprovalRequirement::Advisory → execute, recorded as advisory_auto.
    let tools = make_registry_rev("write", Reversibility::Reversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "write".to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();

    let _ = dispatch_tools_streaming(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        &event_tx,
        None,
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    assert_event_kinds(&events, &["approval_resolved", "tool_start", "tool_result"]);
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[0] {
        assert_eq!(decision, "advisory_auto");
    } else {
        panic!("expected advisory_auto");
    }
}

#[tokio::test]
async fn gate_timeout_denies_mandatory_call() {
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let (_decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_millis(100));

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();

    let result = dispatch_tools_streaming(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        &event_tx,
        Some(&gate),
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1);
    assert!(all_calls[0].is_error, "timeout must produce denial");
    drop(event_tx);
    let events = drain_events(&mut event_rx);
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[1] {
        assert_eq!(decision, "denied", "timeout maps to denied wire string");
    } else {
        panic!("expected approval_resolved at idx 1");
    }
}
