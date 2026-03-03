//! Core types for the session store.

use serde::{Deserialize, Serialize};

/// Session status — lifecycle state of a [`Session`].
///
/// Persisted as a lowercase string (`"active"`, `"archived"`, `"distilled"`).
/// See the `status` column constraint in [`crate::schema::DDL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SessionStatus {
    /// Session is in use and accepting new messages.
    Active,
    /// Session has been closed but is preserved in history.
    Archived,
    /// Session history has been compressed by the distillation engine.
    Distilled,
}

impl SessionStatus {
    /// Returns the lowercase string used in the wire format and SQLite storage.
    ///
    /// Matches `serde(rename_all = "lowercase")`: `"active"`, `"archived"`, or `"distilled"`.
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

/// Session type — classifies session lifecycle behavior.
///
/// Derived from the session key via [`SessionType::from_key`].
/// Persisted as a lowercase string in the `session_type` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SessionType {
    /// Main agent session (default).
    Primary,
    /// Background attention task (key contains `"prosoche"`).
    Background,
    /// Short-lived spawned session (`"ask:"`, `"spawn:"`, `"dispatch:"`, `"ephemeral:"` prefix).
    Ephemeral,
}

impl SessionType {
    /// Returns the lowercase string used in the wire format and SQLite storage.
    ///
    /// Matches `serde(rename_all = "lowercase")`: `"primary"`, `"background"`, or `"ephemeral"`.
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

/// Message role for session history storage.
///
/// Distinct from `hermeneus::types::Role` — this type includes `ToolResult`
/// to support storing tool results as first-class messages in the [`Session`]
/// history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Role {
    /// System prompt or injected context.
    System,
    /// Human/caller input.
    User,
    /// Model response.
    Assistant,
    /// Tool execution result returned to the model.
    ToolResult,
}

impl Role {
    /// Returns the `snake_case` string used in the wire format and SQLite storage.
    ///
    /// Matches `serde(rename_all = "snake_case")`: `"system"`, `"user"`,
    /// `"assistant"`, or `"tool_result"`.
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

/// A session record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub nous_id: String,
    pub session_key: String,
    pub parent_session_id: Option<String>,
    pub status: SessionStatus,
    pub model: Option<String>,
    pub token_count_estimate: i64,
    pub message_count: i64,
    pub last_input_tokens: i64,
    pub bootstrap_hash: Option<String>,
    pub distillation_count: i64,
    pub session_type: SessionType,
    pub last_distilled_at: Option<String>,
    pub computed_context_tokens: i64,
    pub thread_id: Option<String>,
    pub transport: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A message record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub session_id: String,
    pub seq: i64,
    pub role: Role,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub token_estimate: i64,
    pub is_distilled: bool,
    pub created_at: String,
}

/// Usage record for a single turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub session_id: String,
    pub turn_seq: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub model: Option<String>,
}

/// Agent note — explicit agent-written context that survives distillation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNote {
    pub id: i64,
    pub session_id: String,
    pub nous_id: String,
    pub category: String,
    pub content: String,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_status_serde_roundtrip() {
        for status in [
            SessionStatus::Active,
            SessionStatus::Archived,
            SessionStatus::Distilled,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: SessionStatus = serde_json::from_str(&json).unwrap();
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
            let json = serde_json::to_string(&stype).unwrap();
            let back: SessionType = serde_json::from_str(&json).unwrap();
            assert_eq!(stype, back);
        }
    }

    #[test]
    fn role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::ToolResult] {
            let json = serde_json::to_string(&role).unwrap();
            let back: Role = serde_json::from_str(&json).unwrap();
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
            created_at: "2026-02-28T00:00:00Z".to_owned(),
            updated_at: "2026-02-28T01:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&session).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(session.id, back.id);
        assert_eq!(session.status, back.status);
        assert_eq!(session.session_type, back.session_type);
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
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg.role, back.role);
        assert_eq!(msg.content, back.content);
    }
}
