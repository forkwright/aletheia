//! Approval-guard integration tests for shared tool dispatch (#3958, ADR-005).
#![expect(
    clippy::indexing_slicing,
    reason = "test: indices valid after asserting `len`"
)]

use std::time::Duration;

use koina::id::ToolName;
use organon::registry::ToolRegistry;
use organon::types::Reversibility;
use tokio::sync::mpsc;

use super::*;
use crate::approval::{ApprovalChoice, ApprovalDecision, ApprovalGate, ApprovalPostures};
use crate::execute::dispatch::{ToolDispatchPolicy, dispatch_tools};
use crate::pipeline::LoopDetector;
use crate::stream::TurnStreamEvent;
use taxis::config::ApprovalPosture;

fn allow_active_for_tests(
    registry: &ToolRegistry,
    active: impl IntoIterator<Item = ToolName>,
) -> ToolDispatchPolicy {
    let active: std::collections::HashSet<ToolName> = active.into_iter().collect();
    let policy = organon::types::ToolGroupPolicy::AllowAll {
        reason: "execute test helper".to_owned(),
    };
    ToolDispatchPolicy::new(Arc::new(registry.effective_surface(
        organon::surface::SurfaceInputs {
            policy: &policy,
            allowlist: None,
            active: &active,
            server_tools: &[],
            server_tool_config: None,
        },
    )))
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
async fn unknown_tool_is_denied_before_approval_routing() {
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let (_decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "ghost_tool".to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        Some(&gate),
        &policy,
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(all_calls.len(), 1);
    assert!(all_calls[0].is_error);
    assert!(
        all_calls[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .starts_with("unknown_tool:")
    );

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    assert_event_kinds(&events, &["tool_result"]);
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
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        Some(&gate),
        &policy,
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
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        Some(&gate),
        &policy,
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
    // WHY(#5827): is_error alone cannot distinguish this from a tool that ran and
    // failed. `unexecuted` is what carries that distinction out to the after_tool loop.
    assert_eq!(
        result.unexecuted,
        vec!["tool-1".to_owned()],
        "a denied call must be reported as never executed"
    );
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
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        None,
        &policy,
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(all_calls.len(), 1);
    assert!(all_calls[0].is_error, "mandatory without gate must deny");
    assert!(
        all_calls[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("approval policy"),
        "no-gate denial must be recorded as policy, not user denial"
    );

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    // WHY(#7252): no gate means no approver exists, so no `approval_required`
    // request is ever emitted — the typed denial is the whole signal.
    assert_event_kinds(&events, &["approval_resolved", "tool_result"]);
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[0] {
        assert_eq!(decision, "no_gate_denied");
    } else {
        panic!("expected ToolApprovalResolved at idx 0");
    }
}

#[tokio::test]
async fn required_without_gate_defaults_to_denial() {
    let tools = make_registry_rev("write_file", Reversibility::PartiallyReversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "write_file".to_owned(),
        serde_json::json!({"path": "notes.md"}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        None,
        &policy,
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(all_calls.len(), 1);
    assert!(
        all_calls[0].is_error,
        "required approval without gate must deny"
    );
    assert!(
        all_calls[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("approval policy"),
        "no-gate denial must be recorded as policy, not user denial"
    );

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    // WHY(#7252): no gate ⇒ no request event — see mandatory_without_gate.
    assert_event_kinds(&events, &["approval_resolved", "tool_result"]);
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[0] {
        assert_eq!(decision, "no_gate_denied");
    } else {
        panic!("expected ToolApprovalResolved at idx 0");
    }
}

#[tokio::test]
async fn sessions_spawn_without_gate_defaults_to_denial() {
    let mut tools = ToolRegistry::new();
    organon::builtins::register_all(&mut tools).expect("register builtins");
    let sessions_spawn = ToolName::from_static("sessions_spawn");
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);

    let tool_uses = vec![(
        "tool-1".to_owned(),
        sessions_spawn.as_str().to_owned(),
        serde_json::json!({"role": "coder", "task": "touch the workspace"}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();
    let policy = allow_active_for_tests(&tools, [sessions_spawn]);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        None,
        &policy,
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(all_calls.len(), 1);
    assert!(
        all_calls[0].is_error,
        "sessions_spawn without gate must deny"
    );

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    // WHY(#7252): no gate ⇒ no request event — see mandatory_without_gate.
    assert_event_kinds(&events, &["approval_resolved", "tool_result"]);
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[0] {
        assert_eq!(decision, "no_gate_denied");
    } else {
        panic!("expected ToolApprovalResolved at idx 0");
    }
}

#[tokio::test]
async fn batch_no_gate_policy_covers_all_approval_requirements() {
    for (tool_name, reversibility, should_error, expected_result) in [
        (
            "read_file",
            Reversibility::FullyReversible,
            false,
            "executed: read_file",
        ),
        (
            "replace_file",
            Reversibility::Reversible,
            false,
            "executed: replace_file",
        ),
        (
            "delete_file",
            Reversibility::PartiallyReversible,
            true,
            "approval policy",
        ),
        (
            "exec_remote",
            Reversibility::Irreversible,
            true,
            "approval policy",
        ),
    ] {
        let tools = make_registry_rev(tool_name, reversibility);
        let tool_uses = vec![(
            "tool-1".to_owned(),
            tool_name.to_owned(),
            serde_json::json!({}),
        )];
        let mut loop_detector = LoopDetector::new(3);
        let mut all_calls = Vec::new();
        let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

        let result = dispatch_tools(
            &tool_uses,
            &tools,
            &test_tool_ctx(),
            &mut loop_detector,
            &mut all_calls,
            1,
            None,
            None,
            &policy,
            0,
            None,
            None,
        )
        .await
        .expect("dispatch ok");

        assert_eq!(result.blocks.len(), 1);
        assert_eq!(all_calls.len(), 1);
        assert_eq!(
            all_calls[0].is_error, should_error,
            "{tool_name} no-gate policy mismatch"
        );
        assert!(
            all_calls[0]
                .result
                .as_deref()
                .unwrap_or_default()
                .contains(expected_result),
            "{tool_name} result should contain {expected_result:?}"
        );
    }
}

#[tokio::test]
async fn batch_dispatch_mandatory_without_gate_matches_streaming_denial_record() {
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut batch_detector = LoopDetector::new(3);
    let mut batch_calls = Vec::new();
    let mut streaming_detector = LoopDetector::new(3);
    let mut streaming_calls = Vec::new();
    let (event_tx, _event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let batch_result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut batch_detector,
        &mut batch_calls,
        1,
        None,
        None,
        &policy,
        0,
        None,
        None,
    )
    .await
    .expect("batch dispatch ok");

    let streaming_result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut streaming_detector,
        &mut streaming_calls,
        1,
        Some(&event_tx),
        None,
        &policy,
        0,
        None,
        None,
    )
    .await
    .expect("streaming dispatch ok");

    assert_eq!(batch_result.blocks.len(), streaming_result.blocks.len());
    assert_eq!(batch_calls.len(), streaming_calls.len());
    assert_eq!(batch_calls[0].name, streaming_calls[0].name);
    assert_eq!(batch_calls[0].input, streaming_calls[0].input);
    assert_eq!(batch_calls[0].is_error, streaming_calls[0].is_error);
    assert_eq!(batch_calls[0].result, streaming_calls[0].result);
}

/// WHY(#7252): the daemon's turn shape — no stream, no gate (`NousMessage::Turn`
/// carries neither). An approval-required call must produce the typed
/// `no_gate_denied` refusal as its whole signal: no request event can exist
/// because no approver does, and the refusal rides back into the turn so the
/// agent can adapt instead of the call dangling.
#[tokio::test]
async fn daemon_shaped_turn_denies_without_any_approval_request() {
    let tools = make_registry_rev("exec", Reversibility::Irreversible);

    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        None,
        None,
        &policy,
        0,
        None,
        None,
    )
    .await
    .expect("dispatch ok");

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(all_calls.len(), 1);
    assert!(all_calls[0].is_error, "no-gate mandatory call must deny");
    assert_eq!(
        all_calls[0].approval.as_deref(),
        Some("no_gate_denied"),
        "the typed refusal must be recorded on the call"
    );
    assert!(
        all_calls[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("approval policy"),
        "the refusal text must reach the turn, got: {:?}",
        all_calls[0].result
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
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        None,
        &policy,
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
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let _ = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        None,
        &policy,
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
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        Some(&gate),
        &policy,
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

// ── Config-driven approval posture (toolApproval*Policy) ────────────────
//
// `dispatch_tools` always passes `ApprovalPostures::default()`; exercising
// the relaxed posture requires the real dispatch boundary, so these tests
// call `dispatch_tool_items` directly with an explicit posture pair.
async fn dispatch_with_postures(
    tool_uses: &[(String, String, serde_json::Value)],
    tools: &ToolRegistry,
    approval_gate: Option<&ApprovalGate>,
    postures: ApprovalPostures,
    policy: &ToolDispatchPolicy,
    all_calls: &mut Vec<crate::pipeline::ToolCall>,
    stream_tx: Option<&mpsc::Sender<TurnStreamEvent>>,
) -> crate::execute::dispatch::DispatchResult {
    let items: Vec<_> = tool_uses
        .iter()
        .cloned()
        .map(crate::execute::dispatch::ToolDispatchItem::from)
        .collect();
    let identity = crate::stream::TurnEventIdentity {
        turn_id: koina::ulid::Ulid::new(),
        session_id: "test-session".to_owned(),
        request_id: None,
        turn_number: 0,
        client_turn_id: None,
    };
    let signer = organon::receipts::ReceiptSigner::new_session();
    let mut loop_detector = LoopDetector::new(3);
    crate::execute::dispatch::dispatch_tool_items(
        &items,
        tools,
        &test_tool_ctx(),
        &mut loop_detector,
        all_calls,
        1,
        stream_tx,
        approval_gate,
        postures,
        policy,
        0,
        &signer,
        None,
        &identity,
    )
    .await
    .expect("dispatch ok")
}

#[tokio::test]
async fn policy_auto_approve_executes_mandatory_tool_without_gate() {
    // The daemon/REST shape: no approval gate is attached to the turn. With
    // the mandatory tier configured `auto_approve`, a critical-risk tool must
    // execute instead of failing closed — and the execution must still carry
    // its audit record.
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let postures = ApprovalPostures {
        required: ApprovalPosture::Gate,
        mandatory: ApprovalPosture::AutoApprove,
    };
    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_with_postures(
        &tool_uses,
        &tools,
        None,
        postures,
        &policy,
        &mut all_calls,
        Some(&event_tx),
    )
    .await;

    assert_eq!(result.blocks.len(), 1);
    assert_eq!(all_calls.len(), 1);
    assert!(
        !all_calls[0].is_error,
        "auto-approved mandatory call must execute, got: {:?}",
        all_calls[0].result
    );
    assert!(
        all_calls[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("executed: exec"),
        "the real executor must have run: {:?}",
        all_calls[0].result
    );
    assert!(
        result.unexecuted.is_empty(),
        "an auto-approved call must not be reported as unexecuted"
    );

    // The audit trail: durable approval outcome + HMAC receipt on the
    // persisted tool call.
    assert_eq!(
        all_calls[0].approval.as_deref(),
        Some("policy_auto_approved"),
        "the durable record must mark this call as policy-auto-approved"
    );
    assert!(
        all_calls[0].receipt.is_some(),
        "auto-approved execution must still be attested by a receipt"
    );

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    // No approval_required: there was never a decision to ask for. The
    // resolution is still surfaced so a watching client sees the policy
    // outcome rather than silence.
    assert_event_kinds(&events, &["approval_resolved", "tool_start", "tool_result"]);
    if let TurnStreamEvent::ToolApprovalResolved { decision, .. } = &events[0] {
        assert_eq!(decision, "policy_auto_approved");
    } else {
        panic!("expected policy_auto_approved resolution");
    }
}

#[tokio::test]
async fn policy_auto_approve_executes_required_tool_without_gate() {
    let tools = make_registry_rev("write_file", Reversibility::PartiallyReversible);
    let postures = ApprovalPostures {
        required: ApprovalPosture::AutoApprove,
        mandatory: ApprovalPosture::Gate,
    };
    let tool_uses = vec![(
        "tool-1".to_owned(),
        "write_file".to_owned(),
        serde_json::json!({"path": "notes.md"}),
    )];
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_with_postures(
        &tool_uses,
        &tools,
        None,
        postures,
        &policy,
        &mut all_calls,
        None,
    )
    .await;

    assert_eq!(all_calls.len(), 1);
    assert!(
        !all_calls[0].is_error,
        "auto-approved required call must execute"
    );
    assert_eq!(
        all_calls[0].approval.as_deref(),
        Some("policy_auto_approved")
    );
    assert!(result.unexecuted.is_empty());
}

#[tokio::test]
async fn required_tier_relaxation_does_not_relax_mandatory() {
    // Per-tier independence: relaxing the required tier must leave a
    // mandatory call fail-closed when no gate is wired.
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let postures = ApprovalPostures {
        required: ApprovalPosture::AutoApprove,
        mandatory: ApprovalPosture::Gate,
    };
    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_with_postures(
        &tool_uses,
        &tools,
        None,
        postures,
        &policy,
        &mut all_calls,
        None,
    )
    .await;

    assert_eq!(all_calls.len(), 1);
    assert!(
        all_calls[0].is_error,
        "a gated mandatory call with no gate must still deny"
    );
    assert_eq!(
        all_calls[0].approval.as_deref(),
        Some("no_gate_denied"),
        "the denial must record the no-gate outcome, not the posture"
    );
    assert_eq!(result.unexecuted, vec!["tool-1".to_owned()]);
}

#[tokio::test]
async fn auto_approve_short_circuits_even_when_gate_wired() {
    // The streaming/desktop shape: a gate IS attached, but the configured
    // posture is consulted first — the gate must never be awaited. A gate
    // that no one answers would deny after its timeout; the auto path must
    // resolve immediately without consulting it.
    let tools = make_registry_rev("exec", Reversibility::Irreversible);
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let (_decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_millis(50));
    let postures = ApprovalPostures {
        required: ApprovalPosture::Gate,
        mandatory: ApprovalPosture::AutoApprove,
    };
    let tool_uses = vec![(
        "tool-1".to_owned(),
        "exec".to_owned(),
        serde_json::json!({}),
    )];
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    let result = dispatch_with_postures(
        &tool_uses,
        &tools,
        Some(&gate),
        postures,
        &policy,
        &mut all_calls,
        Some(&event_tx),
    )
    .await;

    assert!(
        !all_calls[0].is_error,
        "auto-approved call must execute without awaiting the gate"
    );
    assert_eq!(
        all_calls[0].approval.as_deref(),
        Some("policy_auto_approved")
    );

    drop(event_tx);
    let events = drain_events(&mut event_rx);
    assert_event_kinds(&events, &["approval_resolved", "tool_start", "tool_result"]);
    assert!(
        matches!(result.blocks.len(), 1),
        "one tool result block expected"
    );
}
