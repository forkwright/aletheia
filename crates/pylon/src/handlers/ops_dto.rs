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

/// A recent structured tool invocation from durable turn audit history.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ToolHistoryEntry {
    /// Store-assigned chronological identifier.
    pub id: i64,
    /// Session this tool call belongs to.
    pub session_id: String,
    /// Agent that requested the tool call.
    pub nous_id: String,
    /// Turn sequence shared with usage records.
    pub turn_seq: i64,
    /// Provider/tool-use identifier for this call.
    pub tool_call_id: String,
    /// Registered tool name.
    pub tool_name: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the call produced an error result.
    pub is_error: bool,
    /// Stable outcome label, currently `"success"` or `"error"`.
    pub outcome: String,
    /// Bounded tool result text captured from the execution path.
    pub result: Option<String>,
    /// Approval outcome applied before execution, when known.
    pub approval: Option<String>,
    /// Receipt availability state, either `"present"` or `"absent"`.
    pub receipt_state: String,
    /// HMAC receipt token emitted for this tool result, when present.
    pub receipt: Option<String>,
    /// ISO 8601 timestamp when this audit row was written.
    pub created_at: String,
}

/// Response payload for `GET /api/v1/ops/tools`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OpsToolsResponse {
    /// Tool definitions from the live registry catalog.
    pub catalog: Vec<ToolCatalogEntry>,
    /// Currently-running tool invocations.
    pub live_invocations: Vec<LiveInvocationEntry>,
    /// Recent durable tool-call audit records, newest first.
    pub history: Vec<ToolHistoryEntry>,
    /// Total recorded tool calls from organon metrics.
    pub total_calls: u64,
    /// Total recorded error calls from organon metrics.
    pub total_errors: u64,
    /// Whether chronological tool-call history is unavailable.
    ///
    /// `true` only when the history store cannot be read.
    pub history_unavailable: bool,
}
