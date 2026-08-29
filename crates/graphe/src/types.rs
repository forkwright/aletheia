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
    /// Principal/owner that started this run (e.g. a Signal sender ID, an
    /// authenticated HTTP principal, or an MCP caller identity), distinct
    /// from `nous_id` (the agent acting, not who asked it to).
    ///
    /// `None` for sessions created before this field existed (additive
    /// field; existing JSON deserializes with `None` and is not broken) and
    /// for entrypoints that have not yet threaded a principal through
    /// (aletheia#4795 tracks wiring each remaining entrypoint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// External task/job identifier this session was created under (e.g. a
    /// Diaporeia MCP task id or a dispatch worker slug), distinct from
    /// `thread_id` (a conversational thread, not a unit of work).
    ///
    /// `None` for sessions created before this field existed or from an
    /// entrypoint that has no task concept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Client-generated idempotency key for the turn that created or
    /// resumed this session, threaded from a caller-supplied token so a
    /// retried request cannot double-create a session.
    ///
    /// `None` for sessions created before this field existed or from an
    /// entrypoint that has no client-turn concept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_turn_id: Option<String>,
}

/// A session record persisted in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier (UUID v4).
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: persistence primary key; historical rows carry ULID (#3101) and legacy ses_<24hex> ids that koina::id::SessionId normalizes on Display — a typed field would rewrite the key on reserialize and orphan child rows; new ids are validated at the creation entrypoints
    /// Owning agent identifier.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: persisted record whose value originates from a config-validated NousId at creation; the read path must round-trip rows written before #4638 validation existed
    /// Logical key used to look up or resume this session.
    pub session_key: String, // kanon:ignore RUST/plain-string-secret - NOTE: lookup slug, not a secret credential
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
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: raw foreign key to Session.id; must preserve the stored byte form exactly (UUID, ULID, or legacy ses_) or joins break, so it cannot be a normalizing newtype
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
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: raw foreign key to Session.id; must preserve the stored byte form exactly (UUID, ULID, or legacy ses_) or joins break, so it cannot be a normalizing newtype
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
    /// ISO 8601 timestamp when this usage was recorded (turn completion
    /// time, not session creation time).
    ///
    /// WHY(#5271): usage previously carried no timestamp of its own, so
    /// insight metrics bucketed all of a session's usage under the
    /// session's *creation* date — a long-running session's usage was
    /// misattributed to the day it started, not the day it happened.
    /// `#[serde(default)]` keeps records written before this field
    /// existed deserializable; callers fall back to the owning session's
    /// `created_at` when this is empty.
    #[serde(default)]
    pub created_at: String,
}

/// Structured audit record for one tool invocation within a finalized turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuditRecord {
    /// Store-assigned chronological identifier.
    pub id: i64,
    /// Session this tool call belongs to.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: raw foreign key to Session.id; must preserve the stored byte form exactly (UUID, ULID, or legacy ses_) or joins break, so it cannot be a normalizing newtype
    /// Agent that requested the tool call.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: persisted record whose value originates from a config-validated NousId at creation; the read path must round-trip rows written before #4638 validation existed
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
    /// Stable outcome label (#4558): `"success"`, `"partial_success"`,
    /// `"error"`, or a denial-class string (`"denied_by_group"`,
    /// `"denied_by_hook"`, `"not_found"`, ...) when the call never ran.
    /// Derived from `nous::pipeline::ToolCall::outcome_label()` at
    /// finalize time; open-ended, not a fixed enum, so new classes need no
    /// schema migration here.
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

/// Lifecycle status of a durable [`TurnRecord`] (aletheia#5267).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnRecordStatus {
    /// Turn was accepted and is running; no terminal outcome yet.
    Pending,
    /// Turn completed with a normal assistant response.
    Completed,
    /// Turn completed via a degraded-mode synthetic response.
    Degraded,
    /// Turn failed before producing a durable response.
    Failed,
    /// Turn was cancelled by operator or system.
    Cancelled,
    /// Turn timed out.
    Timeout,
    /// A tool call in this turn was denied by the approval gate.
    ApprovalDenied,
    /// Reconstructed from rows written before this type existed
    /// ([`crate::store::SessionStore::turn_record_or_legacy`]); the true
    /// terminal outcome was never durably recorded and cannot be recovered.
    Unknown,
}

impl TurnRecordStatus {
    /// Return the wire-format string for this status.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::ApprovalDenied => "approval_denied",
            Self::Unknown => "unknown",
        }
    }
}

/// Authoritative durable record binding one conversational turn's request,
/// response, tool activity, model metadata, and accounting together
/// (aletheia#5267).
///
/// WHY: before this type, joining a turn's messages, usage, and tool-audit
/// rows required correlating three partitions by a ULID-derived `turn_seq`
/// with no row that named the turn itself as first-class — replay, crash
/// recovery, UI diagnostics, and billing all had to reconstruct a turn's
/// boundary rather than read one. This type is additive: existing
/// `Session`/`Message`/`UsageRecord`/`ToolAuditRecord` rows and their key
/// schemas are unchanged, so no store-schema-version bump is needed. Rows
/// written before this type existed have no `TurnRecord` at all;
/// [`crate::store::SessionStore::turn_record_or_legacy`] reconstructs a
/// best-effort, honestly-partial one from the surviving
/// `usage`/`tool_audit` rows rather than requiring a backfill migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    /// Canonical turn identity (ULID rendered as a string), stable across
    /// finalize retries.
    pub turn_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: `koina::id::TurnId` exists but is `TurnId(u64)` -- a numeric within-session ordinal. This field is a ULID rendered as a string, minted by nous's pipeline. They are different types for different things, so adopting the newtype would not validate this value, it would change what the column means. Not a migration cost: there is no correct newtype to adopt.
    /// Session this turn belongs to.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: raw foreign key to Session.id; must preserve the stored byte form exactly (UUID, ULID, or legacy ses_) or joins break, so it cannot be a normalizing newtype
    /// Session-local sequence number, monotonically increasing per session.
    ///
    /// Distinct from `UsageRecord::turn_seq`/`ToolAuditRecord::turn_seq` (a
    /// ULID-derived key retained there for their existing idempotency
    /// contract) — this is a real persistent counter maintained in the
    /// `turns` partition, so ordering a session's turns never depends on
    /// reinterpreting ULID timestamp bits (aletheia#5267).
    pub turn_seq: i64,
    /// Lifecycle status at the moment this record was written.
    pub status: TurnRecordStatus,
    /// ISO 8601 timestamp when the turn was accepted.
    pub started_at: String,
    /// ISO 8601 timestamp when the turn reached a terminal status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// Provider instance that served the turn (e.g. an LLM provider name),
    /// when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Actual model that served the turn — may differ from the session's
    /// configured model on fallback or degraded routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Stop reason reported by the pipeline (`end_turn`,
    /// `max_tool_iterations`, ...), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Token usage for the turn, denormalized alongside the `usage`
    /// partition row so a reader can join a turn's accounting without a
    /// second lookup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageRecord>,
    /// Total cost in USD for the turn, when the provider reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Client- or pipeline-supplied idempotency key for this turn. Callers
    /// with no separate client-generated token pass `turn_id` itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Inclusive `[start, end]` session-local message sequence range
    /// persisted for this turn, or `None` if no messages were persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_seq_range: Option<(i64, i64)>,
    /// Global `tool_audit` row ids persisted for this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_audit_ids: Vec<i64>,
    /// Global `notes` row id for the lifecycle/event record committed with
    /// this turn (e.g. nous's turn-attempt note), when one was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_id: Option<i64>,
    /// `true` when this record was reconstructed by
    /// [`crate::store::SessionStore::turn_record_or_legacy`] from pre-#5267
    /// rows rather than written directly by `finalize_turn`. Reconstructed
    /// rows cannot recover `status` (reported as
    /// [`TurnRecordStatus::Unknown`]), `provider`, `stop_reason`,
    /// `cost_usd`, or `message_seq_range`, since none of those had a
    /// durable home before this type existed.
    #[serde(default)]
    pub reconstructed: bool,
}

/// Visibility classification for a blackboard entry (aletheia#5032).
///
/// WHY: `Shared` is `#[default]` so rows written before this taxonomy
/// existed, and any writer that omits the field, keep today's behavior —
/// visible to every viewer. This type only carries the classification; it
/// is `organon::types::services::BlackboardViewer` (and the filtering built
/// on it) that turns it into an enforced policy — this crate stores the
/// value but never filters by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlackboardVisibility {
    /// Visible to any viewer.
    #[default]
    Shared,
    /// Visible only to a viewer whose `nous_id` matches `author_nous_id`.
    NousPrivate,
    /// Visible only to a viewer whose `nous_id` AND `session_id` both match.
    SessionPrivate,
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
    pub author_nous_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: persisted row whose value originates from a config-validated NousId at the write boundary; the read path must round-trip rows written before #4638 validation existed
    pub ttl_seconds: i64,
    pub created_at: String,
    pub expires_at: Option<String>,
    // WHY(#5032): additive fields on an already-persisted row type — old
    // rows without them deserialize via `#[serde(default)]` as Shared/no
    // session, matching pre-taxonomy behavior.
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub visibility: BlackboardVisibility,
}

/// Agent note: explicit agent-written context that survives distillation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNote {
    /// Database-assigned row identifier.
    pub id: i64,
    /// Session this note is attached to.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: raw foreign key to Session.id; must preserve the stored byte form exactly (UUID, ULID, or legacy ses_) or joins break, so it cannot be a normalizing newtype
    /// Agent that wrote the note.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: persisted record whose value originates from a config-validated NousId at creation; the read path must round-trip rows written before #4638 validation existed
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
                owner: None,
                task_id: None,
                client_turn_id: None,
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
                owner: None,
                task_id: None,
                client_turn_id: None,
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
    fn turn_record_status_all_variants_have_stable_wire_strings() {
        let all = [
            TurnRecordStatus::Pending,
            TurnRecordStatus::Completed,
            TurnRecordStatus::Degraded,
            TurnRecordStatus::Failed,
            TurnRecordStatus::Cancelled,
            TurnRecordStatus::Timeout,
            TurnRecordStatus::ApprovalDenied,
            TurnRecordStatus::Unknown,
        ];
        let strs: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
        for s in &strs {
            assert!(!s.is_empty());
        }
        let mut sorted = strs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), strs.len(), "wire strings must be unique");
    }

    #[test]
    fn turn_record_serde_roundtrip_minimal() {
        let record = TurnRecord {
            turn_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            session_id: "ses-1".to_owned(),
            turn_seq: 1,
            status: TurnRecordStatus::Completed,
            started_at: "2026-08-27T00:00:00.000Z".to_owned(),
            completed_at: Some("2026-08-27T00:00:01.000Z".to_owned()),
            provider: Some("anthropic".to_owned()),
            model: Some("claude-sonnet-5".to_owned()),
            stop_reason: Some("end_turn".to_owned()),
            usage: None,
            cost_usd: Some(0.01),
            idempotency_key: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned()),
            message_seq_range: Some((1, 2)),
            tool_audit_ids: vec![1, 2],
            note_id: Some(3),
            reconstructed: false,
        };
        let json = serde_json::to_string(&record).expect("serialize");
        let back: TurnRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.turn_id, record.turn_id);
        assert_eq!(back.turn_seq, record.turn_seq);
        assert_eq!(back.status, record.status);
        assert_eq!(back.message_seq_range, record.message_seq_range);
        assert_eq!(back.tool_audit_ids, record.tool_audit_ids);
    }

    #[test]
    fn turn_record_deserializes_from_pre_5267_shape_without_new_fields() {
        // WHY: a TurnRecord this old cannot exist on disk (the type is new),
        // but the additive-field convention this crate uses everywhere else
        // (client_turn_id, BlackboardVisibility, ...) requires every new
        // field to default cleanly from a JSON object that omits it —
        // this asserts that contract for TurnRecord specifically.
        let minimal = serde_json::json!({
            "turn_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "session_id": "ses-1",
            "turn_seq": 1,
            "status": "completed",
            "started_at": "2026-08-27T00:00:00.000Z",
        });
        let record: TurnRecord =
            serde_json::from_value(minimal).expect("minimal shape must deserialize");
        assert!(record.completed_at.is_none());
        assert!(record.usage.is_none());
        assert!(record.tool_audit_ids.is_empty());
        assert!(!record.reconstructed);
    }
}
