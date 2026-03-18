//! Event-derived reactive state from the global SSE stream.
//!
//! Each struct here models state that Dioxus components read via signals.
//! The [`SseEventRouter`](crate::services::sse::SseEventRouter) writes into
//! an `EventState` as SSE events arrive; in production each field becomes
//! a `Signal<T>` or `Store<T>`.

use std::collections::HashMap;

use crate::api::types::{ActiveTurn, ConnectionState, NousId};

/// Aggregate state derived from global SSE events.
///
/// In Dioxus, each field maps to a signal:
///
/// ```text
/// let active_turns = use_signal(Vec::new);
/// let agent_statuses = use_signal(HashMap::new);
/// let distillation = use_signal(HashMap::new);
/// let sse_connection = use_signal(|| SseConnectionState::Disconnected);
/// ```
#[derive(Debug, Clone)]
pub struct EventState {
    /// Currently active agent turns. Populated on `Init`, updated by
    /// `TurnBefore` (add) and `TurnAfter` (remove).
    pub active_turns: Vec<ActiveTurn>,

    /// Per-agent status string from `StatusUpdate` events.
    /// The string value maps to [`AgentStatus`](crate::state::agent::AgentStatus)
    /// at the component layer.
    pub agent_statuses: HashMap<NousId, String>,

    /// Per-agent distillation progress from `Distill*` events.
    pub distillation: HashMap<NousId, DistillationProgress>,

    /// SSE connection lifecycle state.
    pub connection: SseConnectionState,
}

impl EventState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            active_turns: Vec::new(),
            agent_statuses: HashMap::new(),
            distillation: HashMap::new(),
            connection: SseConnectionState::Disconnected,
        }
    }
}

impl Default for EventState {
    fn default() -> Self {
        Self::new()
    }
}

/// SSE connection lifecycle state, separate from the server connection
/// (P604). This tracks specifically whether we are receiving SSE events.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SseConnectionState {
    /// Not connected to the SSE stream.
    Disconnected,
    /// Actively receiving events.
    Connected,
    /// Lost connection, attempting to reconnect.
    Reconnecting { attempt: u32 },
}

impl SseConnectionState {
    /// Whether the SSE stream is actively connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Map to the API-layer [`ConnectionState`] for components that
    /// consume the generic type.
    #[must_use]
    pub fn to_connection_state(&self) -> ConnectionState {
        match self {
            Self::Disconnected => ConnectionState::Disconnected,
            Self::Connected => ConnectionState::Connected,
            Self::Reconnecting { attempt } => ConnectionState::Reconnecting { attempt: *attempt },
        }
    }
}

/// Distillation (memory compaction) progress for a single agent.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DistillationProgress {
    /// Distillation is in progress but no stage reported yet.
    Started,
    /// Currently executing a named stage.
    Stage { stage: String },
    /// Distillation completed.
    Complete,
}

impl DistillationProgress {
    /// Human-readable label for the current phase.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Started => "distilling",
            Self::Stage { stage } => stage.as_str(),
            Self::Complete => "complete",
        }
    }

    /// Whether distillation is still in progress.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_state_default_is_disconnected() {
        let state = EventState::new();
        assert!(state.active_turns.is_empty());
        assert!(state.agent_statuses.is_empty());
        assert!(state.distillation.is_empty());
        assert_eq!(state.connection, SseConnectionState::Disconnected);
    }

    #[test]
    fn sse_connection_state_is_connected() {
        assert!(!SseConnectionState::Disconnected.is_connected());
        assert!(SseConnectionState::Connected.is_connected());
        assert!(!SseConnectionState::Reconnecting { attempt: 1 }.is_connected());
    }

    #[test]
    fn sse_connection_state_to_connection_state() {
        assert_eq!(
            SseConnectionState::Connected.to_connection_state(),
            ConnectionState::Connected,
        );
        assert_eq!(
            SseConnectionState::Reconnecting { attempt: 3 }.to_connection_state(),
            ConnectionState::Reconnecting { attempt: 3 },
        );
    }

    #[test]
    fn distillation_progress_label() {
        assert_eq!(DistillationProgress::Started.label(), "distilling");
        assert_eq!(
            DistillationProgress::Stage {
                stage: "extracting".to_string()
            }
            .label(),
            "extracting"
        );
        assert_eq!(DistillationProgress::Complete.label(), "complete");
    }

    #[test]
    fn distillation_progress_is_active() {
        assert!(DistillationProgress::Started.is_active());
        assert!(
            DistillationProgress::Stage {
                stage: "x".to_string()
            }
            .is_active()
        );
        assert!(!DistillationProgress::Complete.is_active());
    }
}
