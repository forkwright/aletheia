//! User approval gate for reversibility-class tool calls (#3958, ADR-005).
//!
//! The gate blocks shared tool dispatch between `ToolApprovalRequired`
//! emission and `ToolStart` until the operator answers, or the timeout
//! elapses (default-deny). The decision arrives over a `mpsc::Receiver`
//! whose sender lives in the caller (e.g. pylon's per-session registry,
//! koilon's overlay handler).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, mpsc};
use tracing::warn;

/// Default timeout for awaiting a user decision on a Required/Mandatory tool call.
///
/// 120s matches the desktop daily-driver UX: long enough to read the
/// overlay, short enough that a dropped client connection denies the
/// irreversible action rather than letting it hang the pipeline.
pub const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

/// A user's decision on a single tool approval request.
#[derive(Debug, Clone)]
pub struct ApprovalDecision {
    /// The `tool_id` this decision applies to. Must match the `tool_use_id`
    /// surfaced in the matching `TurnStreamEvent::ToolApprovalRequired`.
    pub tool_id: String,
    /// Approve or deny.
    pub choice: ApprovalChoice,
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

/// Cloneable handle wrapping a shared decision receiver.
///
/// Multiple plumbing layers (actor, pipeline, execute) hold a clone but
/// only one task drains the channel at a time (the dispatch loop), so the
/// inner `Mutex` is uncontended in practice.
#[derive(Clone)]
pub struct ApprovalGate {
    rx: Arc<Mutex<mpsc::Receiver<ApprovalDecision>>>,
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
            rx: Arc::new(Mutex::new(rx)),
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
    /// Stale decisions (mismatched `tool_id`) are dropped with a warning.
    /// Channel-closed or elapsed-timeout both resolve to [`ApprovalChoice::Denied`].
    pub async fn await_decision(&self, tool_id: &str) -> ApprovalChoice {
        let mut rx = self.rx.lock().await;
        match tokio::time::timeout(self.timeout, async {
            loop {
                match rx.recv().await {
                    Some(d) if d.tool_id == tool_id => return d.choice,
                    Some(stale) => {
                        warn!(
                            stale_tool_id = stale.tool_id,
                            expected = tool_id,
                            "approval decision for unexpected tool_id; dropping"
                        );
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
    async fn stale_decisions_dropped_until_match() {
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
    }

    #[test]
    fn wire_strings() {
        assert_eq!(ApprovalChoice::Approved.as_wire_str(), "approved");
        assert_eq!(ApprovalChoice::Denied.as_wire_str(), "denied");
    }
}
