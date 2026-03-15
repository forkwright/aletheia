use crate::api::types::Session;
use crate::id::NousId;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Number of unread messages since the user last focused this agent.
    /// Cleared when the user switches to this agent.
    pub unread_count: u32,
}
