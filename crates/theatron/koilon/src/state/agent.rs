use crate::api::types::Session;
use crate::id::NousId;
use crate::state::ops::ToolMetadata;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentStatus {
    Idle,
    Working,
    Streaming,
    Compacting,
}

/// Backend actor lifecycle for this agent, independent of the TUI's local
/// turn-processing [`AgentStatus`] above (that enum tracks whether the TUI
/// is mid-turn; this one tracks whether the backend actor is even alive).
/// Derived from the raw status string on `skene::api::types::Agent`, which
/// mirrors `pylon::handlers::nous_dto::NousSummary::status` (#4641):
/// `"active"`, `"idle"`, `"dormant"`, `"degraded"`, or `"unknown"` (actor
/// never spawned, unreachable, or the status query timed out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BackendHealth {
    /// Actor is live (`"active"` or `"idle"` on the wire).
    Healthy,
    /// Actor intentionally not spawned (`"dormant"` on the wire).
    Dormant,
    /// Actor reported an error state (`"degraded"` on the wire).
    Degraded,
    /// Unrecognized, missing, or `"unknown"` status: never spawned,
    /// unreachable, or the status query timed out. An unreported status
    /// must never render as healthy.
    Unknown,
}

impl BackendHealth {
    /// Derive the backend health tier from the raw wire status string.
    #[must_use]
    pub fn from_status(status: Option<&str>) -> Self {
        match status {
            Some("active" | "idle") => Self::Healthy,
            Some("dormant") => Self::Dormant,
            Some("degraded") => Self::Degraded,
            _ => Self::Unknown,
        }
    }
}

/// The name and start time of a currently-running tool call, set and cleared atomically.
#[derive(Debug, Clone)]
pub struct ActiveTool {
    pub name: String,
    pub started_at: std::time::Instant,
}

/// An available tool and its current enablement state.
#[derive(Debug, Clone)]
pub struct ToolSummary {
    pub name: String,
    pub enabled: bool,
    pub metadata: ToolMetadata,
}

#[derive(Debug, Clone)]
pub struct AgentState {
    pub id: NousId,
    pub name: String,
    /// Pre-lowercased `name`, cached at ingestion to avoid per-frame allocation in view code.
    pub name_lower: String,
    pub emoji: Option<String>,
    pub status: AgentStatus,
    /// Backend actor lifecycle, reported independently of `status` above.
    pub backend_health: BackendHealth,
    pub active_tool: Option<ActiveTool>,
    pub sessions: Vec<Session>,
    pub model: Option<String>,
    pub compaction_stage: Option<String>,
    /// Set when distillation completes; cleared after 3-second auto-dismiss delay.
    pub distill_completed_at: Option<std::time::Instant>,
    /// Number of unread messages since the user last focused this agent.
    /// Cleared when the user switches to this agent.
    pub unread_count: u32,
    /// Available tools and their enablement state, fetched from the API.
    pub tools: Vec<ToolSummary>,
}

#[cfg(test)]
mod tests {
    use super::BackendHealth;

    #[test]
    fn backend_health_maps_known_wire_values() {
        assert_eq!(
            BackendHealth::from_status(Some("active")),
            BackendHealth::Healthy
        );
        assert_eq!(
            BackendHealth::from_status(Some("idle")),
            BackendHealth::Healthy
        );
        assert_eq!(
            BackendHealth::from_status(Some("dormant")),
            BackendHealth::Dormant
        );
        assert_eq!(
            BackendHealth::from_status(Some("degraded")),
            BackendHealth::Degraded
        );
    }

    #[test]
    fn backend_health_defaults_unreported_to_unknown() {
        // WHY: a missing or unrecognized status must never read as healthy.
        assert_eq!(
            BackendHealth::from_status(Some("unknown")),
            BackendHealth::Unknown
        );
        assert_eq!(
            BackendHealth::from_status(Some("garbage")),
            BackendHealth::Unknown
        );
        assert_eq!(BackendHealth::from_status(None), BackendHealth::Unknown);
    }
}
