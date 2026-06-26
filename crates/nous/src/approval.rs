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

use tokio::sync::{Mutex, mpsc};
use tracing::warn;

/// Default timeout for awaiting a user decision on a Required/Mandatory tool call.
///
/// 120s matches the desktop daily-driver UX: long enough to read the
/// overlay, short enough that a dropped client connection denies the
/// irreversible action rather than letting it hang the pipeline.
pub const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_mins(2);

/// A user's decision on a single tool approval request.
#[derive(Debug, Clone)]
pub struct ApprovalDecision {
    /// The `tool_id` this decision applies to. Must match the `tool_use_id`
    /// surfaced in the matching `TurnStreamEvent::ToolApprovalRequired`.
    pub tool_id: String,
    /// Approve or deny.
    pub choice: ApprovalChoice,
}

impl ApprovalDecision {
    /// Construct a decision for the given tool call ID and choice.
    pub fn new(tool_id: impl Into<String>, choice: ApprovalChoice) -> Self {
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
    rx: mpsc::Receiver<ApprovalDecision>,
    pending: HashMap<String, ApprovalChoice>,
}

/// Cloneable handle wrapping a shared decision receiver.
///
/// Multiple plumbing layers (actor, pipeline, execute) hold a clone but
/// only one task drains the channel at a time (the dispatch loop), so the
/// inner `Mutex` is uncontended in practice.
#[derive(Clone)]
pub struct ApprovalGate {
    state: Arc<Mutex<ApprovalState>>,
    timeout: Duration,
}

impl std::fmt::Debug for ApprovalGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalGate")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ApprovalGate {
    /// Wrap a receiver with an explicit timeout.
    #[must_use]
    pub fn new(rx: mpsc::Receiver<ApprovalDecision>, timeout: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(ApprovalState {
                rx,
                pending: HashMap::new(),
            })),
            timeout,
        }
    }

    /// Wrap a receiver with [`DEFAULT_APPROVAL_TIMEOUT`].
    #[must_use]
    pub fn with_default_timeout(rx: mpsc::Receiver<ApprovalDecision>) -> Self {
        Self::new(rx, DEFAULT_APPROVAL_TIMEOUT)
    }

    /// Block until a decision targeted at `tool_id` arrives.
    ///
    /// Decisions for other tools in the same turn are buffered in a
    /// pending map keyed by `tool_id` and consumed exactly once when that
    /// tool is awaited. Duplicate buffered decisions are ignored with a
    /// warning, keeping the first writer. Channel-closed or elapsed-timeout
    /// both resolve to [`ApprovalChoice::Denied`] for the requested tool.
    pub async fn await_decision(&self, tool_id: &str) -> ApprovalChoice {
        let mut state = self.state.lock().await;

        // If a decision for this tool was already buffered, consume it exactly once.
        if let Some(choice) = state.pending.remove(tool_id) {
            return choice;
        }

        match tokio::time::timeout(self.timeout, async {
            loop {
                match state.rx.recv().await {
                    Some(d) if d.tool_id == tool_id => return d.choice,
                    Some(d) => {
                        // Buffer decisions targeting other tools in this turn so that
                        // sequential dispatch can consume them when it reaches each tool.
                        match state.pending.entry(d.tool_id) {
                            Entry::Occupied(_) => {
                                warn!(
                                    tool_id = %d.tool_id,
                                    "duplicate approval decision; keeping first-writer-wins"
                                );
                            }
                            Entry::Vacant(slot) => {
                                slot.insert(d.choice);
                            }
                        }
                    }
                    None => return ApprovalChoice::Denied,
                }
            }
        })
        .await
        {
            Ok(choice) => choice,
            Err(_elapsed) => {
                warn!(
                    tool_id,
                    timeout_secs = self.timeout.as_secs(),
                    "approval gate timed out — default-deny"
                );
                ApprovalChoice::Denied
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
        tx.send(ApprovalDecision {
            tool_id: "tool-1".to_owned(),
            choice: ApprovalChoice::Approved,
        })
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
        tx.send(ApprovalDecision {
            tool_id: "tool-2".to_owned(),
            choice: ApprovalChoice::Denied,
        })
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
        tx.send(ApprovalDecision {
            tool_id: "old".to_owned(),
            choice: ApprovalChoice::Approved,
        })
        .await
        .expect("send stale");
        tx.send(ApprovalDecision {
            tool_id: "current".to_owned(),
            choice: ApprovalChoice::Denied,
        })
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
        assert_eq!(gate.await_decision("future").await, ApprovalChoice::Approved);
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
        assert_eq!(gate.await_decision("tool-2").await, ApprovalChoice::Approved);
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
        assert_eq!(gate.await_decision("tool-2").await, ApprovalChoice::Approved);
    }

    #[test]
    fn wire_strings() {
        assert_eq!(ApprovalChoice::Approved.as_wire_str(), "approved");
        assert_eq!(ApprovalChoice::Denied.as_wire_str(), "denied");
    }
}
