//! Parameter structs for MCP tools.
//!
//! Each struct derives `schemars::JsonSchema` for automatic JSON Schema generation.

use schemars::JsonSchema;
use serde::Deserialize;

// -- Session params --

/// Parameters for creating a session.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionCreateParams {
    /// The nous agent ID to create the session for.
    pub nous_id: String,
    /// Optional session key. If omitted, defaults to "main".
    pub session_key: Option<String>,
}

/// Parameters for listing sessions.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionListParams {
    /// Filter by nous agent ID.
    pub nous_id: Option<String>,
}

/// Parameters for sending a message to a session.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionMessageParams {
    /// The nous agent ID.
    pub nous_id: String,
    /// The session key identifying the conversation.
    pub session_key: String,
    /// The message content to send.
    pub content: String,
}

/// Parameters for retrieving session history.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SessionHistoryParams {
    /// The session ID (the database primary key).
    pub session_id: String,
    /// Maximum number of messages to return.
    pub limit: Option<i64>,
}

// -- Nous params --

/// Parameters for querying a specific nous agent.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NousIdParam {
    /// The nous agent ID.
    pub nous_id: String,
}

// -- Knowledge params --

/// Parameters for knowledge search.
#[derive(Debug, Deserialize, JsonSchema)]
#[expect(
    dead_code,
    reason = "fields used for JSON Schema generation; tool is a Phase 1 stub"
)]
pub(crate) struct KnowledgeSearchParams {
    /// The search query text.
    pub query: String,
    /// Optional nous agent ID to scope the search.
    pub nous_id: Option<String>,
    /// Maximum number of results to return.
    pub limit: Option<u32>,
}
