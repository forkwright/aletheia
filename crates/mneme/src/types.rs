//! Core types for the session store.

use serde::{Deserialize, Serialize};

/// Lifecycle status of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SessionStatus {
    /// Session is live and accepting new messages.
    Active,
    /// Session has been closed and is retained for history.
    Archived,
    /// Session has been distilled into a summary and may be pruned.
    Distilled,
}

impl SessionStatus {
    /// Return the wire-format string for this status.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Distilled => "distilled",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Session type: classifies session lifecycle behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SessionType {
    /// Long-lived conversational session (the default).
    Primary,
    /// Background task session (e.g. prosoche attention loops).
    Background,
    /// Short-lived session for one-shot tasks (`ask:`, `spawn:`, `dispatch:`).
    Ephemeral,
}

impl SessionType {
    /// Return the wire-format string for this type.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Background => "background",
            Self::Ephemeral => "ephemeral",
        }
    }

    /// Classify session type from key pattern.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        if key.contains("prosoche") {
            Self::Background
        } else if key.starts_with("ask:")
            || key.starts_with("spawn:")
            || key.starts_with("dispatch:")
            || key.starts_with("ephemeral:")
        {
            Self::Ephemeral
        } else {
            Self::Primary
        }
    }
}

impl std::fmt::Display for SessionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Role of a message author within a conversation turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// System-injected context (bootstrap, instructions).
    System,
    /// Human operator input.
    User,
    /// LLM-generated response.
    Assistant,
    /// Output returned from a tool invocation.
    ToolResult,
}

impl Role {
    /// Return the wire-format string for this role.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::ToolResult => "tool_result",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A session record persisted in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier (ULID).
    pub id: String,
    /// Owning agent identifier.
    pub nous_id: String,
    /// Logical key used to look up or resume this session.
    pub session_key: String,
    /// Parent session for sub-task lineage tracking.
    pub parent_session_id: Option<String>,
    /// Current lifecycle status.
    pub status: SessionStatus,
    /// LLM model used for this session's turns.
    pub model: Option<String>,
    /// Approximate total tokens consumed across all messages.
    pub token_count_estimate: i64,
    /// Number of messages in this session.
    pub message_count: i64,
    /// Token count from the most recent input.
    pub last_input_tokens: i64,
    /// Hash of the bootstrap payload to detect config changes.
    pub bootstrap_hash: Option<String>,
    /// Number of times this session has been distilled.
    pub distillation_count: i64,
    /// Classification of the session's lifecycle behavior.
    pub session_type: SessionType,
    /// ISO 8601 timestamp of the last distillation, if any.
    pub last_distilled_at: Option<String>,
    /// Estimated context window token usage.
    pub computed_context_tokens: i64,
    /// External thread identifier (e.g. Signal group thread).
    pub thread_id: Option<String>,
    /// Transport layer that originated this session.
    pub transport: Option<String>,
    /// Human-readable display name set by the user.
    pub display_name: Option<String>,
    /// ISO 8601 timestamp when the session was created.
    pub created_at: String,
    /// ISO 8601 timestamp of the last update.
    pub updated_at: String,
}

/// A single message within a session's conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Database-assigned row identifier.
    pub id: i64,
    /// Session this message belongs to.
    pub session_id: String,
    /// Sequence number within the session (monotonically increasing).
    pub seq: i64,
    /// Author role (system, user, assistant, or `tool_result`).
    pub role: Role,
    /// Message body text.
    pub content: String,
    /// Tool call identifier if this message is a tool result.
    pub tool_call_id: Option<String>,
    /// Tool name if this message is a tool result.
    pub tool_name: Option<String>,
    /// Estimated token count for this message.
    pub token_estimate: i64,
    /// Whether this message was produced by distillation.
    pub is_distilled: bool,
    /// ISO 8601 timestamp when the message was created.
    pub created_at: String,
}

/// Token usage counters for a single conversation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Session this usage belongs to.
    pub session_id: String,
    /// Turn sequence number within the session.
    pub turn_seq: i64,
    /// Tokens consumed from the input (prompt).
    pub input_tokens: i64,
    /// Tokens generated in the output (completion).
    pub output_tokens: i64,
    /// Tokens read from prompt cache.
    pub cache_read_tokens: i64,
    /// Tokens written to prompt cache.
    pub cache_write_tokens: i64,
    /// Model used for this turn, if known.
    pub model: Option<String>,
}

/// Blackboard entry: shared agent state with TTL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardRow {
    pub key: String,
    pub value: String,
    pub author_nous_id: String,
    pub ttl_seconds: i64,
    pub created_at: String,
    pub expires_at: Option<String>,
}

/// Agent note: explicit agent-written context that survives distillation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNote {
    /// Database-assigned row identifier.
    pub id: i64,
    /// Session this note is attached to.
    pub session_id: String,
    /// Agent that wrote the note.
    pub nous_id: String,
    /// Freeform category tag for filtering (e.g. "insight", "task").
    pub category: String,
    /// Note body text.
    pub content: String,
    /// ISO 8601 timestamp when the note was created.
    pub created_at: String,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn session_status_serde_roundtrip() {
        for status in [
            SessionStatus::Active,
            SessionStatus::Archived,
            SessionStatus::Distilled,
        ] {
            let json = serde_json::to_string(&status).expect("SessionStatus is serializable");
            let back: SessionStatus =
                serde_json::from_str(&json).expect("round-trip JSON is valid");
            assert_eq!(status, back);
        }
    }

    #[test]
    fn session_type_serde_roundtrip() {
        for stype in [
            SessionType::Primary,
            SessionType::Background,
            SessionType::Ephemeral,
        ] {
            let json = serde_json::to_string(&stype).expect("SessionType is serializable");
            let back: SessionType = serde_json::from_str(&json).expect("round-trip JSON is valid");
            assert_eq!(stype, back);
        }
    }

    #[test]
    fn role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::ToolResult] {
            let json = serde_json::to_string(&role).expect("Role is serializable");
            let back: Role = serde_json::from_str(&json).expect("round-trip JSON is valid");
            assert_eq!(role, back);
        }
    }

    #[test]
    fn session_type_from_key() {
        assert_eq!(SessionType::from_key("main"), SessionType::Primary);
        assert_eq!(
            SessionType::from_key("prosoche-wake"),
            SessionType::Background
        );
        assert_eq!(
            SessionType::from_key("ask:demiurge"),
            SessionType::Ephemeral
        );
        assert_eq!(SessionType::from_key("spawn:coder"), SessionType::Ephemeral);
        assert_eq!(
            SessionType::from_key("dispatch:task"),
            SessionType::Ephemeral
        );
        assert_eq!(
            SessionType::from_key("ephemeral:one-off"),
            SessionType::Ephemeral
        );
        assert_eq!(SessionType::from_key("signal-group"), SessionType::Primary);
    }

    #[test]
    fn session_serde_roundtrip() {
        let session = Session {
            id: "ses-123".to_owned(),
            nous_id: "syn".to_owned(),
            session_key: "main".to_owned(),
            parent_session_id: None,
            status: SessionStatus::Active,
            model: Some("claude-opus-4-20250514".to_owned()),
            token_count_estimate: 5000,
            message_count: 12,
            last_input_tokens: 2000,
            bootstrap_hash: Some("abc123".to_owned()),
            distillation_count: 2,
            session_type: SessionType::Primary,
            last_distilled_at: None,
            computed_context_tokens: 3000,
            thread_id: None,
            transport: Some("signal".to_owned()),
            display_name: Some("My Session".to_owned()),
            created_at: "2026-02-28T00:00:00Z".to_owned(),
            updated_at: "2026-02-28T01:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&session).expect("Session is serializable");
        let back: Session = serde_json::from_str(&json).expect("round-trip JSON is valid");
        assert_eq!(session.id, back.id);
        assert_eq!(session.status, back.status);
        assert_eq!(session.session_type, back.session_type);
        assert_eq!(session.display_name, back.display_name);
    }

    #[test]
    fn message_serde_roundtrip() {
        let msg = Message {
            id: 1,
            session_id: "ses-123".to_owned(),
            seq: 1,
            role: Role::Assistant,
            content: "hello world".to_owned(),
            tool_call_id: None,
            tool_name: None,
            token_estimate: 50,
            is_distilled: false,
            created_at: "2026-02-28T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&msg).expect("Message is serializable");
        let back: Message = serde_json::from_str(&json).expect("round-trip JSON is valid");
        assert_eq!(msg.role, back.role);
        assert_eq!(msg.content, back.content);
    }

    #[test]
    fn session_status_all_variants() {
        let all = [
            SessionStatus::Active,
            SessionStatus::Archived,
            SessionStatus::Distilled,
        ];
        for status in all {
            let s = status.as_str();
            assert!(!s.is_empty(), "as_str() must be non-empty for {status:?}");
        }
    }

    #[test]
    fn session_type_all_variants() {
        let all = [
            SessionType::Primary,
            SessionType::Background,
            SessionType::Ephemeral,
        ];
        for stype in all {
            let s = stype.as_str();
            assert!(!s.is_empty(), "as_str() must be non-empty for {stype:?}");
        }
    }

    #[test]
    fn role_all_variants() {
        let all = [Role::System, Role::User, Role::Assistant, Role::ToolResult];
        for role in all {
            let s = role.as_str();
            assert!(!s.is_empty(), "as_str() must be non-empty for {role:?}");
        }
    }

    #[test]
    fn session_type_from_key_round_trip() {
        assert_eq!(
            SessionType::from_key("prosoche-loop"),
            SessionType::Background
        );
        assert_eq!(
            SessionType::from_key(SessionType::Background.as_str()),
            SessionType::Primary,
            "plain 'background' string has no prefix match, defaults to Primary"
        );
        for prefix in &["ask:", "spawn:", "dispatch:", "ephemeral:"] {
            let key = format!("{prefix}test");
            assert_eq!(
                SessionType::from_key(&key),
                SessionType::Ephemeral,
                "key '{key}' should resolve to Ephemeral"
            );
        }
        assert_eq!(
            SessionType::from_key("regular-session"),
            SessionType::Primary
        );
    }
}
