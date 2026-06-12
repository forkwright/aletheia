use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A tool definition from the live registry catalog.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolCatalogEntry {
    /// Tool display name.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// Tool identifier used by the registry.
    pub id: String,
}

/// A currently-running tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LiveInvocationEntry {
    /// Invocation identifier.
    pub id: u64,
    /// Tool name being invoked.
    pub tool_name: String,
    /// Elapsed time since the invocation started, in milliseconds.
    pub elapsed_ms: u64,
}

/// Response payload for `GET /api/v1/ops/tools`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OpsToolsResponse {
    /// Tool definitions from the live registry catalog.
    pub catalog: Vec<ToolCatalogEntry>,
    /// Currently-running tool invocations.
    pub live_invocations: Vec<LiveInvocationEntry>,
    /// Total recorded tool calls from organon metrics.
    pub total_calls: u64,
    /// Total recorded error calls from organon metrics.
    pub total_errors: u64,
    /// Whether chronological tool-call history is unavailable.
    ///
    /// The current runtime does not persist a per-call history, so this is
    /// `true` until a history store is added.
    pub history_unavailable: bool,
}
