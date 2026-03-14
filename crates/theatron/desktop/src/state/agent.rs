//! Agent registry and connection status types.
//!
//! In Dioxus, `Signal<Vec<AgentEntry>>` lives in context. The sidebar reads it.
//! `Signal<Option<NousId>>` tracks the focused agent. `Signal<ConnectionStatus>`
//! drives the status bar indicator.

use super::chat::NousId;

/// An agent in the registry, displayed in the sidebar.
#[derive(Debug, Clone)]
pub struct AgentEntry {
    pub id: NousId,
    pub name: String,
    pub emoji: Option<String>,
    pub model: Option<String>,
    pub status: AgentStatus,
    pub active_tool: Option<String>,
    pub has_notification: bool,
}

/// Agent lifecycle status, driven by SSE `StatusUpdate` events.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentStatus {
    #[default]
    Idle,
    Working,
    Streaming,
    Compacting,
}

impl AgentStatus {
    /// Short label for status bar display.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Streaming => "streaming",
            Self::Compacting => "compacting",
        }
    }

    /// Whether the agent is doing work (any non-idle state).
    #[must_use]
    pub fn is_busy(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

/// SSE connection health, shown in the status bar.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    /// Connected and receiving events.
    #[default]
    Connected,
    /// Disconnected, attempting reconnect.
    Reconnecting {
        /// Number of reconnect attempts so far.
        attempt: u32,
    },
    /// Permanently failed (auth error, server gone).
    Failed { reason: String },
}

impl ConnectionStatus {
    #[must_use]
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_status_default_is_idle() {
        assert_eq!(AgentStatus::default(), AgentStatus::Idle);
    }

    #[test]
    fn agent_status_label() {
        assert_eq!(AgentStatus::Idle.label(), "idle");
        assert_eq!(AgentStatus::Working.label(), "working");
        assert_eq!(AgentStatus::Streaming.label(), "streaming");
        assert_eq!(AgentStatus::Compacting.label(), "compacting");
    }

    #[test]
    fn agent_status_is_busy() {
        assert!(!AgentStatus::Idle.is_busy());
        assert!(AgentStatus::Working.is_busy());
        assert!(AgentStatus::Streaming.is_busy());
        assert!(AgentStatus::Compacting.is_busy());
    }

    #[test]
    fn connection_status_default_connected() {
        let status = ConnectionStatus::default();
        assert!(status.is_connected());
    }

    #[test]
    fn connection_status_reconnecting() {
        let status = ConnectionStatus::Reconnecting { attempt: 3 };
        assert!(!status.is_connected());
    }

    #[test]
    fn connection_status_failed() {
        let status = ConnectionStatus::Failed {
            reason: "auth expired".to_string(),
        };
        assert!(!status.is_connected());
    }
}
