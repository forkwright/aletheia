use crate::api::types::Session;
use crate::id::NousId;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentStatus {
    Idle,
    Working,
    Streaming,
    Compacting,
}

/// The name and start time of a currently-running tool call, set and cleared atomically.
#[derive(Debug, Clone)]
pub struct ActiveTool {
    pub name: String,
    pub started_at: std::time::Instant,
}

/// An available tool and its current enablement state.
#[derive(Debug, Clone)]
#[expect(
    dead_code,
    reason = "name is stored for future tool-detail display; only enabled is read by the status bar indicator"
)]
pub struct ToolSummary {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct AgentState {
    pub id: NousId,
    pub name: String,
    /// Pre-lowercased `name`, cached at ingestion to avoid per-frame allocation in view code.
    pub name_lower: String,
    pub emoji: Option<String>,
    pub status: AgentStatus,
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
