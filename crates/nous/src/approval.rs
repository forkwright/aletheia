//! User approval gate for reversibility-class tool calls (#3958, ADR-005).
//!
//! The gate blocks shared tool dispatch between `ToolApprovalRequired`
//! emission and `ToolStart` until the operator answers, or the timeout
//! elapses (default-deny). The decision arrives over a `mpsc::Receiver`
//! whose sender lives in the caller (e.g. pylon's per-turn registry,
//! koilon's overlay handler).

use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;
use std::time::Duration;

use snafu::Snafu;
use tokio::sync::{Mutex, mpsc};
use tracing::warn;

/// Default timeout for awaiting a user decision on a Required/Mandatory tool call.
///
/// 120s matches the desktop daily-driver UX: long enough to read the
/// overlay, short enough that a dropped client connection denies the
/// irreversible action rather than letting it hang the pipeline.
pub const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_mins(2);

/// Maximum operator approval wait accepted by the Nous approval policy.
///
/// SAFETY(#5011): approval waits must be finite. Longer waits behave like
/// unbounded hangs for disconnected clients and keep irreversible actions in an
/// ambiguous state, so operator-provided policy is validated before use.
pub const MAX_APPROVAL_TIMEOUT: Duration = Duration::from_mins(30);

/// Approval policy validation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum ApprovalPolicyError {
    /// The timeout was zero, which would make approval immediately expire.
    #[snafu(display("approval timeout must be greater than zero"))]
    ZeroTimeout,
    /// The timeout was longer than the maximum supported wait.
    #[snafu(display("approval timeout {timeout_secs}s exceeds maximum {max_timeout_secs}s"))]
    TimeoutTooLong {
        /// Requested timeout in whole seconds.
        timeout_secs: u64,
        /// Maximum supported timeout in whole seconds.
        max_timeout_secs: u64,
    },
}

/// Operator-owned approval behavior for a single approval gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ApprovalPolicy {
    timeout: Duration,
}

impl ApprovalPolicy {
    /// Build the default-deny policy with an explicit finite timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalPolicyError`] when `timeout` is zero or too long.
    pub fn default_deny(timeout: Duration) -> Result<Self, ApprovalPolicyError> {
        if timeout.is_zero() {
            return ZeroTimeoutSnafu.fail();
        }
        if timeout > MAX_APPROVAL_TIMEOUT {
            return TimeoutTooLongSnafu {
                timeout_secs: timeout.as_secs(),
                max_timeout_secs: MAX_APPROVAL_TIMEOUT.as_secs(),
            }
            .fail();
        }
        Ok(Self { timeout })
    }

    /// Approval wait timeout.
    #[must_use]
    pub const fn timeout(self) -> Duration {
        self.timeout
    }

    /// Resolution used when the timeout elapses.
    #[must_use]
    pub const fn on_timeout(self) -> ApprovalChoice {
        ApprovalChoice::Denied
    }

    /// Resolution used when the decision channel disconnects.
    #[must_use]
    pub const fn on_disconnect(self) -> ApprovalChoice {
        ApprovalChoice::Denied
    }
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_APPROVAL_TIMEOUT,
        }
    }
}

koina::newtype_id!(
    /// Identifier for a provider-emitted tool call awaiting approval.
    pub struct ApprovalToolId(String)
);

/// A user's decision on a single tool approval request.
#[derive(Debug, Clone)]
pub struct ApprovalDecision {
    /// The `tool_id` this decision applies to. Must match the `tool_use_id`
    /// surfaced in the matching `TurnStreamEvent::ToolApprovalRequired`.
    pub tool_id: ApprovalToolId,
    /// Approve or deny.
    pub choice: ApprovalChoice,
}

impl ApprovalDecision {
    /// Construct a decision for the given tool call ID and choice.
    pub fn new(tool_id: impl Into<ApprovalToolId>, choice: ApprovalChoice) -> Self {
        Self {
            tool_id: tool_id.into(),
            choice,
        }
    }
}

/// Operator choice for a tool approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApprovalChoice {
    /// Proceed with execution.
    Approved,
    /// Skip execution; synthesize a denial `ToolResult` for the model.
    Denied,
}

impl ApprovalChoice {
    /// Wire string carried in `TurnStreamEvent::ToolApprovalResolved.decision`.
    #[must_use]
    pub const fn as_wire_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }
}

/// Shared state held by every clone of an [`ApprovalGate`].
///
/// WHY(#5010): a single mpsc channel carries decisions for every tool in
/// the turn, but dispatch awaits tools sequentially. Buffering decisions
/// keyed by `tool_id` lets decisions arrive in any order without being
/// dropped as stale.
struct ApprovalState {
    rx: Mutex<mpsc::Receiver<ApprovalDecision>>,
    pending: Mutex<HashMap<ApprovalToolId, ApprovalChoice>>,
}

/// Cloneable handle wrapping a shared decision receiver.
///
/// Multiple plumbing layers (actor, pipeline, execute) hold a clone but
/// only one task drains the channel at a time (the dispatch loop), so the
/// receiver lock is uncontended in practice.
#[derive(Clone)]
pub struct ApprovalGate {
    state: Arc<ApprovalState>,
    policy: ApprovalPolicy,
}

impl std::fmt::Debug for ApprovalGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalGate")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl ApprovalGate {
    /// Wrap a receiver with an explicit timeout.
    #[must_use]
    pub fn new(rx: mpsc::Receiver<ApprovalDecision>, timeout: Duration) -> Self {
        let policy = match ApprovalPolicy::default_deny(timeout) {
            Ok(policy) => policy,
            Err(error) => {
                warn!(
                    %error,
                    "invalid approval timeout; falling back to default approval policy"
                );
                ApprovalPolicy::default()
            }
        };
        Self::with_policy(rx, policy)
    }

    /// Wrap a receiver with an explicit validated timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalPolicyError`] when `timeout` is zero or too long.
    pub fn try_new(
        rx: mpsc::Receiver<ApprovalDecision>,
        timeout: Duration,
    ) -> Result<Self, ApprovalPolicyError> {
        Ok(Self::with_policy(
            rx,
            ApprovalPolicy::default_deny(timeout)?,
        ))
    }

    /// Wrap a receiver with an already validated approval policy.
    #[must_use]
    pub fn with_policy(rx: mpsc::Receiver<ApprovalDecision>, policy: ApprovalPolicy) -> Self {
        Self {
            state: Arc::new(ApprovalState {
                rx: Mutex::new(rx),
                pending: Mutex::new(HashMap::new()),
            }),
            policy,
        }
    }

    /// Wrap a receiver with [`DEFAULT_APPROVAL_TIMEOUT`].
    #[must_use]
    pub fn with_default_timeout(rx: mpsc::Receiver<ApprovalDecision>) -> Self {
        Self::with_policy(rx, ApprovalPolicy::default())
    }

    /// Active approval policy.
    #[must_use]
    pub const fn policy(&self) -> ApprovalPolicy {
        self.policy
    }

    /// Block until a decision targeted at `tool_id` arrives.
    ///
    /// Decisions for other tools in the same turn are buffered in a
    /// pending map keyed by `tool_id` and consumed exactly once when that
    /// tool is awaited. Duplicate buffered decisions are ignored with a
    /// warning, keeping the first writer. Channel-closed or elapsed-timeout
    /// both resolve to [`ApprovalChoice::Denied`] for the requested tool.
    pub async fn await_decision(&self, tool_id: &str) -> ApprovalChoice {
        let tool_id = ApprovalToolId::from(tool_id);

        match tokio::time::timeout(self.policy.timeout(), self.wait_for_decision(&tool_id)).await {
            Ok(choice) => choice,
            Err(_elapsed) => {
                warn!(
                    tool_id = tool_id.as_str(),
                    timeout_secs = self.policy.timeout().as_secs(),
                    "approval gate timed out — default-deny"
                );
                self.policy.on_timeout()
            }
        }
    }

    async fn wait_for_decision(&self, tool_id: &ApprovalToolId) -> ApprovalChoice {
        loop {
            if let Some(choice) = self.take_pending(tool_id).await {
                return choice;
            }

            let decision = {
                let mut rx = self.state.rx.lock().await;
                // A concurrent waiter may have buffered this decision while
                // this task was queued for the receiver.
                if let Some(choice) = self.take_pending(tool_id).await {
                    return choice;
                }
                rx.recv().await
            };

            match decision {
                Some(decision) if decision.tool_id.as_str() == tool_id.as_str() => {
                    return decision.choice;
                }
                Some(decision) => self.buffer_pending(decision).await,
                None => return self.policy.on_disconnect(),
            }
        }
    }

    async fn take_pending(&self, tool_id: &ApprovalToolId) -> Option<ApprovalChoice> {
        self.state.pending.lock().await.remove(tool_id.as_str())
    }

    async fn buffer_pending(&self, decision: ApprovalDecision) {
        // Buffer decisions targeting other tools in this turn so that
        // sequential dispatch can consume them when it reaches each tool.
        match self.state.pending.lock().await.entry(decision.tool_id) {
            Entry::Occupied(occ) => {
                warn!(
                    tool_id = %occ.key(),
                    "duplicate approval decision; keeping first-writer-wins"
                );
            }
            Entry::Vacant(slot) => {
                slot.insert(decision.choice);
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approved_decision_resolves() {
        let (tx, rx) = mpsc::channel(4);
        let gate = ApprovalGate::with_default_timeout(rx);
        tx.send(ApprovalDecision::new("tool-1", ApprovalChoice::Approved))
            .await
            .expect("send");
        assert_eq!(
            gate.await_decision("tool-1").await,
            ApprovalChoice::Approved
        );
    }

    #[tokio::test]
    async fn denied_decision_resolves() {
        let (tx, rx) = mpsc::channel(4);
        let gate = ApprovalGate::with_default_timeout(rx);
        tx.send(ApprovalDecision::new("tool-2", ApprovalChoice::Denied))
            .await
            .expect("send");
        assert_eq!(gate.await_decision("tool-2").await, ApprovalChoice::Denied);
    }

    #[tokio::test]
    async fn timeout_yields_denial() {
        let (_tx, rx) = mpsc::channel(4);
        let gate = ApprovalGate::new(rx, Duration::from_millis(50));
        assert_eq!(gate.await_decision("tool-x").await, ApprovalChoice::Denied);
    }

    #[tokio::test]
    async fn policy_non_default_timeout_changes_gate_behavior() {
        let (_tx, rx) = mpsc::channel(4);
        let policy = ApprovalPolicy::default_deny(Duration::from_millis(10)).expect("valid policy");
        let gate = ApprovalGate::with_policy(rx, policy);
        assert_eq!(gate.policy().timeout(), Duration::from_millis(10));
        assert_eq!(gate.await_decision("tool-x").await, ApprovalChoice::Denied);
    }

    #[test]
    fn policy_rejects_unsafe_timeouts() {
        assert!(matches!(
            ApprovalPolicy::default_deny(Duration::ZERO),
            Err(ApprovalPolicyError::ZeroTimeout)
        ));
        assert!(matches!(
            ApprovalPolicy::default_deny(MAX_APPROVAL_TIMEOUT + Duration::from_secs(1)),
            Err(ApprovalPolicyError::TimeoutTooLong { .. })
        ));
    }

    #[tokio::test]
    async fn closed_channel_yields_denial() {
        let (tx, rx) = mpsc::channel(4);
        let gate = ApprovalGate::with_default_timeout(rx);
        drop(tx);
        assert_eq!(gate.await_decision("tool-x").await, ApprovalChoice::Denied);
    }

    #[tokio::test]
    async fn mismatched_decision_buffered_until_match() {
        let (tx, rx) = mpsc::channel(4);
        let gate = ApprovalGate::with_default_timeout(rx);
        tx.send(ApprovalDecision::new("old", ApprovalChoice::Approved))
            .await
            .expect("send stale");
        tx.send(ApprovalDecision::new("current", ApprovalChoice::Denied))
            .await
            .expect("send current");
        assert_eq!(gate.await_decision("current").await, ApprovalChoice::Denied);
        // The earlier decision was buffered, not dropped, and is available later.
        assert_eq!(gate.await_decision("old").await, ApprovalChoice::Approved);
    }

    #[tokio::test]
    async fn future_decision_buffered_before_await() {
        let (tx, rx) = mpsc::channel(4);
        let gate = ApprovalGate::new(rx, Duration::from_millis(50));
        tx.send(ApprovalDecision::new("future", ApprovalChoice::Approved))
            .await
            .expect("send future");
        // Await a different tool; the future decision stays buffered.
        assert_eq!(gate.await_decision("now").await, ApprovalChoice::Denied);
        assert_eq!(
            gate.await_decision("future").await,
            ApprovalChoice::Approved
        );
    }

    #[tokio::test]
    async fn two_tools_answered_out_of_order() {
        let (tx, rx) = mpsc::channel(4);
        let gate = ApprovalGate::new(rx, Duration::from_millis(50));
        tx.send(ApprovalDecision::new("tool-2", ApprovalChoice::Approved))
            .await
            .expect("send tool-2");
        tx.send(ApprovalDecision::new("tool-1", ApprovalChoice::Denied))
            .await
            .expect("send tool-1");
        assert_eq!(gate.await_decision("tool-1").await, ApprovalChoice::Denied);
        assert_eq!(
            gate.await_decision("tool-2").await,
            ApprovalChoice::Approved
        );
    }

    #[tokio::test]
    async fn duplicate_future_decision_keeps_first_wins() {
        let (tx, rx) = mpsc::channel(4);
        let gate = ApprovalGate::new(rx, Duration::from_millis(50));
        tx.send(ApprovalDecision::new("tool-2", ApprovalChoice::Approved))
            .await
            .expect("send first");
        tx.send(ApprovalDecision::new("tool-2", ApprovalChoice::Denied))
            .await
            .expect("send duplicate");
        // While awaiting tool-1, two decisions for tool-2 arrive.
        assert_eq!(gate.await_decision("tool-1").await, ApprovalChoice::Denied);
        // First-writer-wins: Approved, not the later Denied.
        assert_eq!(
            gate.await_decision("tool-2").await,
            ApprovalChoice::Approved
        );
    }

    #[test]
    fn wire_strings() {
        assert_eq!(ApprovalChoice::Approved.as_wire_str(), "approved");
        assert_eq!(ApprovalChoice::Denied.as_wire_str(), "denied");
    }
}
