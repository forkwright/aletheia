//! Governance-metrics wiring tests (#4837).
//!
//! Prior to this, organon exposed only invocation-count/status/duration
//! metrics -- no approval, sandbox, policy-denial, receipt, or truncation
//! counters existed anywhere in the tree. These tests exercise the real
//! dispatch path (not just the isolated recording functions in
//! `organon::metrics`'s own unit tests) to prove each new counter actually
//! increments when the code path that owns it runs.
//!
//! WHY unique tool names per test: the counter families are `LazyLock`
//! statics backed by `Arc`-internal state (see `organon::metrics`'s own
//! WHY comment), so a fresh `MetricsRegistry` is a fresh EXPORT VIEW over
//! globally shared counts, not a reset. Each test below uses a tool name
//! touched by no other test in the suite so its exact-count assertion is
//! immune to parallel test execution.

use koina::metrics::MetricsRegistry;
use organon::types::Reversibility;

use super::*;
use crate::approval::{ApprovalChoice, ApprovalDecision, ApprovalGate};
use crate::execute::dispatch::{ToolDispatchPolicy, dispatch_tools};
use crate::pipeline::LoopDetector;

fn fresh_organon_registry() -> MetricsRegistry {
    let r = MetricsRegistry::new();
    r.with_registry(organon::metrics::register);
    r
}

fn encode(r: &MetricsRegistry) -> String {
    let mut buf = String::new();
    r.encode(&mut buf).expect("encode");
    buf
}

/// WHY: every governance-metrics test wires identical tool_uses/registry/
/// loop_detector/all_calls/policy scaffolding around one `dispatch_tools`
/// call. What genuinely varies across tests: the tool name registered in
/// the `ToolRegistry` vs the name actually dispatched (the unknown-tool
/// test deliberately diverges the two so `denial_for` classifies it
/// `ToolPolicyDenial::Unknown`), the `Reversibility`, whether an approval
/// gate is present, and whether a receipt signer is present.
async fn dispatch_single_tool(
    registered_name: &str,
    dispatched_name: &str,
    reversibility: Reversibility,
    approval_gate: Option<&ApprovalGate>,
    receipt_signer: Option<&organon::receipts::ReceiptSigner>,
) {
    let tools = make_registry_rev(registered_name, reversibility);
    let tool_uses = vec![(
        "tool-1".to_owned(),
        dispatched_name.to_owned(),
        serde_json::json!({}),
    )];
    let mut loop_detector = LoopDetector::new(3);
    let mut all_calls = Vec::new();
    let policy = ToolDispatchPolicy::allow_all_for_tests(&tools);

    dispatch_tools(
        &tool_uses,
        &tools,
        &test_tool_ctx(),
        &mut loop_detector,
        &mut all_calls,
        1,
        None,
        approval_gate,
        &policy,
        0,
        receipt_signer,
        None,
    )
    .await
    .expect("dispatch ok");
}

#[tokio::test]
async fn approval_decision_metric_records_auto_approved() {
    let registry = fresh_organon_registry();
    let tool_name = "_test_metrics_gov_autoapprove";

    dispatch_single_tool(
        tool_name,
        tool_name,
        Reversibility::FullyReversible,
        None,
        None,
    )
    .await;

    let out = encode(&registry);
    assert!(
        out.contains(&format!(
            "aletheia_approval_decisions_total{{tool_name=\"{tool_name}\",decision=\"auto_approved\"}} 1"
        )),
        "got: {out}"
    );
}

#[tokio::test]
async fn approval_decision_metric_records_denied_by_gate() {
    use std::time::Duration;

    let registry = fresh_organon_registry();
    let tool_name = "_test_metrics_gov_gatedenied";
    let (decision_tx, decision_rx) = tokio::sync::mpsc::channel::<ApprovalDecision>(4);
    let gate = ApprovalGate::new(decision_rx, Duration::from_secs(5));
    decision_tx
        .send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Denied,
        })
        .await
        .expect("send denial");

    dispatch_single_tool(
        tool_name,
        tool_name,
        Reversibility::Irreversible,
        Some(&gate),
        None,
    )
    .await;

    let out = encode(&registry);
    assert!(
        out.contains(&format!(
            "aletheia_approval_decisions_total{{tool_name=\"{tool_name}\",decision=\"denied\"}} 1"
        )),
        "got: {out}"
    );
}

#[tokio::test]
async fn policy_denial_metric_records_unknown_tool() {
    let registry = fresh_organon_registry();
    let ghost_name = "_test_metrics_gov_ghost";

    // WHY: the registry needs at least one entry; the dispatched name is
    // deliberately absent from it so `denial_for` classifies it
    // `ToolPolicyDenial::Unknown`.
    dispatch_single_tool(
        "_test_metrics_gov_ghost_sibling",
        ghost_name,
        Reversibility::FullyReversible,
        None,
        None,
    )
    .await;

    let out = encode(&registry);
    assert!(
        out.contains(&format!(
            "aletheia_policy_denied_total{{tool_name=\"{ghost_name}\",policy=\"not_found\"}} 1"
        )),
        "got: {out}"
    );
}

#[tokio::test]
async fn receipt_metric_records_emitted_when_signer_configured() {
    let registry = fresh_organon_registry();
    let tool_name = "_test_metrics_gov_receipt_emit";
    let signer = organon::receipts::ReceiptSigner::new_session();

    dispatch_single_tool(
        tool_name,
        tool_name,
        Reversibility::FullyReversible,
        None,
        Some(&signer),
    )
    .await;

    let out = encode(&registry);
    assert!(
        out.contains(&format!(
            "aletheia_receipts_total{{tool_name=\"{tool_name}\",status=\"emitted\"}} 1"
        )),
        "got: {out}"
    );
}

#[tokio::test]
async fn receipt_metric_records_missing_when_no_signer_configured() {
    let registry = fresh_organon_registry();
    let tool_name = "_test_metrics_gov_receipt_miss";

    dispatch_single_tool(
        tool_name,
        tool_name,
        Reversibility::FullyReversible,
        None,
        None,
    )
    .await;

    let out = encode(&registry);
    assert!(
        out.contains(&format!(
            "aletheia_receipts_total{{tool_name=\"{tool_name}\",status=\"missing\"}} 1"
        )),
        "got: {out}"
    );
}
