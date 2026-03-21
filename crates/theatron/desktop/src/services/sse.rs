//! SSE event routing service.
//!
//! Routes global SSE events from [`SseConnection`](crate::api::sse::SseConnection)
//! into the [`EventState`] reactive store. In a Dioxus component, this runs
//! inside a `use_coroutine`:
//!
//! ```ignore
//! let mut event_state = use_signal(EventState::new);
//! let mut router = SseEventRouter::new();
//!
//! use_coroutine(|_rx| async move {
//!     let mut sse = SseConnection::connect(client, &base_url, cancel);
//!     while let Some(event) = sse.next().await {
//!         if router.apply(&event) {
//!             event_state.set(router.state().clone());
//!         }
//!     }
//! });
//! ```
//!
//! The router is framework-agnostic: it mutates an [`EventState`] and returns
//! whether the state changed, leaving signal writes to the caller.

use theatron_core::api::types::{ActiveTurn, SseEvent};
use theatron_core::id::NousId;

use crate::state::events::{DistillationProgress, EventState, SseConnectionState};

/// Routes parsed [`SseEvent`]s into [`EventState`].
///
/// Tracks reconnection attempts internally so the caller only needs to
/// forward events from [`SseConnection::next()`](crate::api::sse::SseConnection::next).
pub struct SseEventRouter {
    state: EventState,
    reconnect_attempts: u32,
}

impl SseEventRouter {
    /// Create a new router with default state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: EventState::new(),
            reconnect_attempts: 0,
        }
    }

    /// Immutable access to the current event-derived state.
    #[must_use]
    pub fn state(&self) -> &EventState {
        &self.state
    }

    /// Process an SSE event, updating internal state.
    ///
    /// Returns `true` if the state changed and signals should be notified.
    /// `Ping` events return `false` since they carry no state change.
    pub fn apply(&mut self, event: &SseEvent) -> bool {
        match event {
            SseEvent::Connected => self.on_connected(),
            SseEvent::Disconnected => self.on_disconnected(),
            SseEvent::Init { active_turns } => self.on_init(active_turns),
            SseEvent::TurnBefore {
                nous_id,
                session_id,
                turn_id,
            } => self.on_turn_before(nous_id, session_id, turn_id),
            SseEvent::TurnAfter {
                nous_id,
                session_id,
            } => self.on_turn_after(nous_id, session_id),
            SseEvent::ToolCalled { nous_id, tool_name } => self.on_tool_called(nous_id, tool_name),
            SseEvent::ToolFailed {
                nous_id,
                tool_name,
                error,
            } => self.on_tool_failed(nous_id, tool_name, error),
            SseEvent::StatusUpdate { nous_id, status } => self.on_status_update(nous_id, status),
            SseEvent::SessionCreated { .. } | SseEvent::SessionArchived { .. } => {
                // NOTE: Session lifecycle events are noted but don't update EventState
                // directly: session list refresh is handled by the session service.
                tracing::debug!(?event, "session lifecycle event (no state change)");
                false
            }
            SseEvent::DistillBefore { nous_id } => self.on_distill_before(nous_id),
            SseEvent::DistillStage { nous_id, stage } => self.on_distill_stage(nous_id, stage),
            SseEvent::DistillAfter { nous_id } => self.on_distill_after(nous_id),
            SseEvent::Ping => false,
            _ => {
                tracing::debug!(?event, "unhandled SSE event variant");
                false
            }
        }
    }

    // -- Connection lifecycle ------------------------------------------------

    fn on_connected(&mut self) -> bool {
        self.reconnect_attempts = 0;
        self.state.connection = SseConnectionState::Connected;
        true
    }

    fn on_disconnected(&mut self) -> bool {
        self.reconnect_attempts += 1;
        self.state.connection = SseConnectionState::Reconnecting {
            attempt: self.reconnect_attempts,
        };
        true
    }

    // -- Init ----------------------------------------------------------------

    fn on_init(&mut self, active_turns: &[ActiveTurn]) -> bool {
        self.state.active_turns = active_turns.to_vec();
        true
    }

    // -- Turn lifecycle ------------------------------------------------------

    fn on_turn_before(
        &mut self,
        nous_id: &NousId,
        session_id: &theatron_core::id::SessionId,
        turn_id: &theatron_core::id::TurnId,
    ) -> bool {
        // NOTE: Only add if not already tracked (idempotent).
        let already = self
            .state
            .active_turns
            .iter()
            .any(|t| t.nous_id == *nous_id && t.turn_id == *turn_id);
        if !already {
            self.state.active_turns.push(ActiveTurn {
                nous_id: nous_id.clone(),
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
            });
        }
        true
    }

    fn on_turn_after(
        &mut self,
        nous_id: &NousId,
        session_id: &theatron_core::id::SessionId,
    ) -> bool {
        let before = self.state.active_turns.len();
        self.state.active_turns.retain(|t| {
            !(t.nous_id.as_str() == nous_id.as_str()
                && t.session_id.as_str() == session_id.as_str())
        });
        self.state.active_turns.len() != before
    }

    // -- Tool events ---------------------------------------------------------

    fn on_tool_called(&mut self, nous_id: &NousId, tool_name: &str) -> bool {
        // NOTE: Update agent status to reflect tool activity.
        self.state
            .agent_statuses
            .insert(nous_id.clone(), format!("tool:{tool_name}"));
        true
    }

    fn on_tool_failed(&mut self, nous_id: &NousId, tool_name: &str, error: &str) -> bool {
        tracing::warn!(
            nous_id = nous_id.as_str(),
            tool_name,
            error,
            "tool execution failed"
        );
        self.state
            .agent_statuses
            .insert(nous_id.clone(), format!("tool-failed:{tool_name}"));
        true
    }

    // -- Status update -------------------------------------------------------

    fn on_status_update(&mut self, nous_id: &NousId, status: &str) -> bool {
        let prev = self.state.agent_statuses.get(nous_id);
        if prev.map(String::as_str) == Some(status) {
            return false;
        }
        self.state
            .agent_statuses
            .insert(nous_id.clone(), status.to_string());
        true
    }

    // -- Distillation --------------------------------------------------------

    fn on_distill_before(&mut self, nous_id: &NousId) -> bool {
        self.state
            .distillation
            .insert(nous_id.clone(), DistillationProgress::Started);
        true
    }

    fn on_distill_stage(&mut self, nous_id: &NousId, stage: &str) -> bool {
        self.state.distillation.insert(
            nous_id.clone(),
            DistillationProgress::Stage {
                stage: stage.to_string(),
            },
        );
        true
    }

    fn on_distill_after(&mut self, nous_id: &NousId) -> bool {
        self.state
            .distillation
            .insert(nous_id.clone(), DistillationProgress::Complete);
        true
    }
}

impl Default for SseEventRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theatron_core::id::{NousId, SessionId, TurnId};

    fn nous(id: &str) -> NousId {
        NousId::from(id)
    }

    fn session(id: &str) -> SessionId {
        SessionId::from(id)
    }

    fn turn(id: &str) -> TurnId {
        TurnId::from(id)
    }

    // -- Connection lifecycle ------------------------------------------------

    #[test]
    fn connected_sets_state_and_resets_attempts() {
        let mut router = SseEventRouter::new();
        router.reconnect_attempts = 3;

        let changed = router.apply(&SseEvent::Connected);

        assert!(changed);
        assert_eq!(router.state().connection, SseConnectionState::Connected);
        assert_eq!(router.reconnect_attempts, 0);
    }

    #[test]
    fn disconnected_increments_reconnect_attempts() {
        let mut router = SseEventRouter::new();

        router.apply(&SseEvent::Disconnected);
        assert_eq!(
            router.state().connection,
            SseConnectionState::Reconnecting { attempt: 1 }
        );

        router.apply(&SseEvent::Disconnected);
        assert_eq!(
            router.state().connection,
            SseConnectionState::Reconnecting { attempt: 2 }
        );
    }

    #[test]
    fn connected_after_disconnect_resets_to_connected() {
        let mut router = SseEventRouter::new();

        router.apply(&SseEvent::Disconnected);
        router.apply(&SseEvent::Disconnected);
        router.apply(&SseEvent::Connected);

        assert_eq!(router.state().connection, SseConnectionState::Connected);
        assert_eq!(router.reconnect_attempts, 0);
    }

    // -- Init ----------------------------------------------------------------

    #[test]
    fn init_replaces_active_turns() {
        let mut router = SseEventRouter::new();
        router.apply(&SseEvent::Connected);

        let turns = vec![
            ActiveTurn {
                nous_id: nous("syn"),
                session_id: session("s1"),
                turn_id: turn("t1"),
            },
            ActiveTurn {
                nous_id: nous("mneme"),
                session_id: session("s2"),
                turn_id: turn("t2"),
            },
        ];

        let changed = router.apply(&SseEvent::Init {
            active_turns: turns,
        });

        assert!(changed);
        assert_eq!(router.state().active_turns.len(), 2);
        assert_eq!(&*router.state().active_turns[0].nous_id, "syn");
        assert_eq!(&*router.state().active_turns[1].nous_id, "mneme");
    }

    // -- Turn lifecycle ------------------------------------------------------

    #[test]
    fn turn_before_adds_active_turn() {
        let mut router = SseEventRouter::new();
        router.apply(&SseEvent::Connected);

        router.apply(&SseEvent::TurnBefore {
            nous_id: nous("syn"),
            session_id: session("s1"),
            turn_id: turn("t1"),
        });

        assert_eq!(router.state().active_turns.len(), 1);
        assert_eq!(&*router.state().active_turns[0].turn_id, "t1");
    }

    #[test]
    fn turn_before_is_idempotent() {
        let mut router = SseEventRouter::new();

        router.apply(&SseEvent::TurnBefore {
            nous_id: nous("syn"),
            session_id: session("s1"),
            turn_id: turn("t1"),
        });
        router.apply(&SseEvent::TurnBefore {
            nous_id: nous("syn"),
            session_id: session("s1"),
            turn_id: turn("t1"),
        });

        assert_eq!(router.state().active_turns.len(), 1);
    }

    #[test]
    fn turn_after_removes_active_turn() {
        let mut router = SseEventRouter::new();

        router.apply(&SseEvent::TurnBefore {
            nous_id: nous("syn"),
            session_id: session("s1"),
            turn_id: turn("t1"),
        });
        assert_eq!(router.state().active_turns.len(), 1);

        let changed = router.apply(&SseEvent::TurnAfter {
            nous_id: nous("syn"),
            session_id: session("s1"),
        });

        assert!(changed);
        assert!(router.state().active_turns.is_empty());
    }

    #[test]
    fn turn_after_no_match_returns_false() {
        let mut router = SseEventRouter::new();

        let changed = router.apply(&SseEvent::TurnAfter {
            nous_id: nous("syn"),
            session_id: session("s1"),
        });

        assert!(!changed);
    }

    // -- Status update -------------------------------------------------------

    #[test]
    fn status_update_stores_agent_status() {
        let mut router = SseEventRouter::new();

        let changed = router.apply(&SseEvent::StatusUpdate {
            nous_id: nous("syn"),
            status: "working".to_string(),
        });

        assert!(changed);
        assert_eq!(
            router.state().agent_statuses.get(&nous("syn")),
            Some(&"working".to_string()),
        );
    }

    #[test]
    fn status_update_no_change_returns_false() {
        let mut router = SseEventRouter::new();

        router.apply(&SseEvent::StatusUpdate {
            nous_id: nous("syn"),
            status: "idle".to_string(),
        });

        let changed = router.apply(&SseEvent::StatusUpdate {
            nous_id: nous("syn"),
            status: "idle".to_string(),
        });

        assert!(!changed);
    }

    // -- Tool events ---------------------------------------------------------

    #[test]
    fn tool_called_updates_status() {
        let mut router = SseEventRouter::new();

        let changed = router.apply(&SseEvent::ToolCalled {
            nous_id: nous("syn"),
            tool_name: "read_file".to_string(),
        });

        assert!(changed);
        assert_eq!(
            router.state().agent_statuses.get(&nous("syn")),
            Some(&"tool:read_file".to_string()),
        );
    }

    #[test]
    fn tool_failed_updates_status() {
        let mut router = SseEventRouter::new();

        let changed = router.apply(&SseEvent::ToolFailed {
            nous_id: nous("syn"),
            tool_name: "exec".to_string(),
            error: "timeout".to_string(),
        });

        assert!(changed);
        assert_eq!(
            router.state().agent_statuses.get(&nous("syn")),
            Some(&"tool-failed:exec".to_string()),
        );
    }

    // -- Distillation --------------------------------------------------------

    #[test]
    fn distill_lifecycle() {
        let mut router = SseEventRouter::new();
        let id = nous("syn");

        router.apply(&SseEvent::DistillBefore {
            nous_id: id.clone(),
        });
        assert_eq!(
            router.state().distillation.get(&id),
            Some(&DistillationProgress::Started),
        );

        router.apply(&SseEvent::DistillStage {
            nous_id: id.clone(),
            stage: "extracting".to_string(),
        });
        assert_eq!(
            router.state().distillation.get(&id),
            Some(&DistillationProgress::Stage {
                stage: "extracting".to_string()
            }),
        );

        router.apply(&SseEvent::DistillAfter {
            nous_id: id.clone(),
        });
        assert_eq!(
            router.state().distillation.get(&id),
            Some(&DistillationProgress::Complete),
        );
        assert!(!router.state().distillation[&id].is_active());
    }

    // -- Ping ----------------------------------------------------------------

    #[test]
    fn ping_returns_no_change() {
        let mut router = SseEventRouter::new();
        assert!(!router.apply(&SseEvent::Ping));
    }

    // -- Session events (no state change) ------------------------------------

    #[test]
    fn session_created_returns_no_change() {
        let mut router = SseEventRouter::new();

        let changed = router.apply(&SseEvent::SessionCreated {
            nous_id: nous("syn"),
            session_id: session("s-new"),
        });

        assert!(!changed);
    }

    // -- Full lifecycle integration ------------------------------------------

    #[test]
    fn full_sse_lifecycle() {
        let mut router = SseEventRouter::new();

        // Connect
        router.apply(&SseEvent::Connected);
        assert!(router.state().connection.is_connected());

        // Receive init with one active turn
        router.apply(&SseEvent::Init {
            active_turns: vec![ActiveTurn {
                nous_id: nous("syn"),
                session_id: session("s1"),
                turn_id: turn("t1"),
            }],
        });
        assert_eq!(router.state().active_turns.len(), 1);

        // Agent starts working
        router.apply(&SseEvent::StatusUpdate {
            nous_id: nous("syn"),
            status: "working".to_string(),
        });

        // New turn begins on another agent
        router.apply(&SseEvent::TurnBefore {
            nous_id: nous("mneme"),
            session_id: session("s2"),
            turn_id: turn("t2"),
        });
        assert_eq!(router.state().active_turns.len(), 2);

        // First turn completes
        router.apply(&SseEvent::TurnAfter {
            nous_id: nous("syn"),
            session_id: session("s1"),
        });
        assert_eq!(router.state().active_turns.len(), 1);
        assert_eq!(&*router.state().active_turns[0].nous_id, "mneme");

        // Disconnect and reconnect
        router.apply(&SseEvent::Disconnected);
        assert!(!router.state().connection.is_connected());
        router.apply(&SseEvent::Connected);
        assert!(router.state().connection.is_connected());
    }
}
