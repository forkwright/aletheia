//! Ops/registry introspection DTOs mirroring Pylon's wire shapes
//! (`crates/pylon/src/handlers/ops_dto.rs`). Skene has no dependency on
//! pylon, so these are independent structs kept in sync by the contract
//! tests in `super::tests`.

use serde::{Deserialize, Serialize};

/// A tool definition from the live registry catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's ToolCatalogEntry; self-documenting by name"
)]
pub struct ToolCatalogEntry {
    pub name: String,
    pub description: String,
    pub id: String,
    pub category: String,
    pub reversibility: String,
    pub approval: String,
    pub requires_approval: bool,
    pub destructive: bool,
    pub groups: Vec<String>,
    pub source_plane: String,
    pub metadata_verified: bool,
}

/// A currently-running tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveInvocationEntry {
    /// Invocation identifier.
    pub id: u64,
    /// Tool name being invoked.
    pub tool_name: String,
    /// Elapsed time since the invocation started, in milliseconds.
    pub elapsed_ms: u64,
}

/// A recent structured tool invocation from durable turn audit history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's ToolHistoryEntry; self-documenting by name"
)]
pub struct ToolHistoryEntry {
    pub id: i64,
    pub session_id: String,
    pub nous_id: String,
    pub turn_seq: i64,
    pub tool_call_id: String,
    pub tool_name: String,
    pub duration_ms: u64,
    pub is_error: bool,
    pub outcome: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub approval: Option<String>,
    pub receipt_state: String,
    #[serde(default)]
    pub receipt: Option<String>,
    pub created_at: String,
}

/// Response for `GET /api/v1/ops/tools`.
///
/// Mirrors `pylon::handlers::ops_dto::OpsToolsResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Whether chronological tool-call history is unavailable (the history
    /// store could not be read at all).
    #[serde(default)]
    pub history_unavailable: bool,
    /// Count of `tool_audit` rows that failed to decode and were omitted
    /// from `history` (aletheia#7217). `0` when none were corrupt.
    #[serde(default)]
    pub tool_audit_corrupt_count: usize,
}
