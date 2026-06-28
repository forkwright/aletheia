// WHY: wire DTO
//! Session endpoint wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Body for `POST /api/v1/sessions`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSessionRequest {
    /// Target nous agent to bind the session to.
    pub nous_id: String,
    /// Client-chosen key for session deduplication.
    pub session_key: String,
}

/// Body for `PUT /api/v1/sessions/{id}/name`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameSessionRequest {
    /// New display name for the session.
    pub name: String,
}

/// Body for `POST /api/v1/sessions/{id}/messages`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SendMessageRequest {
    /// User message text.
    pub content: String,
}

/// Body for `POST /api/v1/sessions/stream` (turn streaming protocol).
#[derive(Debug, Deserialize)]
pub struct StreamTurnRequest {
    /// Target nous agent ID.
    pub nous_id: String,
    /// User message text.
    pub message: String,
    /// Session key for deduplication (defaults to "main").
    #[serde(default = "super::default_session_key")]
    pub session_key: String,
    /// Client-generated ULID that identifies one user action for idempotency.
    #[serde(default, alias = "clientTurnId")]
    pub client_turn_id: Option<String>,
}

/// Query parameters for `GET /api/v1/sessions`.
#[derive(Debug, Deserialize)]
pub struct ListSessionsParams {
    /// Filter sessions by agent ID.
    pub nous_id: Option<String>,
    /// Case-insensitive search across session id, key, status, and display name.
    pub search: Option<String>,
    /// Filter sessions by lifecycle status (`active`, `archived`, `distilled`).
    pub status: Option<String>,
    /// Maximum number of sessions to return (default 50, max 1000).
    pub limit: Option<u32>,
    /// Cursor token from a previous response's `next_cursor` field.
    #[serde(default)]
    pub after: Option<String>,
}

/// Query parameters for `GET /api/v1/sessions/{id}/history`.
#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    /// Maximum number of messages to return.
    pub limit: Option<u32>,
    /// Return messages with `seq` strictly less than this value.
    pub before: Option<i64>,
}

/// Response for `GET /api/v1/sessions` (list).
///
/// Uses the standard paginated envelope. The `items` field contains
/// `SessionListItem` values; `has_more` and `next_cursor` enable paging.
pub type ListSessionsResponse = crate::pagination::PaginatedResponse<SessionListItem>;

/// Session summary for list endpoints.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionListItem {
    /// Session identifier.
    pub id: String,
    /// Nous agent that owns this session.
    pub nous_id: String,
    /// Client-chosen deduplication key.
    pub session_key: String,
    /// Lifecycle status (e.g. `"active"`, `"archived"`, `"distilled"`).
    pub status: String,
    /// Total messages stored in this session.
    pub message_count: i64,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
    /// Human-readable name, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Session metadata returned by create and get endpoints.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SessionResponse {
    /// Session identifier.
    pub id: String,
    /// Nous agent owning this session.
    pub nous_id: String,
    /// Client-chosen deduplication key.
    pub session_key: String,
    /// Lifecycle status (e.g. `"active"`, `"archived"`, `"distilled"`).
    pub status: String,
    /// LLM model used for this session, if set.
    pub model: Option<String>,
    /// Human-readable display name, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Total messages stored in this session.
    pub message_count: i64,
    /// Estimated total tokens across all messages.
    pub token_count_estimate: i64,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
}

/// Response for `GET /api/v1/sessions/{id}/history`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HistoryResponse {
    /// Conversation messages in chronological order.
    pub messages: Vec<HistoryMessage>,
}

/// A single message in the conversation history.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HistoryMessage {
    /// Database row ID.
    pub id: i64,
    /// Sequence number within the session.
    pub seq: i64,
    /// Message role (`"user"`, `"assistant"`, `"tool"`).
    pub role: String,
    /// Message text content.
    pub content: String,
    /// Tool call ID if this is a tool result message.
    pub tool_call_id: Option<String>,
    /// Tool name if this is a tool result message.
    pub tool_name: Option<String>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

/// Replay-faithful export for a single session.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionReplayResponse {
    /// Replay export schema version.
    pub version: u32,
    /// Export kind marker so consumers do not confuse this with transcript-only output.
    pub export_type: String,
    /// ISO 8601 timestamp when the server generated this export.
    pub exported_at: String,
    /// Session metadata.
    pub session: ReplaySession,
    /// Full raw message history, including distilled rows.
    pub messages: Vec<ReplayMessage>,
    /// Durable token usage rows keyed by turn sequence.
    pub usage_records: Vec<ReplayUsageRecord>,
    /// Structured tool audit rows keyed by turn sequence.
    pub tool_audit_records: Vec<ReplayToolAuditRecord>,
    /// Durable turn lifecycle records parsed from the session note log.
    pub turn_attempts: Vec<ReplayTurnAttempt>,
}

/// Session metadata included in replay exports.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplaySession {
    /// Session identifier.
    pub id: String,
    /// Nous agent that owns this session.
    pub nous_id: String,
    /// Client-chosen session key.
    pub session_key: String,
    /// Lifecycle status.
    pub status: String,
    /// Session type.
    pub session_type: String,
    /// Model configured for the session, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Total stored message count.
    pub message_count: i64,
    /// Estimated token count across stored messages.
    pub token_count_estimate: i64,
    /// Number of distillation passes recorded for this session.
    pub distillation_count: i64,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
    /// Parent session identifier for subtask lineage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// External thread identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Transport that originated the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    /// Human-readable display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Most recent input token count.
    pub last_input_tokens: i64,
    /// Bootstrap hash used to detect context changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap_hash: Option<String>,
    /// Last distillation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_distilled_at: Option<String>,
    /// Computed context token estimate.
    pub computed_context_tokens: i64,
}

/// Message row included in replay exports.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplayMessage {
    /// Database row ID.
    pub id: i64,
    /// Sequence number within the session.
    pub seq: i64,
    /// Message role.
    pub role: String,
    /// Message text content.
    pub content: String,
    /// Tool call ID when this row is linked to a tool call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool name when this row is linked to a tool call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Estimated token count for this message.
    pub token_estimate: i64,
    /// Whether this row was produced by distillation.
    pub is_distilled: bool,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

/// Token usage row included in replay exports.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplayUsageRecord {
    /// Turn sequence number shared with audit records.
    pub turn_seq: i64,
    /// Input tokens consumed.
    pub input_tokens: i64,
    /// Output tokens generated.
    pub output_tokens: i64,
    /// Prompt-cache tokens read.
    pub cache_read_tokens: i64,
    /// Prompt-cache tokens written.
    pub cache_write_tokens: i64,
    /// Model used for this turn, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Structured tool audit row included in replay exports.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplayToolAuditRecord {
    /// Store-assigned chronological ID.
    pub id: i64,
    /// Nous agent that requested the tool.
    pub nous_id: String,
    /// Turn sequence number shared with usage rows.
    pub turn_seq: i64,
    /// Provider/tool-use identifier for this call.
    pub tool_call_id: String,
    /// Registered tool name.
    pub tool_name: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the tool result was an error.
    pub is_error: bool,
    /// Stable outcome label.
    pub outcome: String,
    /// Bounded tool result text captured from the execution path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Approval outcome applied before execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<String>,
    /// HMAC receipt token emitted for this tool result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

/// Durable turn lifecycle record included in replay exports.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplayTurnAttempt {
    /// Turn-attempt record schema version.
    pub version: u32,
    /// Canonical turn ULID.
    pub turn_id: String,
    /// Session this turn belongs to.
    pub session_id: String,
    /// Nous agent that owns this turn.
    pub nous_id: String,
    /// Current lifecycle status.
    pub status: String,
    /// Pipeline stage that emitted this record, when relevant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// Human-readable error code for failed states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Redacted error message for failed states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Provider/model context, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Number of messages already persisted for finalize-pending records.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_persisted: Option<usize>,
    /// Expected total messages for finalize-pending records.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_messages: Option<usize>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}
