//! Declared-redaction enforcement tests (#6808): the registry's declared
//! `RedactionPolicy` is applied at the dispatch boundary to every surface
//! that leaves the executor loop — the persisted `ToolCall` record, the
//! stream events (approval prompt, tool start, tool result), and the
//! receipt ledger — while the executor still sees the real arguments.

use std::time::Duration;

use koina::id::ToolName;
use organon::registry::ToolRegistry;
use organon::types::{RedactionPolicy, Reversibility, ToolCapabilityMetadata};
use tokio::sync::mpsc;

use super::*;
use crate::approval::{ApprovalChoice, ApprovalDecision, ApprovalGate};
use crate::execute::dispatch::{ToolDispatchPolicy, dispatch_tools};
use crate::pipeline::LoopDetector;
use crate::stream::TurnStreamEvent;

fn registry_with_redaction(
    name: &str,
    reversibility: Reversibility,
    redaction: RedactionPolicy,
) -> ToolRegistry {
    let mut registry = make_registry_rev(name, reversibility);
    registry.declare_capability(
        ToolName::new(name).expect("valid test tool name"),
        ToolCapabilityMetadata {
            owner: "nous::execute::tests".to_owned(),
            redaction,
            ..ToolCapabilityMetadata::default()
        },
    );
    registry
}

struct DispatchOutcome {
    calls: Vec<crate::pipeline::ToolCall>,
    events: Vec<TurnStreamEvent>,
}

async fn dispatch_one(
    tools: &ToolRegistry,
    tool_name: &str,
    input: serde_json::Value,
    approval_gate: Option<&ApprovalGate>,
    receipt_ledger: Option<&std::sync::Mutex<organon::receipts::ReceiptLedger>>,
) -> DispatchOutcome {
    let (event_tx, mut event_rx) = mpsc::channel::<TurnStreamEvent>(64);
    let tool_uses = vec![("tool-1".to_owned(), tool_name.to_owned(), input)];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(tools);

    dispatch_tools(
        &tool_uses,
        tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        Some(&event_tx),
        approval_gate,
        &policy,
        0,
        None,
        receipt_ledger,
    )
    .await
    .expect("dispatch ok");

    drop(event_tx);
    let mut events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }
    DispatchOutcome {
        calls: all_calls,
        events,
    }
}

#[tokio::test]
async fn full_policy_redacts_recorded_input_and_result() {
    let tool = "_test_redaction_full";
    // WHY these values: short, whitespace-bearing, prose-like — the
    // hermeneus content heuristic (long, whitespace-free strings) would NOT
    // catch them, so any redaction observed is the declared policy's alone.
    let input = serde_json::json!({"note": "standup moved to nine", "count": 2});
    let tools =
        registry_with_redaction(tool, Reversibility::FullyReversible, RedactionPolicy::Full);

    let outcome = dispatch_one(&tools, tool, input, None, None).await;

    assert_eq!(outcome.calls.len(), 1);
    let call = &outcome.calls[0];
    assert_eq!(
        call.input,
        serde_json::json!({"note": "[REDACTED]", "count": "[REDACTED]"}),
        "Full redacts every input leaf in the persisted record"
    );
    assert_eq!(
        call.result.as_deref(),
        Some("[REDACTED]"),
        "Full redacts the recorded result text"
    );
    // The LLM-facing result block is deliberately NOT redacted: the model
    // mid-turn needs the real output; the policy governs the trace.
    let tool_result = outcome
        .events
        .iter()
        .find_map(|e| match e {
            TurnStreamEvent::ToolResult { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("tool result event emitted");
    assert_eq!(
        tool_result, "[REDACTED]",
        "the stream-side result is a trace surface and follows the policy"
    );
}

#[tokio::test]
async fn fields_policy_redacts_named_field_and_passes_others() {
    let tool = "_test_redaction_fields";
    let input = serde_json::json!({
        "url": "https://acme.corp/api",
        "headers": {"PRIVATE-TOKEN": "tok-value"},
    });
    let tools = registry_with_redaction(
        tool,
        Reversibility::FullyReversible,
        RedactionPolicy::Fields(vec!["headers".to_owned()]),
    );

    let outcome = dispatch_one(&tools, tool, input, None, None).await;

    let call = &outcome.calls[0];
    assert_eq!(
        call.input["headers"],
        serde_json::json!("[REDACTED]"),
        "declared field redacted in the persisted record"
    );
    assert_eq!(
        call.input["url"],
        serde_json::json!("https://acme.corp"),
        "undeclared field passes through"
    );
    assert!(
        call.result
            .as_deref()
            .unwrap_or_default()
            .contains("executed:"),
        "Fields is argument-scoped: the recorded result passes through, got {:?}",
        call.result
    );
}

#[tokio::test]
async fn none_policy_passes_input_and_result_through() {
    let tool = "_test_redaction_none";
    let input = serde_json::json!({"note": "plain value", "count": 2});
    let tools = make_registry_rev(tool, Reversibility::FullyReversible);

    let outcome = dispatch_one(&tools, tool, input.clone(), None, None).await;

    let call = &outcome.calls[0];
    assert_eq!(
        call.input, input,
        "no declaration means no per-tool redaction"
    );
    assert!(
        call.result
            .as_deref()
            .unwrap_or_default()
            .contains("executed:"),
        "result passes through, got {:?}",
        call.result
    );
}

#[tokio::test]
async fn absent_declared_field_redacts_nothing_else() {
    // The misspelled-field case at the runtime layer: a declared name that
    // matches nothing in the payload must not change any other redaction
    // behavior (the declaration-side gate is where a typo fails loudly).
    let tool = "_test_redaction_absent";
    let input = serde_json::json!({"url": "https://acme.corp", "method": "GET"});
    let tools = registry_with_redaction(
        tool,
        Reversibility::FullyReversible,
        RedactionPolicy::Fields(vec!["heders".to_owned()]),
    );

    let outcome = dispatch_one(&tools, tool, input.clone(), None, None).await;

    assert_eq!(
        outcome.calls[0].input, input,
        "a declared field that matched nothing leaves the payload otherwise untouched"
    );
}

#[tokio::test]
async fn approval_prompt_and_tool_start_carry_redacted_input() {
    let tool = "_test_redaction_approval";
    let input = serde_json::json!({
        "text": "type-this-password",
        "action": "type_text",
    });
    let tools = registry_with_redaction(
        tool,
        Reversibility::Irreversible,
        RedactionPolicy::Fields(vec!["text".to_owned()]),
    );
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Approved,
        })
        .await
        .expect("send approval");

    let outcome = dispatch_one(&tools, tool, input, Some(&gate), None).await;

    let approval_input = outcome
        .events
        .iter()
        .find_map(|e| match e {
            TurnStreamEvent::ToolApprovalRequired { input, .. } => Some(input.clone()),
            _ => None,
        })
        .expect("approval-required event emitted");
    assert_eq!(
        approval_input["text"],
        serde_json::json!("[REDACTED]"),
        "the approval prompt redacts the declared field"
    );
    assert_eq!(
        approval_input["action"],
        serde_json::json!("type_text"),
        "the approval prompt keeps undeclared fields legible"
    );

    let start_input = outcome
        .events
        .iter()
        .find_map(|e| match e {
            TurnStreamEvent::ToolStart { input, .. } => Some(input.clone()),
            _ => None,
        })
        .expect("tool-start event emitted");
    assert_eq!(
        start_input["text"],
        serde_json::json!("[REDACTED]"),
        "the tool-start event carries the same redacted copy"
    );
}

#[tokio::test]
async fn receipt_ledger_holds_only_the_redacted_copy() {
    let tool = "_test_redaction_receipt";
    let secret = "value the heuristic would keep: has spaces";
    let input = serde_json::json!({"note": secret});
    let tools =
        registry_with_redaction(tool, Reversibility::FullyReversible, RedactionPolicy::Full);
    let ledger = std::sync::Mutex::new(organon::receipts::ReceiptLedger::new());

    let outcome = dispatch_one(&tools, tool, input, None, Some(&ledger)).await;

    let receipt = outcome.calls[0]
        .receipt
        .as_deref()
        .expect("receipt emitted")
        .to_owned();
    let guard = ledger.lock().expect("ledger lock");
    let entry = guard.lookup(&receipt).expect("receipt recorded");
    assert!(
        !entry.args_json.contains(secret),
        "ledger args must not hold the sensitive value: {}",
        entry.args_json
    );
    assert!(
        entry.args_json.contains("[REDACTED]"),
        "ledger args carry the redaction marker: {}",
        entry.args_json
    );
    assert_eq!(
        entry.result, "[REDACTED]",
        "ledger result follows the Full policy"
    );
}

#[tokio::test]
async fn denied_call_record_is_redacted_too() {
    let tool = "_test_redaction_denied";
    let input = serde_json::json!({"text": "password-to-type", "action": "type_text"});
    let tools = registry_with_redaction(
        tool,
        Reversibility::Irreversible,
        RedactionPolicy::Fields(vec!["text".to_owned()]),
    );
    let (decision_tx, decision_rx) = mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Denied,
        })
        .await
        .expect("send denial");

    let outcome = dispatch_one(&tools, tool, input, Some(&gate), None).await;

    assert_eq!(outcome.calls.len(), 1);
    assert!(outcome.calls[0].is_error, "denied call recorded as error");
    assert_eq!(
        outcome.calls[0].input["text"],
        serde_json::json!("[REDACTED]"),
        "a denied call's recorded input follows the same policy"
    );
    assert_eq!(
        outcome.calls[0].input["action"],
        serde_json::json!("type_text"),
        "undeclared fields stay legible on the denial record"
    );
}
