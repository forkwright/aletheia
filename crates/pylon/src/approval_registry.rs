//! Per-turn approval-decision sender registry (#3958, ADR-005).
//!
//! Pylon's streaming handler registers a pending key when a turn emits
//! `tool_approval_required`; approval handlers route the operator's decision
//! into the matching nous-side gate by turn and tool id, with session id kept
//! as context for session-scoped API routes. The guard unregisters only keys
//! for the turn that created it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nous::approval::ApprovalDecision;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ApprovalKey {
    turn_id: String,
    tool_id: String,
}

struct ApprovalEntry {
    session_id: String,
    sender: mpsc::Sender<ApprovalDecision>,
}

/// Concurrent map from `(turn_id, tool_id)` → approval-decision sender.
#[derive(Default)]
pub struct ApprovalRegistry {
    // WHY: std::sync::Mutex (not tokio) so Guard::drop can call remove_turn
    // synchronously without spawning — avoids fire-and-forget race with
    // runtime shutdown and makes panics in cleanup observable.
    inner: Mutex<HashMap<ApprovalKey, ApprovalEntry>>,
}

impl ApprovalRegistry {
    /// Create a fresh empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a guard for a streaming turn.
    pub fn register_turn(self: &Arc<Self>, session_id: String, turn_id: String) -> Guard {
        Guard {
            registry: Arc::clone(self),
            session_id: Some(session_id),
            turn_id: Some(turn_id),
        }
    }

    /// Register one pending tool approval for an active turn.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (indicates a prior panic in
    /// `remove_turn` or `try_send`).
    pub async fn register_tool(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_id: String,
        sender: mpsc::Sender<ApprovalDecision>,
    ) {
        let mut map = self.inner.lock().expect("approval registry lock poisoned");
        map.insert(
            ApprovalKey {
                turn_id: turn_id.to_owned(),
                tool_id,
            },
            ApprovalEntry {
                session_id: session_id.to_owned(),
                sender,
            },
        );
    }

    /// Look up the sender for `(turn_id, tool_id)` and send a decision.
    ///
    /// When `session_id` is `Some`, it must match the pending entry's session
    /// context. Returns `false` if no exact pending entry exists, the session
    /// context does not match, or the receiver has dropped.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned.
    pub async fn try_send(
        &self,
        session_id: Option<&str>,
        turn_id: &str,
        tool_id: &str,
        decision: ApprovalDecision,
    ) -> bool {
        let key = ApprovalKey {
            turn_id: turn_id.to_owned(),
            tool_id: tool_id.to_owned(),
        };
        let mut map = self.inner.lock().expect("approval registry lock poisoned");
        let Some(entry) = map.get(&key) else {
            return false;
        };
        if let Some(session_id) = session_id
            && entry.session_id != session_id
        {
            return false;
        }

        let sender = entry.sender.clone();
        map.remove(&key);
        drop(map);
        sender.try_send(decision).is_ok()
    }

    fn remove_turn(&self, session_id: &str, turn_id: &str) {
        let mut map = self.inner.lock().expect("approval registry lock poisoned");
        map.retain(|key, entry| key.turn_id != turn_id || entry.session_id != session_id);
    }
}

/// RAII guard that unregisters a turn's pending senders when dropped.
pub struct Guard {
    registry: Arc<ApprovalRegistry>,
    session_id: Option<String>,
    turn_id: Option<String>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let (Some(sid), Some(turn_id)) = (self.session_id.take(), self.turn_id.take()) {
            // WHY: call remove_turn directly (no spawn) so cleanup is
            // deterministic and any panic in remove_turn propagates to the
            // caller — eliminating the fire-and-forget race with runtime
            // shutdown that the prior tokio::spawn pattern had (#5737).
            self.registry.remove_turn(&sid, &turn_id);
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use nous::approval::ApprovalChoice;

    use super::*;

    #[tokio::test]
    async fn register_send_remove_roundtrip() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, mut rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess-1".to_owned(), "turn-1".to_owned());
        reg.register_tool("sess-1", "turn-1", "t-1".to_owned(), tx)
            .await;

        assert!(
            reg.try_send(
                Some("sess-1"),
                "turn-1",
                "t-1",
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
                Some("missing"),
                "turn-x",
                "tool-x",
                ApprovalDecision {
                    tool_id: "tool-x".to_owned(),
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
            let _guard = reg.register_turn("sess-2".to_owned(), "turn-2".to_owned());
            reg.register_tool("sess-2", "turn-2", "t-2".to_owned(), tx)
                .await;
            assert!(reg.inner.lock().expect("lock").contains_key(&ApprovalKey {
                turn_id: "turn-2".to_owned(),
                tool_id: "t-2".to_owned(),
            }));
        }
        // WHY: cleanup is now synchronous (no spawn), so no yield/sleep needed.
        assert!(reg.inner.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn guard_drop_under_task_abort_is_synchronous() {
        // WHY(#5737): verify that Guard::drop completes cleanup without
        // relying on a spawned task — so abort/shutdown cannot drop entries.
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        let reg2 = Arc::clone(&reg);
        let handle = tokio::spawn(async move {
            let _guard = reg2.register_turn("sess-3".to_owned(), "turn-3".to_owned());
            reg2.register_tool("sess-3", "turn-3", "t-3".to_owned(), tx)
                .await;
            // Yield so the abort can land while we hold the guard.
            tokio::task::yield_now().await;
        });
        // Give task time to register and yield.
        tokio::task::yield_now().await;
        handle.abort();
        // Await the aborted handle so drop runs.
        let _ = handle.await;
        // Guard::drop must have removed the entry synchronously on abort.
        assert!(reg.inner.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn concurrent_turns_same_session_route_by_turn_and_tool() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx_a, mut rx_a) = mpsc::channel::<ApprovalDecision>(4);
        let (tx_b, mut rx_b) = mpsc::channel::<ApprovalDecision>(4);
        let _guard_a = reg.register_turn("sess".to_owned(), "turn-a".to_owned());
        let _guard_b = reg.register_turn("sess".to_owned(), "turn-b".to_owned());
        reg.register_tool("sess", "turn-a", "tool-a".to_owned(), tx_a)
            .await;
        reg.register_tool("sess", "turn-b", "tool-b".to_owned(), tx_b)
            .await;

        assert!(
            !reg.try_send(
                Some("sess"),
                "turn-a",
                "tool-b",
                ApprovalDecision {
                    tool_id: "tool-b".to_owned(),
                    choice: ApprovalChoice::Approved,
                }
            )
            .await,
            "stale tool id must not route to another turn"
        );
        assert!(
            reg.try_send(
                Some("sess"),
                "turn-b",
                "tool-b",
                ApprovalDecision {
                    tool_id: "tool-b".to_owned(),
                    choice: ApprovalChoice::Denied,
                }
            )
            .await
        );

        assert!(rx_a.try_recv().is_err());
        let decision = rx_b.recv().await.expect("turn-b decision");
        assert_eq!(decision.tool_id, "tool-b");
        assert_eq!(decision.choice, ApprovalChoice::Denied);
    }

    #[tokio::test]
    async fn dropping_old_guard_does_not_remove_new_turn() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx_a, _rx_a) = mpsc::channel::<ApprovalDecision>(4);
        let (tx_b, _rx_b) = mpsc::channel::<ApprovalDecision>(4);
        let guard_a = reg.register_turn("sess".to_owned(), "turn-a".to_owned());
        let _guard_b = reg.register_turn("sess".to_owned(), "turn-b".to_owned());
        reg.register_tool("sess", "turn-a", "tool-a".to_owned(), tx_a)
            .await;
        reg.register_tool("sess", "turn-b", "tool-b".to_owned(), tx_b)
            .await;

        drop(guard_a);

        let map = reg.inner.lock().expect("lock");
        assert!(!map.contains_key(&ApprovalKey {
            turn_id: "turn-a".to_owned(),
            tool_id: "tool-a".to_owned(),
        }));
        assert!(map.contains_key(&ApprovalKey {
            turn_id: "turn-b".to_owned(),
            tool_id: "tool-b".to_owned(),
        }));
    }
}
