use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Tool exposed through the ops inventory surface.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ActiveTool {
    /// Tool display name.
    pub name: String,
    /// Tool identifier used by the registry.
    pub id: String,
}

/// A recorded tool call from the ops history surface.
///
/// NOTE: the current runtime does not persist a chronological history of tool
/// calls, so the handler returns an empty list until that store exists.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolHistoryEntry {
    /// Tool name.
    pub name: String,
    /// Whether the call ended in an error outcome.
    pub is_error: bool,
    /// Call duration in milliseconds.
    pub duration_ms: u64,
}

/// Response payload for `GET /api/v1/ops/tools`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OpsToolsResponse {
    /// Currently active tool registry entries.
    pub active_tools: Vec<ActiveTool>,
    /// Historical tool calls, if the runtime has a history source.
    pub tool_history: Vec<ToolHistoryEntry>,
    /// Total recorded tool calls from organon metrics.
    pub total_calls: u64,
    /// Total recorded error calls from organon metrics.
    pub total_errors: u64,
}
