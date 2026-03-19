//! Request and response types for session endpoints.

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

/// Body for `POST /api/v1/sessions/stream` (TUI streaming protocol).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTurnRequest {
    /// Target agent ID.
    #[serde(alias = "agentId")]
    pub agent_id: String,
    /// User message text.
    pub message: String,
    /// Session key for deduplication (defaults to "main").
    #[serde(alias = "sessionKey", default = "default_session_key")]
    pub session_key: String,
}

fn default_session_key() -> String {
    "main".to_owned()
}

/// Query parameters for `GET /api/v1/sessions`.
#[derive(Debug, Deserialize)]
pub struct ListSessionsParams {
    /// Filter sessions by agent ID.
    pub nous_id: Option<String>,
    /// Maximum number of sessions to return.
    pub limit: Option<u32>,
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
#[derive(Debug, Serialize, ToSchema)]
pub struct ListSessionsResponse {
    /// Session summaries matching the query.
    pub sessions: Vec<SessionListItem>,
}

/// Session summary for list endpoints.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionListItem {
    /// Session identifier.
    pub id: String,
    /// Nous agent that owns this session.
    pub nous_id: String,
    /// Client-chosen deduplication key.
    pub session_key: String,
    /// Lifecycle status (e.g. `"active"`, `"archived"`).
    pub status: String,
    /// Total messages stored in this session.
    pub message_count: i64,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
    /// Human-readable display name, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
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

impl SessionResponse {
    pub(super) fn from_mneme(s: &aletheia_mneme::types::Session) -> Self {
        Self {
            id: s.id.clone(),
            nous_id: s.nous_id.clone(),
            session_key: s.session_key.clone(),
            status: s.status.as_str().to_owned(),
            model: s.model.clone(),
            name: s.origin.display_name.clone(),
            message_count: s.metrics.message_count,
            token_count_estimate: s.metrics.token_count_estimate,
            created_at: s.created_at.clone(),
            updated_at: s.updated_at.clone(),
        }
    }
}

/// Response for `GET /api/v1/sessions/{id}/history`.
#[derive(Debug, Serialize, ToSchema)]
pub struct HistoryResponse {
    /// Conversation messages in chronological order.
    pub messages: Vec<HistoryMessage>,
}

/// A single message in the conversation history.
#[derive(Debug, Serialize, ToSchema)]
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
