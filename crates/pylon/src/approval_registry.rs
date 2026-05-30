//! Per-session approval-decision sender registry (#3958, ADR-005).
//!
//! Pylon's streaming handler registers a `Sender` keyed by session id when a
//! turn starts; the `POST /api/v1/sessions/{session_id}/approvals` handler
//! looks it up to route the operator's decision into the nous-side gate.
//! The Sender is unregistered when the turn ends (either via the
//! [`Guard`] dropping or explicit removal).

use std::collections::HashMap;
use std::sync::Arc;

use nous::approval::ApprovalDecision;
use tokio::sync::{Mutex, mpsc};

/// Concurrent map from session id → approval-decision sender.
#[derive(Default)]
pub struct ApprovalRegistry {
    inner: Mutex<HashMap<String, mpsc::Sender<ApprovalDecision>>>,
}

impl ApprovalRegistry {
    /// Create a fresh empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a sender for `session_id`, returning a guard that removes
    /// the entry when dropped.
    pub async fn register(
        self: &Arc<Self>,
        session_id: String,
        sender: mpsc::Sender<ApprovalDecision>,
    ) -> Guard {
        let mut map = self.inner.lock().await;
        map.insert(session_id.clone(), sender);
        Guard {
            registry: Arc::clone(self),
            session_id: Some(session_id),
        }
    }

    /// Look up the sender for `session_id` and send a decision.
    /// Returns `false` if no entry exists or the receiver has dropped.
    pub async fn try_send(&self, session_id: &str, decision: ApprovalDecision) -> bool {
        let map = self.inner.lock().await;
        if let Some(sender) = map.get(session_id) {
            sender.try_send(decision).is_ok()
        } else {
            false
        }
    }

    async fn remove(&self, session_id: &str) {
        let mut map = self.inner.lock().await;
        map.remove(session_id);
    }
}

/// RAII guard that unregisters the session's sender when dropped.
pub struct Guard {
    registry: Arc<ApprovalRegistry>,
    session_id: Option<String>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(sid) = self.session_id.take() {
            let registry = Arc::clone(&self.registry);
            tokio::spawn(async move {
                registry.remove(&sid).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nous::approval::ApprovalChoice;

    #[tokio::test]
    async fn register_send_remove_roundtrip() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, mut rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register("sess-1".to_owned(), tx).await;

        assert!(
            reg.try_send(
                "sess-1",
                ApprovalDecision {
                    tool_id: "t-1".to_owned(),
                    choice: ApprovalChoice::Approved,
                }
            )
            .await
        );
        let decision = rx.recv().await.expect("decision");
        assert_eq!(decision.tool_id, "t-1");
    }

    #[tokio::test]
    async fn unknown_session_returns_false() {
        let reg = ApprovalRegistry::new();
        assert!(
            !reg.try_send(
                "missing",
                ApprovalDecision {
                    tool_id: "x".to_owned(),
                    choice: ApprovalChoice::Approved,
                }
            )
            .await
        );
    }

    #[tokio::test]
    async fn guard_unregisters_on_drop() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        {
            let _guard = reg.register("sess-2".to_owned(), tx).await;
            assert!(reg.inner.lock().await.contains_key("sess-2"));
        }
        // Allow the spawned removal task to run.
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(!reg.inner.lock().await.contains_key("sess-2"));
    }
}
