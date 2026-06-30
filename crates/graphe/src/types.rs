//! Core types for the session store.

use eidos::meta::{ArtefactMeta, Stamped};
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
    /// Known lifecycle values in backend wire order.
    pub const ALL: &[Self] = &[Self::Active, Self::Archived, Self::Distilled];

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
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Background => "background",
            Self::Ephemeral => "ephemeral",
        }
    }

    /// Classify session type from key pattern.
    ///
    /// WHY: Session keys encode origin conventions. Internal daemon work is
    /// always background; one-shot tool prefixes are ephemeral; everything
    /// else defaults to primary so user chat and imported/restored sessions
    /// remain first-class.
    #[must_use]
    pub(crate) fn from_key(key: &str) -> Self {
        if key.starts_with("daemon:") || key.contains("prosoche") {
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

/// Role of a message author within a conversation turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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

/// Implement `Display` by delegating to `as_str()`.
macro_rules! display_via_as_str {
    ($($ty:ty),+ $(,)?) => {$(
        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    )+};
}

display_via_as_str!(SessionStatus, SessionType, Role);

/// Reserved prefixes for internal session/agent identifiers.
///
/// User-supplied IDs must not collide with these namespaces; internal callers
/// that legitimately mint such keys must bypass the user guard via the
/// dedicated unchecked constructors.
pub const RESERVED_SESSION_PREFIXES: &[&str] = &["cross:"];

/// Whether `value` starts with any reserved internal prefix.
#[must_use]
pub fn is_reserved_session_prefix(value: &str) -> bool {
    RESERVED_SESSION_PREFIXES
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

/// A session or agent identifier that has been verified to not use a reserved prefix.
///
/// Constructed only via [`parse_session_or_agent_id`]. Callers that need the
/// raw string can call [`ValidatedId::as_str`] or let the value drop.
pub struct ValidatedId<'a>(&'a str);

impl<'a> ValidatedId<'a> {
    /// Return the validated identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &'a str {
        self.0
    }
}

/// Parses `value` as a user-supplied session or agent identifier.
///
/// Returns a [`ValidatedId`] when `value` does not start with any reserved
/// internal prefix, or [`ReservedIdPrefixError`] when it does.
///
/// # Errors
///
/// Returns [`ReservedIdPrefixError`] when `value` starts with a reserved
/// internal prefix such as `cross:`.
pub fn parse_session_or_agent_id(value: &str) -> Result<ValidatedId<'_>, ReservedIdPrefixError> {
    if let Some(prefix) = RESERVED_SESSION_PREFIXES
        .iter()
        .find(|prefix| value.starts_with(**prefix))
    {
        return Err(ReservedIdPrefixSnafu {
            prefix: prefix.to_string(),
            value: value.to_owned(),
        }
        .build());
    }
    Ok(ValidatedId(value))
}

/// Error returned when an identifier uses a reserved internal prefix.
// kanon:ignore RUST/no-debug-derive-on-public-types — error contains only the offending prefix and value; safe to derive
#[derive(Debug, snafu::Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum ReservedIdPrefixError {
    /// Identifier starts with a reserved internal prefix.
    #[snafu(display("identifier uses reserved internal prefix '{prefix}': {value}"))]
    ReservedIdPrefix {
        /// The reserved prefix that was matched.
        prefix: String,
        /// The full identifier that was rejected.
        value: String,
        /// Source location where the error was constructed.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Token and message count metrics for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
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
    /// ISO 8601 timestamp of the last distillation, if any.
    pub last_distilled_at: Option<String>,
    /// Estimated context window token usage.
    pub computed_context_tokens: i64,
}

/// External origin and identity metadata for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOrigin {
    /// Parent session for sub-task lineage tracking.
    pub parent_session_id: Option<String>,
    /// External thread identifier (e.g. Signal group thread).
    pub thread_id: Option<String>,
    /// Transport layer that originated this session.
    pub transport: Option<String>,
    /// Human-readable display name set by the user.
    pub display_name: Option<String>,
}

/// A session record persisted in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier (UUID v4).
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
    /// Owning agent identifier.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
    /// Logical key used to look up or resume this session.
    pub session_key: String,
    /// Current lifecycle status.
    pub status: SessionStatus,
    /// LLM model used for this session's turns.
    pub model: Option<String>,
    /// Classification of the session's lifecycle behavior.
    pub session_type: SessionType,
    /// ISO 8601 timestamp when the session was created.
    pub created_at: String,
    /// ISO 8601 timestamp of the last update.
    pub updated_at: String,
    /// Token and message count metrics.
    #[serde(flatten)]
    pub metrics: SessionMetrics,
    /// External origin and identity metadata.
    #[serde(flatten)]
    pub origin: SessionOrigin,
    /// Provenance stamp written at persistence time.
    ///
    /// `None` for sessions created before the `Stamped` arc (additive field;
    /// existing JSON deserializes with `None` and is not broken).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artefact_meta: Option<ArtefactMeta>,
}

impl Stamped for Session {
    /// Returns provenance metadata for this session at the moment of persistence.
    ///
    /// `row_counts` includes `"messages"` (from `metrics.message_count`) and
    /// `"distillations"` (from `metrics.distillation_count`).
    fn stamp(&self) -> ArtefactMeta {
        let msg_count = u64::try_from(self.metrics.message_count).unwrap_or(0);
        let distillation_count = u64::try_from(self.metrics.distillation_count).unwrap_or(0);
        ArtefactMeta::new(
            concat!("graphe@", env!("CARGO_PKG_VERSION")),
            1,
            &self.updated_at,
        )
        .with_count("messages", msg_count)
        .with_count("distillations", distillation_count)
    }
}

/// A single message within a session's conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Database-assigned row identifier.
    pub id: i64,
    /// Session this message belongs to.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
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
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
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

/// Structured audit record for one tool invocation within a finalized turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuditRecord {
    /// Store-assigned chronological identifier.
    pub id: i64,
    /// Session this tool call belongs to.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
    /// Agent that requested the tool call.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
    /// Turn sequence shared with usage records for the finalized turn.
    pub turn_seq: i64,
    /// Provider/tool-use identifier for this call.
    pub tool_call_id: String,
    /// Registered tool name.
    pub tool_name: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the tool result was an error.
    pub is_error: bool,
    /// Stable outcome label, currently `"success"` or `"error"`.
    pub outcome: String,
    /// Bounded tool result text captured from the execution path.
    pub result: Option<String>,
    /// Approval outcome applied before execution, when known.
    pub approval: Option<String>,
    /// HMAC receipt token emitted for this tool result, when present.
    pub receipt: Option<String>,
    /// ISO 8601 timestamp when this audit row was written.
    pub created_at: String,
}

/// Blackboard entry: shared agent state with TTL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "blackboard row fields are self-documenting by name"
)]
pub struct BlackboardRow {
    pub key: String,
    pub value: String,
    pub author_nous_id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
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
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
    /// Agent that wrote the note.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id — wire-format serde type; newtype would break JSON compatibility and change public API
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
        // WHY: user chat and imported/restored sessions must stay primary.
        assert_eq!(SessionType::from_key("main"), SessionType::Primary);
        assert_eq!(SessionType::from_key("signal-group"), SessionType::Primary);
        assert_eq!(
            SessionType::from_key("imported:2026-01-01"),
            SessionType::Primary
        );
        assert_eq!(
            SessionType::from_key("restored:backup"),
            SessionType::Primary
        );

        // WHY: daemon autonomous work and prosoche attention loops are
        // background, not operator-primary sessions.
        assert_eq!(
            SessionType::from_key("prosoche-wake"),
            SessionType::Background
        );
        assert_eq!(
            SessionType::from_key("daemon:prosoche"),
            SessionType::Background
        );
        assert_eq!(
            SessionType::from_key("daemon:self-prompt"),
            SessionType::Background
        );
        assert_eq!(
            SessionType::from_key("daemon:evolution"),
            SessionType::Background
        );
        assert_eq!(
            SessionType::from_key("daemon:reflection"),
            SessionType::Background
        );
        assert_eq!(
            SessionType::from_key("daemon:probe-audit"),
            SessionType::Background
        );

        // WHY: one-shot tool/dispatch sessions are ephemeral.
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
    }

    #[test]
    fn session_serde_roundtrip() {
        let session = Session {
            id: "ses-123".to_owned(),
            nous_id: "syn".to_owned(),
            session_key: "main".to_owned(),
            status: SessionStatus::Active,
            model: Some("claude-opus-4-20250514".to_owned()),
            session_type: SessionType::Primary,
            created_at: "2026-02-28T00:00:00Z".to_owned(),
            updated_at: "2026-02-28T01:00:00Z".to_owned(),
            metrics: SessionMetrics {
                token_count_estimate: 5000,
                message_count: 12,
                last_input_tokens: 2000,
                bootstrap_hash: Some("abc123".to_owned()),
                distillation_count: 2,
                last_distilled_at: None,
                computed_context_tokens: 3000,
            },
            origin: SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: Some("signal".to_owned()),
                display_name: Some("My Session".to_owned()),
            },
            artefact_meta: None,
        };
        let json = serde_json::to_string(&session).expect("Session is serializable");
        let back: Session = serde_json::from_str(&json).expect("round-trip JSON is valid");
        assert_eq!(session.id, back.id);
        assert_eq!(session.status, back.status);
        assert_eq!(session.session_type, back.session_type);
        assert_eq!(session.origin.display_name, back.origin.display_name);
    }

    fn sample_session() -> Session {
        Session {
            id: "ses-test".to_owned(),
            nous_id: "syn".to_owned(),
            session_key: "main".to_owned(),
            status: SessionStatus::Active,
            model: None,
            session_type: SessionType::Primary,
            created_at: "2026-04-22T00:00:00Z".to_owned(),
            updated_at: "2026-04-22T01:00:00Z".to_owned(),
            metrics: SessionMetrics {
                token_count_estimate: 100,
                message_count: 5,
                last_input_tokens: 50,
                bootstrap_hash: None,
                distillation_count: 1,
                last_distilled_at: None,
                computed_context_tokens: 80,
            },
            origin: SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: None,
                display_name: None,
            },
            artefact_meta: None,
        }
    }

    #[test]
    fn session_stamp_producer_and_schema_version() {
        let session = sample_session();
        let meta = session.stamp();
        assert!(
            meta.producer.starts_with("graphe@"),
            "producer must start with 'graphe@', got: {}",
            meta.producer
        );
        assert_eq!(meta.schema_version, 1, "schema_version must be 1");
    }

    #[test]
    fn session_stamp_row_counts() {
        let session = sample_session();
        let meta = session.stamp();
        assert_eq!(
            meta.row_counts.get("messages").copied(),
            Some(5),
            "messages row_count should match metrics.message_count"
        );
        assert_eq!(
            meta.row_counts.get("distillations").copied(),
            Some(1),
            "distillations row_count should match metrics.distillation_count"
        );
    }

    #[test]
    fn session_artefact_meta_is_additive_on_serde() {
        // Sessions without artefact_meta in JSON (e.g. old records) must
        // deserialize without error and produce artefact_meta == None.
        let json = r#"{
            "id": "ses-old",
            "nous_id": "syn",
            "session_key": "main",
            "status": "active",
            "session_type": "primary",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "token_count_estimate": 0,
            "message_count": 0,
            "last_input_tokens": 0,
            "distillation_count": 0,
            "computed_context_tokens": 0
        }"#;
        let session: Session = serde_json::from_str(json).expect("old sessions must deserialize");
        assert!(
            session.artefact_meta.is_none(),
            "artefact_meta should default to None for old sessions"
        );
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
    fn reserved_prefix_rejects_cross_session_key() {
        let result = parse_session_or_agent_id("cross:alice");
        assert!(
            result.is_err(),
            "cross:-prefixed identifiers must be rejected for user-supplied IDs"
        );
        let Err(err) = result else {
            panic!("cross:-prefixed identifiers must be rejected for user-supplied IDs");
        };
        assert!(err.to_string().contains("cross:"));
    }

    #[test]
    fn reserved_prefix_accepts_ordinary_ids() {
        for id in ["ses-123", "alice", "ask:demiurge", "spawn:coder"] {
            assert!(
                parse_session_or_agent_id(id).is_ok(),
                "'{id}' should not be a reserved prefix"
            );
        }
    }

    #[test]
    fn is_reserved_session_prefix_detects_cross() {
        assert!(is_reserved_session_prefix("cross:foo"));
        assert!(!is_reserved_session_prefix("foo:cross:"));
        assert!(!is_reserved_session_prefix("Cross:foo"));
    }

    #[test]
    fn session_type_from_key_round_trip() {
        assert_eq!(
            SessionType::from_key("prosoche-loop"),
            SessionType::Background
        );
        assert_eq!(
            SessionType::from_key("daemon:prosoche"),
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
        for daemon_key in &[
            "daemon:self-prompt",
            "daemon:evolution",
            "daemon:reflection",
            "daemon:probe-audit",
        ] {
            assert_eq!(
                SessionType::from_key(daemon_key),
                SessionType::Background,
                "key '{daemon_key}' should resolve to Background"
            );
        }
        assert_eq!(
            SessionType::from_key("regular-session"),
            SessionType::Primary
        );
    }
}
