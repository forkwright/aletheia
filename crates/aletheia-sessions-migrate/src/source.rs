//! Read-only `SQLite` source readers.
//!
//! Each function returns a `Vec` because the migrator runs
//! one-shot and the entire operator DB fits in memory (worst case observed:
//! ~50 MB `SQLite`, ~30k message rows). If that ever changes, swap to
//! cursor-streamed readers — keys never depend on cross-row ordering at the
//! source level.
//!
//! Source `schema_version` is asserted by `crate::schema::validate` before
//! these readers run; [`crate::verify`] rebuilds expected fjall key/value
//! entries from these rows and compares them against the destination.

use graphe::types::{
    AgentNote, BlackboardRow, Message, Role, Session, SessionMetrics, SessionOrigin, SessionStatus,
    SessionType, UsageRecord,
};
use rusqlite::types::FromSql;
use rusqlite::{Connection, Row};
use serde::{Deserialize, Serialize};
use snafu::IntoError as _;
use snafu::ResultExt as _;

use crate::error::{LegacyExtraReadSnafu, Result, SqliteSnafu};

/// Legacy session columns that have no analog on the new fjall `Session`
/// type. When non-default, we route them to a `migration_legacy`
/// fjall partition so the data is preserved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyExtras {
    /// `thinking_enabled` flag (0/1). Default: 0.
    pub thinking_enabled: Option<i64>,
    /// `thinking_budget` token cap. Default: 10000.
    pub thinking_budget: Option<i64>,
    /// `working_state` opaque blob (TEXT JSON). Default: NULL.
    pub working_state: Option<String>,
    /// `distillation_priming` opaque blob (TEXT). Default: NULL.
    pub distillation_priming: Option<String>,
}

impl LegacyExtras {
    /// Has any non-default value?
    #[must_use]
    pub fn is_non_default(&self) -> bool {
        // Defaults: thinking_enabled = 0 (or NULL), thinking_budget = 10000
        // (or NULL), working_state = NULL, distillation_priming = NULL.
        let thinking_flag_non_default = matches!(self.thinking_enabled, Some(v) if v != 0);
        let budget_non_default = matches!(self.thinking_budget, Some(v) if v != 10000);
        thinking_flag_non_default
            || budget_non_default
            || self.working_state.is_some()
            || self.distillation_priming.is_some()
    }
}

/// One row from the `sessions` table, fully mapped to the new `Session`
/// type plus any legacy extras the migrator must preserve out-of-band.
#[derive(Clone)]
pub struct SessionRow {
    /// Session record in the new fjall shape.
    pub session: Session,
    /// Legacy columns that don't map to the new shape.
    pub legacy: LegacyExtras,
}

fn session_column<T>(row: &Row<'_>, column: &'static str) -> Result<T>
where
    T: FromSql,
{
    row.get(column).context(SqliteSnafu {
        context: format!("mapping sessions.{column}"),
    })
}

fn legacy_column<T>(row: &Row<'_>, session_id: &str, column: &'static str) -> Result<Option<T>>
where
    T: FromSql,
{
    row.get::<_, Option<T>>(column).map_err(|source| {
        LegacyExtraReadSnafu {
            session_id: session_id.to_owned(),
            column: column.to_owned(),
        }
        .into_error(Box::new(source))
    })
}

/// Map one `SQLite` `sessions` row to a `Session` plus legacy extras.
fn map_session(row: &Row<'_>) -> Result<SessionRow> {
    let session_id: String = session_column(row, "id")?;
    let status_str: String = session_column(row, "status")?;
    let type_str: Option<String> = session_column(row, "session_type")?;
    let thinking_enabled = legacy_column(row, &session_id, "thinking_enabled")?;
    let thinking_budget = legacy_column(row, &session_id, "thinking_budget")?;
    let working_state = legacy_column(row, &session_id, "working_state")?;
    let distillation_priming = legacy_column(row, &session_id, "distillation_priming")?;
    Ok(SessionRow {
        session: Session {
            id: session_id,
            nous_id: session_column(row, "nous_id")?,
            session_key: session_column(row, "session_key")?,
            status: match status_str.as_str() {
                "archived" => SessionStatus::Archived,
                "distilled" => SessionStatus::Distilled,
                _ => SessionStatus::Active,
            },
            model: session_column(row, "model")?,
            session_type: match type_str.as_deref() {
                Some("background") => SessionType::Background,
                Some("ephemeral") => SessionType::Ephemeral,
                _ => SessionType::Primary,
            },
            created_at: session_column(row, "created_at")?,
            updated_at: session_column(row, "updated_at")?,
            metrics: SessionMetrics {
                token_count_estimate: session_column(row, "token_count_estimate")?,
                message_count: session_column(row, "message_count")?,
                last_input_tokens: session_column(row, "last_input_tokens")?,
                bootstrap_hash: session_column(row, "bootstrap_hash")?,
                distillation_count: session_column(row, "distillation_count")?,
                last_distilled_at: session_column(row, "last_distilled_at")?,
                computed_context_tokens: session_column(row, "computed_context_tokens")?,
            },
            origin: SessionOrigin {
                parent_session_id: session_column(row, "parent_session_id")?,
                thread_id: session_column(row, "thread_id")?,
                transport: session_column(row, "transport")?,
                display_name: session_column(row, "display_name")?,
            },
            artefact_meta: None,
        },
        legacy: LegacyExtras {
            thinking_enabled,
            thinking_budget,
            working_state,
            distillation_priming,
        },
    })
}

/// Read every session.
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] if the SELECT cannot be
/// prepared, executed, or any row fails to map.
pub fn read_sessions(conn: &Connection) -> Result<Vec<SessionRow>> {
    let mut stmt = conn
        .prepare("SELECT * FROM sessions ORDER BY created_at ASC")
        .context(SqliteSnafu {
            context: "preparing sessions select".to_owned(),
        })?;
    let mut rows = stmt.query([]).context(SqliteSnafu {
        context: "querying sessions".to_owned(),
    })?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().context(SqliteSnafu {
        context: "reading sessions row".to_owned(),
    })? {
        out.push(map_session(row)?);
    }
    Ok(out)
}

/// Map one `SQLite` `messages` row to a `Message`.
fn map_message(row: &Row<'_>) -> rusqlite::Result<Message> {
    let role_str: String = row.get("role")?;
    let distilled: i64 = row.get("is_distilled")?;
    Ok(Message {
        id: row.get("id")?,
        session_id: row.get("session_id")?,
        seq: row.get("seq")?,
        role: match role_str.as_str() {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool_result" => Role::ToolResult,
            _ => Role::System,
        },
        content: row.get("content")?,
        tool_call_id: row.get("tool_call_id")?,
        tool_name: row.get("tool_name")?,
        token_estimate: row.get("token_estimate")?,
        is_distilled: distilled != 0,
        created_at: row.get("created_at")?,
    })
}

/// Read every message, ordered by (`session_id`, `seq`).
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
pub fn read_messages(conn: &Connection) -> Result<Vec<Message>> {
    let mut stmt = conn
        .prepare("SELECT * FROM messages ORDER BY session_id ASC, seq ASC")
        .context(SqliteSnafu {
            context: "preparing messages select".to_owned(),
        })?;
    let rows: Vec<Message> = stmt
        .query_map([], map_message)
        .context(SqliteSnafu {
            context: "querying messages".to_owned(),
        })?
        .collect::<rusqlite::Result<_>>()
        .context(SqliteSnafu {
            context: "mapping message rows".to_owned(),
        })?;
    Ok(rows)
}

/// Map one `SQLite` `usage` row.
fn map_usage(row: &Row<'_>) -> rusqlite::Result<UsageRecord> {
    Ok(UsageRecord {
        session_id: row.get("session_id")?,
        turn_seq: row.get("turn_seq")?,
        input_tokens: row.get("input_tokens")?,
        output_tokens: row.get("output_tokens")?,
        cache_read_tokens: row.get("cache_read_tokens")?,
        cache_write_tokens: row.get("cache_write_tokens")?,
        model: row.get("model")?,
    })
}

/// Read every usage record.
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
pub fn read_usage(conn: &Connection) -> Result<Vec<UsageRecord>> {
    let mut stmt = conn
        .prepare("SELECT * FROM usage ORDER BY session_id ASC, turn_seq ASC")
        .context(SqliteSnafu {
            context: "preparing usage select".to_owned(),
        })?;
    let rows: Vec<UsageRecord> = stmt
        .query_map([], map_usage)
        .context(SqliteSnafu {
            context: "querying usage".to_owned(),
        })?
        .collect::<rusqlite::Result<_>>()
        .context(SqliteSnafu {
            context: "mapping usage rows".to_owned(),
        })?;
    Ok(rows)
}

/// One distillation row (legacy schema). The fjall layout uses a private
/// `DistillationRecord` shape inside `fjall_store.rs`; we mirror it byte-
/// for-byte so deserialisation by the runtime succeeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistillationRecord {
    /// Owning session.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: mirrors legacy SQLite schema byte-for-byte; newtype would break serde deserialization
    /// Message count before distillation.
    pub messages_before: i64,
    /// Message count after distillation.
    pub messages_after: i64,
    /// Token count before distillation.
    pub tokens_before: i64,
    /// Token count after distillation.
    pub tokens_after: i64,
    /// Model that produced the summary.
    pub model: Option<String>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

fn map_distillation(row: &Row<'_>) -> rusqlite::Result<DistillationRecord> {
    Ok(DistillationRecord {
        session_id: row.get("session_id")?,
        messages_before: row.get("messages_before")?,
        messages_after: row.get("messages_after")?,
        tokens_before: row.get("tokens_before")?,
        tokens_after: row.get("tokens_after")?,
        model: row.get("model")?,
        created_at: row.get("created_at")?,
    })
}

/// Read every distillation record, ordered by `(session_id, id)` so the
/// per-session local sequence we assign matches insertion order.
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
pub fn read_distillations(conn: &Connection) -> Result<Vec<DistillationRecord>> {
    let mut stmt = conn
        .prepare("SELECT * FROM distillations ORDER BY session_id ASC, id ASC")
        .context(SqliteSnafu {
            context: "preparing distillations select".to_owned(),
        })?;
    let rows: Vec<DistillationRecord> = stmt
        .query_map([], map_distillation)
        .context(SqliteSnafu {
            context: "querying distillations".to_owned(),
        })?
        .collect::<rusqlite::Result<_>>()
        .context(SqliteSnafu {
            context: "mapping distillation rows".to_owned(),
        })?;
    Ok(rows)
}

/// One agent note. Reuses the canonical `AgentNote` type.
fn map_note(row: &Row<'_>) -> rusqlite::Result<AgentNote> {
    Ok(AgentNote {
        id: row.get("id")?,
        session_id: row.get("session_id")?,
        nous_id: row.get("nous_id")?,
        category: row.get("category")?,
        content: row.get("content")?,
        created_at: row.get("created_at")?,
    })
}

/// Read every agent note, ordered by `(session_id, id)` so the per-session
/// local sequence we assign matches insertion order.
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
pub fn read_notes(conn: &Connection) -> Result<Vec<AgentNote>> {
    let mut stmt = conn
        .prepare("SELECT * FROM agent_notes ORDER BY session_id ASC, id ASC")
        .context(SqliteSnafu {
            context: "preparing agent_notes select".to_owned(),
        })?;
    let rows: Vec<AgentNote> = stmt
        .query_map([], map_note)
        .context(SqliteSnafu {
            context: "querying agent_notes".to_owned(),
        })?
        .collect::<rusqlite::Result<_>>()
        .context(SqliteSnafu {
            context: "mapping agent_note rows".to_owned(),
        })?;
    Ok(rows)
}

/// Map one blackboard row.
fn map_blackboard(row: &Row<'_>) -> rusqlite::Result<BlackboardRow> {
    Ok(BlackboardRow {
        key: row.get("key")?,
        value: row.get("value")?,
        author_nous_id: row.get("author_nous_id")?,
        ttl_seconds: row.get("ttl_seconds")?,
        created_at: row.get("created_at")?,
        expires_at: row.get("expires_at")?,
    })
}

/// Read every blackboard entry. We keep expired entries here too — the
/// fjall layer will filter them at read time, and operators may want
/// to inspect them post-migration.
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
pub fn read_blackboard(conn: &Connection) -> Result<Vec<BlackboardRow>> {
    let mut stmt = conn
        .prepare("SELECT key, value, author_nous_id, ttl_seconds, created_at, expires_at FROM blackboard ORDER BY key ASC")
        .context(SqliteSnafu {
            context: "preparing blackboard select".to_owned(),
        })?;
    let rows: Vec<BlackboardRow> = stmt
        .query_map([], map_blackboard)
        .context(SqliteSnafu {
            context: "querying blackboard".to_owned(),
        })?
        .collect::<rusqlite::Result<_>>()
        .context(SqliteSnafu {
            context: "mapping blackboard rows".to_owned(),
        })?;
    Ok(rows)
}
