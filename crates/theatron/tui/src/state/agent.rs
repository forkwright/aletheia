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

#[derive(Debug, Clone)]
pub struct AgentState {
    pub id: NousId,
    pub name: String,
    /// Pre-lowercased `name`, cached at ingestion to avoid per-frame allocation in view code.
    pub name_lower: String,
    pub emoji: Option<String>,
    pub status: AgentStatus,
    pub active_tool: Option<String>,
    pub tool_started_at: Option<std::time::Instant>,
    pub sessions: Vec<Session>,
    pub model: Option<String>,
    pub compaction_stage: Option<String>,
    /// Indicates this agent completed a turn while not focused.
    /// Cleared when the user switches to this agent.
    pub has_notification: bool,
}
