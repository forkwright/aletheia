//! Per-turn approval-decision sender registry (#3958, ADR-005).
//!
//! Pylon's streaming handler registers a pending key when a turn emits
//! `tool_approval_required`; approval handlers route the operator's decision
//! into the matching nous-side gate by turn and tool id, with session id kept
//! as context for session-scoped API routes. The guard unregisters only keys
//! for the turn that created it.
//!
//! WHY(#6822): keys that leave the pending map are remembered as bounded,
//! time-limited tombstones recording *why* they left — operator-resolved,
//! gate-timed-out, or turn-ended — so a late resolve can be answered with
//! that disposition instead of collapsing into "never existed".

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use nous::approval::{ApprovalChoice, ApprovalDecision};
use tokio::sync::mpsc;
use tokio::time::Instant;

/// How long the registry remembers why a pending approval left the map.
///
/// WHY(#6822): the window bounds tombstone memory while covering the realistic
/// late-client cases — a UI rendering a stale approval card, or an operator
/// answering after the gate's default-deny (approval timeouts are minutes, so
/// ten minutes comfortably outlives any pending card a client still shows).
const TOMBSTONE_TTL: Duration = Duration::from_mins(10);

/// Upper bound on retained tombstones across all sessions.
///
/// WHY(#6822): a busy multi-session deployment must not grow the map without
/// bound inside the TTL window; past the cap the soonest-to-expire tombstone
/// is evicted first, degrading the oldest answer back to "unknown".
const MAX_TOMBSTONES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ApprovalKey {
    turn_id: String,
    tool_id: String,
}

struct ApprovalEntry {
    session_id: String,
    sender: mpsc::Sender<ApprovalDecision>,
}

/// Why a previously pending approval is no longer routable (#6822).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDisposition {
    /// An operator decision was routed through this registry.
    Resolved {
        /// The decision that won.
        choice: ApprovalChoice,
    },
    /// The nous-side gate resolved the approval with no decision routed
    /// through this registry: the server-side timeout default-denied it.
    TimedOut,
    /// The turn ended (completed, cancelled, or disconnected) while the
    /// approval was still pending.
    TurnEnded,
}

impl ApprovalDisposition {
    /// Stable wire string carried in the error envelope's `details.reason`.
    #[must_use]
    pub const fn as_reason_str(self) -> &'static str {
        match self {
            Self::Resolved { .. } => "already_resolved",
            Self::TimedOut => "timed_out",
            Self::TurnEnded => "turn_ended",
        }
    }

    /// Wire string of the decision an already-resolved approval received.
    #[must_use]
    pub const fn resolved_decision(self) -> Option<&'static str> {
        match self {
            Self::Resolved { choice } => Some(choice.as_wire_str()),
            Self::TimedOut | Self::TurnEnded => None,
        }
    }
}

/// Outcome of routing an operator decision to a pending approval (#6822).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteOutcome {
    /// The decision reached the active turn's gate.
    Routed,
    /// The approval existed but is gone; the disposition says how it went.
    Gone(ApprovalDisposition),
    /// No approval was ever registered under this key — or its tombstone
    /// aged out of the retention window.
    Unknown,
}

struct Tombstone {
    session_id: String,
    disposition: ApprovalDisposition,
    expires_at: Instant,
}

#[derive(Default)]
struct Inner {
    pending: HashMap<ApprovalKey, ApprovalEntry>,
    tombstones: HashMap<ApprovalKey, Tombstone>,
}

impl Inner {
    /// Record why `key` left the pending map, sweeping expired tombstones and
    /// enforcing [`MAX_TOMBSTONES`] (soonest-to-expire evicted first).
    fn bury(&mut self, key: ApprovalKey, session_id: String, disposition: ApprovalDisposition) {
        let now = Instant::now();
        self.tombstones.retain(|_, stone| stone.expires_at > now);
        if self.tombstones.len() >= MAX_TOMBSTONES {
            let evict = self
                .tombstones
                .iter()
                .min_by_key(|(_, stone)| stone.expires_at)
                .map(|(key, _)| key.clone());
            if let Some(evict) = evict {
                self.tombstones.remove(&evict);
            }
        }
        self.tombstones.insert(
            key,
            Tombstone {
                session_id,
                disposition,
                expires_at: now + TOMBSTONE_TTL,
            },
        );
    }
}

/// Concurrent map from `(turn_id, tool_id)` → approval-decision sender, plus
/// tombstones for keys that left it (#6822).
#[derive(Default)]
pub struct ApprovalRegistry {
    inner: Mutex<Inner>,
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
    #[expect(
        clippy::unused_async,
        reason = "synchronous std::sync::Mutex critical section; kept async to preserve the registry API"
    )]
    pub async fn register_tool(
        &self,
        session_id: &str,
        turn_id: &str,
        tool_id: String,
        sender: mpsc::Sender<ApprovalDecision>,
    ) {
        let key = ApprovalKey {
            turn_id: turn_id.to_owned(),
            tool_id,
        };
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // WHY(#6822): a fresh registration supersedes any stale tombstone for
        // the same key — the live entry is the truth again.
        inner.tombstones.remove(&key);
        inner.pending.insert(
            key,
            ApprovalEntry {
                session_id: session_id.to_owned(),
                sender,
            },
        );
    }

    /// Look up the sender for `(turn_id, tool_id)` and send a decision.
    ///
    /// When `session_id` is `Some`, it must match the pending entry's (or
    /// tombstone's) session context; a mismatch reports [`RouteOutcome::Unknown`]
    /// so a wrong-session caller learns nothing about another session's
    /// approvals. A routed decision leaves a `Resolved` tombstone; a sender
    /// whose receiver already hung up leaves a `TurnEnded` one.
    #[expect(
        clippy::unused_async,
        reason = "synchronous std::sync::Mutex critical section; kept async to preserve the registry API"
    )]
    pub async fn try_send(
        &self,
        session_id: Option<&str>,
        turn_id: &str,
        tool_id: &str,
        decision: ApprovalDecision,
    ) -> RouteOutcome {
        let key = ApprovalKey {
            turn_id: turn_id.to_owned(),
            tool_id: tool_id.to_owned(),
        };
        let choice = decision.choice;
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(entry) = inner.pending.get(&key) {
            if let Some(session_id) = session_id
                && entry.session_id != session_id
            {
                return RouteOutcome::Unknown;
            }
            let sender = entry.sender.clone();
            let owner = entry.session_id.clone();
            inner.pending.remove(&key);
            return if sender.try_send(decision).is_ok() {
                inner.bury(key, owner, ApprovalDisposition::Resolved { choice });
                RouteOutcome::Routed
            } else {
                // WHY(#6822): the receiver hanging up while the key is still
                // registered means the turn machinery tore down before its
                // guard dropped — the turn is over, not the gate mid-wait.
                inner.bury(key, owner, ApprovalDisposition::TurnEnded);
                RouteOutcome::Gone(ApprovalDisposition::TurnEnded)
            };
        }

        if let Some(stone) = inner.tombstones.get(&key) {
            if stone.expires_at <= Instant::now() {
                inner.tombstones.remove(&key);
                return RouteOutcome::Unknown;
            }
            if let Some(session_id) = session_id
                && stone.session_id != session_id
            {
                return RouteOutcome::Unknown;
            }
            return RouteOutcome::Gone(stone.disposition);
        }

        RouteOutcome::Unknown
    }

    /// Record that the nous-side gate resolved `(turn_id, tool_id)` without a
    /// decision routed through this registry.
    ///
    /// WHY(#6822): on this path the only sender is the approvals endpoint, and
    /// a routed decision removes the key before the gate can observe it — so a
    /// gate resolution that finds the key still registered can only be the
    /// server-side timeout default-deny. No-op when the key is absent.
    pub fn mark_gate_resolved(&self, session_id: &str, turn_id: &str, tool_id: &str) {
        let key = ApprovalKey {
            turn_id: turn_id.to_owned(),
            tool_id: tool_id.to_owned(),
        };
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner
            .pending
            .get(&key)
            .is_some_and(|entry| entry.session_id == session_id)
        {
            inner.pending.remove(&key);
            inner.bury(key, session_id.to_owned(), ApprovalDisposition::TimedOut);
        }
    }

    fn remove_turn(&self, session_id: &str, turn_id: &str) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let keys: Vec<ApprovalKey> = inner
            .pending
            .iter()
            .filter(|(key, entry)| key.turn_id == turn_id && entry.session_id == session_id)
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            if let Some(entry) = inner.pending.remove(&key) {
                inner.bury(key, entry.session_id, ApprovalDisposition::TurnEnded);
            }
        }
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
        // WHY(#5737): Perform removal synchronously from Drop so cleanup is
        // deterministic and cannot be lost to a fire-and-forget spawn during
        // runtime shutdown. The inner lock is a std::sync::Mutex, so Drop can
        // hold it without spawning.
        if let (Some(sid), Some(turn_id)) = (self.session_id.take(), self.turn_id.take()) {
            self.registry.remove_turn(&sid, &turn_id);
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use nous::approval::ApprovalChoice;

    use super::*;

    fn decision(tool_id: &str, choice: ApprovalChoice) -> ApprovalDecision {
        ApprovalDecision {
            tool_id: tool_id.to_owned(),
            choice,
        }
    }

    #[tokio::test]
    async fn register_send_remove_roundtrip() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, mut rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess-1".to_owned(), "turn-1".to_owned());
        reg.register_tool("sess-1", "turn-1", "t-1".to_owned(), tx)
            .await;

        assert_eq!(
            reg.try_send(
                Some("sess-1"),
                "turn-1",
                "t-1",
                decision("t-1", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Routed
        );
        let routed = rx.recv().await.expect("decision");
        assert_eq!(routed.tool_id, "t-1");
    }

    #[tokio::test]
    async fn unknown_key_reports_unknown() {
        let reg = ApprovalRegistry::new();
        assert_eq!(
            reg.try_send(
                Some("missing"),
                "turn-x",
                "tool-x",
                decision("tool-x", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Unknown
        );
    }

    #[tokio::test]
    async fn guard_drop_buries_pending_keys_as_turn_ended() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        {
            let _guard = reg.register_turn("sess-2".to_owned(), "turn-2".to_owned());
            reg.register_tool("sess-2", "turn-2", "t-2".to_owned(), tx)
                .await;
            assert!(
                reg.inner
                    .lock()
                    .expect("lock")
                    .pending
                    .contains_key(&ApprovalKey {
                        turn_id: "turn-2".to_owned(),
                        tool_id: "t-2".to_owned(),
                    })
            );
        }
        // WHY(#5737): Removal now runs synchronously inside Drop, so the entry
        // is gone immediately; no yield/sleep is needed.
        assert!(reg.inner.lock().expect("lock").pending.is_empty());
        // WHY(#6822): the key is remembered as turn-ended, not forgotten.
        assert_eq!(
            reg.try_send(
                Some("sess-2"),
                "turn-2",
                "t-2",
                decision("t-2", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Gone(ApprovalDisposition::TurnEnded)
        );
    }

    #[tokio::test]
    async fn routed_decision_buries_already_resolved_with_choice() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess".to_owned(), "turn".to_owned());
        reg.register_tool("sess", "turn", "t".to_owned(), tx).await;

        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Routed
        );
        let outcome = reg
            .try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Denied),
            )
            .await;
        assert_eq!(
            outcome,
            RouteOutcome::Gone(ApprovalDisposition::Resolved {
                choice: ApprovalChoice::Approved
            })
        );
        let RouteOutcome::Gone(disposition) = outcome else {
            unreachable!("asserted Gone above");
        };
        assert_eq!(disposition.as_reason_str(), "already_resolved");
        assert_eq!(disposition.resolved_decision(), Some("approved"));
    }

    #[tokio::test]
    async fn gate_resolution_buries_timed_out() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess".to_owned(), "turn".to_owned());
        reg.register_tool("sess", "turn", "t".to_owned(), tx).await;

        reg.mark_gate_resolved("sess", "turn", "t");
        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Gone(ApprovalDisposition::TimedOut)
        );
    }

    #[tokio::test]
    async fn gate_resolution_after_routed_decision_is_noop() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess".to_owned(), "turn".to_owned());
        reg.register_tool("sess", "turn", "t".to_owned(), tx).await;

        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Denied)
            )
            .await,
            RouteOutcome::Routed
        );
        reg.mark_gate_resolved("sess", "turn", "t");
        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Denied)
            )
            .await,
            RouteOutcome::Gone(ApprovalDisposition::Resolved {
                choice: ApprovalChoice::Denied
            }),
            "an operator-resolved tombstone must not be downgraded to timed_out"
        );
    }

    #[tokio::test]
    async fn dropped_receiver_buries_turn_ended() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess".to_owned(), "turn".to_owned());
        reg.register_tool("sess", "turn", "t".to_owned(), tx).await;
        drop(rx);

        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Gone(ApprovalDisposition::TurnEnded)
        );
    }

    #[tokio::test]
    async fn wrong_session_learns_nothing_from_tombstones() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess".to_owned(), "turn".to_owned());
        reg.register_tool("sess", "turn", "t".to_owned(), tx).await;
        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Routed
        );

        assert_eq!(
            reg.try_send(
                Some("other-session"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Unknown,
            "a wrong-session caller must see the same shape as never-existed"
        );
        assert_eq!(
            reg.try_send(None, "turn", "t", decision("t", ApprovalChoice::Approved))
                .await,
            RouteOutcome::Gone(ApprovalDisposition::Resolved {
                choice: ApprovalChoice::Approved
            }),
            "the unscoped legacy path still sees the disposition"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn tombstones_expire_after_ttl() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess".to_owned(), "turn".to_owned());
        reg.register_tool("sess", "turn", "t".to_owned(), tx).await;
        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Routed
        );

        tokio::time::advance(TOMBSTONE_TTL + Duration::from_secs(1)).await;
        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Unknown,
            "an aged-out tombstone degrades back to unknown"
        );
    }

    #[tokio::test]
    async fn tombstone_count_is_bounded() {
        let reg = Arc::new(ApprovalRegistry::new());
        {
            let _guard = reg.register_turn("sess".to_owned(), "turn".to_owned());
            for i in 0..=MAX_TOMBSTONES {
                let (tx, _rx) = mpsc::channel::<ApprovalDecision>(1);
                reg.register_tool("sess", "turn", format!("t-{i}"), tx)
                    .await;
            }
        }
        assert_eq!(
            reg.inner.lock().expect("lock").tombstones.len(),
            MAX_TOMBSTONES,
            "burials past the cap must evict rather than grow"
        );
    }

    #[tokio::test]
    async fn reregistration_clears_stale_tombstone() {
        let reg = Arc::new(ApprovalRegistry::new());
        let (tx, _rx) = mpsc::channel::<ApprovalDecision>(4);
        let _guard = reg.register_turn("sess".to_owned(), "turn".to_owned());
        reg.register_tool("sess", "turn", "t".to_owned(), tx).await;
        reg.mark_gate_resolved("sess", "turn", "t");

        let (tx2, mut rx2) = mpsc::channel::<ApprovalDecision>(4);
        reg.register_tool("sess", "turn", "t".to_owned(), tx2).await;
        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn",
                "t",
                decision("t", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Routed,
            "a live re-registration supersedes the tombstone"
        );
        assert_eq!(rx2.recv().await.expect("decision").tool_id, "t");
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

        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn-a",
                "tool-b",
                decision("tool-b", ApprovalChoice::Approved)
            )
            .await,
            RouteOutcome::Unknown,
            "stale tool id must not route to another turn"
        );
        assert_eq!(
            reg.try_send(
                Some("sess"),
                "turn-b",
                "tool-b",
                decision("tool-b", ApprovalChoice::Denied)
            )
            .await,
            RouteOutcome::Routed
        );

        assert!(rx_a.try_recv().is_err());
        let routed = rx_b.recv().await.expect("turn-b decision");
        assert_eq!(routed.tool_id, "tool-b");
        assert_eq!(routed.choice, ApprovalChoice::Denied);
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
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let inner = reg.inner.lock().expect("lock");
        assert!(!inner.pending.contains_key(&ApprovalKey {
            turn_id: "turn-a".to_owned(),
            tool_id: "tool-a".to_owned(),
        }));
        assert!(inner.pending.contains_key(&ApprovalKey {
            turn_id: "turn-b".to_owned(),
            tool_id: "tool-b".to_owned(),
        }));
    }
}
