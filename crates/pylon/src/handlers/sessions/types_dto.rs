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
    /// Lifecycle status (e.g. `"active"`, `"archived"`).
    pub status: String,
    /// LLM model associated with this session, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Total messages stored in this session.
    pub message_count: i64,
    /// Estimated total tokens across all messages.
    pub token_count_estimate: i64,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
    /// Human-readable name, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Session metadata returned by create and get endpoints.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionResponse {
    /// Session identifier.
    pub id: String,
    /// Nous agent owning this session.
    pub nous_id: String,
    /// Client-chosen deduplication key.
    pub session_key: String,
    /// Lifecycle status (e.g. `"active"`, `"archived"`).
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
