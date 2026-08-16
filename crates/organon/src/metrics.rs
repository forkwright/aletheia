//! Prometheus metric definitions for the tool system.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.
//!
//! Live invocations are tracked separately from the Prometheus counters so that
//! the ops surface can report currently-running tool calls. An RAII guard
//! removes the entry when the guard is dropped or the owning async future is
//! cancelled.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

use crate::sandbox::{EgressPolicy, SandboxEnforcement};

// ── Label sets ──

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ToolInvocationLabels {
    tool_name: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ToolLabels {
    tool_name: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ApprovalDecisionLabels {
    tool_name: String,
    decision: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct SandboxModeLabels {
    enforcement: String,
    egress: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct PolicyDenialLabels {
    tool_name: String,
    policy: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ReceiptLabels {
    tool_name: String,
    status: String,
}

// ── Metric families ──

static TOOL_INVOCATIONS_TOTAL: LazyLock<Family<ToolInvocationLabels, Counter>> =
    LazyLock::new(Family::default);

fn tool_duration_histogram() -> Histogram {
    Histogram::new([0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 120.0])
}

type ToolHistogramFamily = Family<ToolLabels, Histogram, fn() -> Histogram>;

static TOOL_DURATION_SECONDS: LazyLock<ToolHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(tool_duration_histogram));

static APPROVAL_DECISIONS_TOTAL: LazyLock<Family<ApprovalDecisionLabels, Counter>> =
    LazyLock::new(Family::default);

static SANDBOX_MODE_TOTAL: LazyLock<Family<SandboxModeLabels, Counter>> =
    LazyLock::new(Family::default);

static POLICY_DENIED_TOTAL: LazyLock<Family<PolicyDenialLabels, Counter>> =
    LazyLock::new(Family::default);

static RECEIPTS_TOTAL: LazyLock<Family<ReceiptLabels, Counter>> = LazyLock::new(Family::default);

static TOOL_OUTPUT_TRUNCATED_TOTAL: LazyLock<Family<ToolLabels, Counter>> =
    LazyLock::new(Family::default);

// ── Live invocation tracking ──

static NEXT_INVOCATION_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_INVOCATIONS: LazyLock<Mutex<Vec<ActiveEntry>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

struct ActiveEntry {
    id: u64,
    tool_name: String,
    started_at: Instant,
}

/// A currently-running tool invocation reported by the ops surface.
#[derive(Debug, Clone)]
pub struct LiveInvocation {
    /// Stable invocation identifier.
    pub id: u64,
    /// Tool name being executed.
    pub tool_name: String,
    /// When the invocation started.
    pub started_at: Instant,
}

/// RAII guard that removes a live invocation entry on drop.
///
/// Keep this guard alive for the duration of the tool execution so that
/// cancellation or early return automatically clears the live entry.
#[derive(Debug)]
pub struct ActiveInvocationGuard {
    id: u64,
}

impl Drop for ActiveInvocationGuard {
    fn drop(&mut self) {
        remove_active(self.id);
    }
}

/// Begin tracking a live tool invocation.
///
/// The returned guard must be retained until the invocation completes. Dropping
/// it removes the entry from the live set, including when the async execution
/// future is cancelled.
#[must_use]
pub fn track_invocation(tool_name: &str) -> ActiveInvocationGuard {
    let id = NEXT_INVOCATION_ID.fetch_add(1, Ordering::Relaxed);
    {
        #[expect(
            clippy::expect_used,
            reason = "live-invocation mutex is not poisoned by design"
        )]
        let mut active = ACTIVE_INVOCATIONS
            .lock()
            .expect("live invocation mutex poisoned");
        active.push(ActiveEntry {
            id,
            tool_name: tool_name.to_owned(),
            started_at: Instant::now(),
        });
    }
    ActiveInvocationGuard { id }
}

fn remove_active(id: u64) {
    #[expect(
        clippy::expect_used,
        reason = "live-invocation mutex is not poisoned by design"
    )]
    let mut active = ACTIVE_INVOCATIONS
        .lock()
        .expect("live invocation mutex poisoned");
    active.retain(|entry| entry.id != id);
}

/// Snapshot of all currently-running tool invocations.
#[must_use]
pub fn live_invocations() -> Vec<LiveInvocation> {
    #[expect(
        clippy::expect_used,
        reason = "live-invocation mutex is not poisoned by design"
    )]
    let active = ACTIVE_INVOCATIONS
        .lock()
        .expect("live invocation mutex poisoned");
    active
        .iter()
        .map(|entry| LiveInvocation {
            id: entry.id,
            tool_name: entry.tool_name.clone(),
            started_at: entry.started_at,
        })
        .collect()
}

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_tool_invocations",
        "Total tool invocations",
        TOOL_INVOCATIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_tool_duration_seconds",
        "Tool execution duration in seconds",
        TOOL_DURATION_SECONDS.clone(),
    );
    registry.register(
        "aletheia_approval_decisions",
        "Approval-gate decisions by outcome",
        APPROVAL_DECISIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_sandbox_mode",
        "Subprocess spawns by configured sandbox enforcement/egress mode",
        SANDBOX_MODE_TOTAL.clone(),
    );
    registry.register(
        "aletheia_policy_denied",
        "Tool calls denied before execution, by denial class",
        POLICY_DENIED_TOTAL.clone(),
    );
    registry.register(
        "aletheia_receipts",
        "Tool-call receipts by emission status",
        RECEIPTS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_tool_output_truncated",
        "Tool invocations whose output was truncated to the configured byte bound",
        TOOL_OUTPUT_TRUNCATED_TOTAL.clone(),
    );
}

// ── Recording ──

/// Outcome bucket used for tool invocation metrics.
#[derive(Clone, Copy)]
pub(crate) enum InvocationStatus {
    Ok,
    Partial,
    Error,
}

impl InvocationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Partial => "partial",
            Self::Error => "error",
        }
    }
}

/// Record a tool invocation.
pub(crate) fn record_invocation(tool_name: &str, duration_secs: f64, status: InvocationStatus) {
    TOOL_INVOCATIONS_TOTAL
        .get_or_create(&ToolInvocationLabels {
            tool_name: tool_name.to_owned(),
            status: status.as_str().to_owned(),
        })
        .inc();
    TOOL_DURATION_SECONDS
        .get_or_create(&ToolLabels {
            tool_name: tool_name.to_owned(),
        })
        .observe(duration_secs);
}

/// Record an approval-gate decision for a Required/Mandatory/Advisory/None
/// tool call.
///
/// `decision` is the same outcome vocabulary already carried on
/// `ToolCall::approval` (`auto_approved`, `advisory_auto`, `no_gate_denied`,
/// or the gate's own `approved`/`denied`) -- reused here rather than a
/// narrower invented enum so the metric never drifts from what the caller
/// already logs.
pub fn record_approval_decision(tool_name: &str, decision: &str) {
    APPROVAL_DECISIONS_TOTAL
        .get_or_create(&ApprovalDecisionLabels {
            tool_name: tool_name.to_owned(),
            decision: decision.to_owned(),
        })
        .inc();
}

fn enforcement_str(enforcement: SandboxEnforcement) -> &'static str {
    match enforcement {
        SandboxEnforcement::Enforcing => "enforcing",
        SandboxEnforcement::Permissive => "permissive",
        // WHY: SandboxEnforcement is `#[non_exhaustive]` (single-owned by
        // taxis, ARCHITECTURE #4846).
        _ => "unknown",
    }
}

fn egress_str(egress: EgressPolicy) -> &'static str {
    match egress {
        EgressPolicy::Deny => "deny",
        EgressPolicy::Allow => "allow",
        EgressPolicy::Allowlist => "allowlist",
        // WHY: EgressPolicy is `#[non_exhaustive]` (single-owned by taxis,
        // ARCHITECTURE #4846).
        _ => "unknown",
    }
}

/// Record the sandbox mode a subprocess was spawned under.
///
/// Recorded in the PARENT process at spawn time, from the policy already
/// resolved before `Command::spawn` -- not from inside the `pre_exec`
/// closure. A `pre_exec` closure runs in the forked child between `fork`
/// and `exec`; a counter incremented there lives in memory the parent's
/// registry never observes (and `exec` usually discards it immediately
/// after), so it would silently never reach `/metrics`.
pub fn record_sandbox_mode(enforcement: SandboxEnforcement, egress: EgressPolicy) {
    SANDBOX_MODE_TOTAL
        .get_or_create(&SandboxModeLabels {
            enforcement: enforcement_str(enforcement).to_owned(),
            egress: egress_str(egress).to_owned(),
        })
        .inc();
}

/// Record a subprocess spawn that ran with no sandbox policy configured.
pub fn record_sandbox_unconfigured() {
    SANDBOX_MODE_TOTAL
        .get_or_create(&SandboxModeLabels {
            enforcement: "none".to_owned(),
            egress: "none".to_owned(),
        })
        .inc();
}

/// Record a tool call denied before it ever executed.
///
/// `policy` is the same outcome vocabulary `record_denied_call` already
/// attaches to `ToolCall::approval` for a denied call (`denied_by_role`,
/// `denied_by_group`, `denied_by_hook`, `denied_inactive`, `not_found`,
/// `failed`, `undispatched_loop_warning`, `no_gate_denied`, or the approval
/// gate's own `denied`) -- distinct from [`record_approval_decision`],
/// which tracks every approval-gate outcome including approvals; this
/// tracks only calls that never ran.
pub fn record_policy_denial(tool_name: &str, policy: &str) {
    POLICY_DENIED_TOTAL
        .get_or_create(&PolicyDenialLabels {
            tool_name: tool_name.to_owned(),
            policy: policy.to_owned(),
        })
        .inc();
}

/// Record whether a tool call's receipt was emitted.
///
/// `status` is `"emitted"` when a `ReceiptSigner` was configured for the
/// dispatch and signed the call, `"missing"` when no signer was configured.
pub fn record_receipt(tool_name: &str, status: &str) {
    RECEIPTS_TOTAL
        .get_or_create(&ReceiptLabels {
            tool_name: tool_name.to_owned(),
            status: status.to_owned(),
        })
        .inc();
}

/// Record a tool invocation whose output was truncated to the configured
/// byte bound.
pub fn record_output_truncation(tool_name: &str) {
    TOOL_OUTPUT_TRUNCATED_TOTAL
        .get_or_create(&ToolLabels {
            tool_name: tool_name.to_owned(),
        })
        .inc();
}

#[cfg(test)]
mod tests {
    use koina::metrics::MetricsRegistry;

    use super::*;

    fn fresh_registry() -> MetricsRegistry {
        let r = MetricsRegistry::new();
        r.with_registry(register);
        r
    }

    fn encode(r: &MetricsRegistry) -> String {
        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        buf
    }

    fn live_for(tool_name: &str) -> Vec<LiveInvocation> {
        live_invocations()
            .into_iter()
            .filter(|inv| inv.tool_name == tool_name)
            .collect()
    }

    #[test]
    fn register_and_record_invocation_success() {
        let r = fresh_registry();
        record_invocation("_test_tool_ok", 0.05, InvocationStatus::Ok);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_tool_invocations_total{tool_name=\"_test_tool_ok\",status=\"ok\"} 1"
            ),
            "got: {out}"
        );
        assert!(
            out.contains("aletheia_tool_duration_seconds_count{tool_name=\"_test_tool_ok\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_invocation_failure() {
        let r = fresh_registry();
        record_invocation("_test_tool_err", 0.01, InvocationStatus::Error);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_tool_invocations_total{tool_name=\"_test_tool_err\",status=\"error\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_invocation_partial() {
        let r = fresh_registry();
        record_invocation("_test_tool_partial", 0.02, InvocationStatus::Partial);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_tool_invocations_total{tool_name=\"_test_tool_partial\",status=\"partial\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn track_invocation_adds_live_entry() {
        let guard = track_invocation("_test_live");
        let live = live_for("_test_live");
        assert_eq!(live.len(), 1, "expected one live invocation");
        assert!(
            live.iter().any(|inv| inv.tool_name == "_test_live"),
            "live invocation should report the tracked tool name"
        );
        // The guard is intentionally retained until after the assertions.
        drop(guard);
    }

    #[test]
    fn guard_drop_removes_live_entry() {
        {
            let _guard = track_invocation("_test_drop");
            assert_eq!(live_for("_test_drop").len(), 1);
        }
        assert!(
            live_for("_test_drop").is_empty(),
            "drop must remove live entry"
        );
    }

    #[tokio::test]
    async fn cancellation_removes_live_entry() {
        let handle = tokio::spawn(async {
            let _guard = track_invocation("_test_cancel");
            // Never resolve, forcing cancellation when the future is dropped.
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        assert_eq!(live_for("_test_cancel").len(), 1);
        handle.abort();
        let join = handle.await;
        assert!(join.is_err(), "aborted task should return a join error");
        assert!(
            live_for("_test_cancel").is_empty(),
            "cancelling the future must drop the guard and remove the live entry"
        );
    }

    #[test]
    fn live_invocations_returns_unique_ids() {
        let a = track_invocation("_test_a");
        let b = track_invocation("_test_b");
        let ids: Vec<u64> = live_invocations()
            .iter()
            .filter(|l| l.tool_name == "_test_a" || l.tool_name == "_test_b")
            .map(|l| l.id)
            .collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids.first(), ids.get(1));
        drop((a, b));
    }

    #[test]
    fn register_and_record_approval_decision() {
        let r = fresh_registry();
        record_approval_decision("_test_approval_tool", "approved");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_approval_decisions_total{tool_name=\"_test_approval_tool\",decision=\"approved\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn approval_decision_cardinality_is_per_tool_and_decision() {
        let r = fresh_registry();
        record_approval_decision("_test_card", "approved");
        record_approval_decision("_test_card", "approved");
        record_approval_decision("_test_card", "denied");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_approval_decisions_total{tool_name=\"_test_card\",decision=\"approved\"} 2"
            ),
            "got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_approval_decisions_total{tool_name=\"_test_card\",decision=\"denied\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_sandbox_mode() {
        let r = fresh_registry();
        record_sandbox_mode(SandboxEnforcement::Enforcing, EgressPolicy::Deny);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_sandbox_mode_total{enforcement=\"enforcing\",egress=\"deny\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_sandbox_unconfigured() {
        let r = fresh_registry();
        record_sandbox_unconfigured();
        let out = encode(&r);
        assert!(
            out.contains("aletheia_sandbox_mode_total{enforcement=\"none\",egress=\"none\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_policy_denial() {
        let r = fresh_registry();
        record_policy_denial("_test_denied_tool", "denied_by_role");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_policy_denied_total{tool_name=\"_test_denied_tool\",policy=\"denied_by_role\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_receipt_emitted_and_missing() {
        let r = fresh_registry();
        record_receipt("_test_receipt_tool", "emitted");
        record_receipt("_test_receipt_tool", "missing");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_receipts_total{tool_name=\"_test_receipt_tool\",status=\"emitted\"} 1"
            ),
            "got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_receipts_total{tool_name=\"_test_receipt_tool\",status=\"missing\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_output_truncation() {
        let r = fresh_registry();
        record_output_truncation("_test_truncated_tool");
        record_output_truncation("_test_truncated_tool");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_tool_output_truncated_total{tool_name=\"_test_truncated_tool\"} 2"
            ),
            "got: {out}"
        );
    }
}
