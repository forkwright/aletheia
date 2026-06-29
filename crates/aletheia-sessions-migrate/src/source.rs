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

use std::collections::BTreeMap;

use graphe::types::{
    AgentNote, BlackboardRow, Message, Role, Session, SessionMetrics, SessionOrigin, SessionStatus,
    SessionType, UsageRecord,
};
use rusqlite::types::FromSql;
use rusqlite::{Connection, Row};
use serde::{Deserialize, Serialize};
use snafu::IntoError as _;
use snafu::ResultExt as _;

use crate::error::{LegacyExtraReadSnafu, Result, SqliteSnafu, UnknownLegacyEnumSnafu};

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

#[derive(Debug, Clone)]
pub(crate) struct LegacySidecarEntry {
    pub(crate) key: String,
    pub(crate) value: Vec<u8>,
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

fn unknown_enum(
    table: &'static str,
    row_id: impl Into<String>,
    column: &'static str,
    raw_value: impl Into<String>,
) -> crate::error::Error {
    UnknownLegacyEnumSnafu {
        table: table.to_owned(),
        row_id: row_id.into(),
        column: column.to_owned(),
        raw_value: raw_value.into(),
    }
    .build()
}

fn parse_session_status(row_id: &str, raw: &str) -> Result<SessionStatus> {
    match raw {
        "active" => Ok(SessionStatus::Active),
        "archived" => Ok(SessionStatus::Archived),
        "distilled" => Ok(SessionStatus::Distilled),
        _ => Err(unknown_enum("sessions", row_id, "status", raw)),
    }
}

fn parse_session_type(row_id: &str, raw: Option<&str>) -> Result<SessionType> {
    match raw {
        None | Some("primary") => Ok(SessionType::Primary),
        Some("background") => Ok(SessionType::Background),
        Some("ephemeral") => Ok(SessionType::Ephemeral),
        Some(value) => Err(unknown_enum("sessions", row_id, "session_type", value)),
    }
}

fn parse_message_role(row_id: i64, raw: &str) -> Result<Role> {
    match raw {
        "system" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool_result" => Ok(Role::ToolResult),
        _ => Err(unknown_enum("messages", row_id.to_string(), "role", raw)),
    }
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
    let status = parse_session_status(&session_id, &status_str)?;
    let session_type = parse_session_type(&session_id, type_str.as_deref())?;
    Ok(SessionRow {
        session: Session {
            id: session_id,
            nous_id: session_column(row, "nous_id")?,
            session_key: session_column(row, "session_key")?,
            status,
            model: session_column(row, "model")?,
            session_type,
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
fn map_message(row: &Row<'_>) -> Result<Message> {
    let id: i64 = row.get("id").context(SqliteSnafu {
        context: "mapping messages.id".to_owned(),
    })?;
    let role_str: String = row.get("role").context(SqliteSnafu {
        context: "mapping messages.role".to_owned(),
    })?;
    let distilled: i64 = row.get("is_distilled").context(SqliteSnafu {
        context: "mapping messages.is_distilled".to_owned(),
    })?;
    Ok(Message {
        id,
        session_id: row.get("session_id").context(SqliteSnafu {
            context: "mapping messages.session_id".to_owned(),
        })?,
        seq: row.get("seq").context(SqliteSnafu {
            context: "mapping messages.seq".to_owned(),
        })?,
        role: parse_message_role(id, &role_str)?,
        content: row.get("content").context(SqliteSnafu {
            context: "mapping messages.content".to_owned(),
        })?,
        tool_call_id: row.get("tool_call_id").context(SqliteSnafu {
            context: "mapping messages.tool_call_id".to_owned(),
        })?,
        tool_name: row.get("tool_name").context(SqliteSnafu {
            context: "mapping messages.tool_name".to_owned(),
        })?,
        token_estimate: row.get("token_estimate").context(SqliteSnafu {
            context: "mapping messages.token_estimate".to_owned(),
        })?,
        is_distilled: distilled != 0,
        created_at: row.get("created_at").context(SqliteSnafu {
            context: "mapping messages.created_at".to_owned(),
        })?,
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
    let mut rows = stmt.query([]).context(SqliteSnafu {
        context: "querying messages".to_owned(),
    })?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().context(SqliteSnafu {
        context: "reading messages row".to_owned(),
    })? {
        out.push(map_message(row)?);
    }
    Ok(out)
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

pub(crate) fn read_legacy_sidecars(conn: &Connection) -> Result<Vec<LegacySidecarEntry>> {
    let mut entries = Vec::new();
    append_usage_legacy_sidecars(conn, &mut entries)?;
    append_distillation_legacy_sidecars(conn, &mut entries)?;
    append_blackboard_legacy_sidecars(conn, &mut entries)?;
    Ok(entries)
}

fn append_usage_legacy_sidecars(
    conn: &Connection,
    entries: &mut Vec<LegacySidecarEntry>,
) -> Result<()> {
    let mut stmt = conn
        .prepare("SELECT session_id, turn_seq, id, created_at FROM usage ORDER BY session_id ASC, turn_seq ASC")
        .context(SqliteSnafu {
            context: "preparing usage legacy sidecar select".to_owned(),
        })?;
    let mut rows = stmt.query([]).context(SqliteSnafu {
        context: "querying usage legacy sidecars".to_owned(),
    })?;
    while let Some(row) = rows.next().context(SqliteSnafu {
        context: "reading usage legacy sidecar row".to_owned(),
    })? {
        let session_id: String = row.get("session_id").context(SqliteSnafu {
            context: "mapping usage.session_id legacy sidecar".to_owned(),
        })?;
        let turn_seq: i64 = row.get("turn_seq").context(SqliteSnafu {
            context: format!("mapping usage.turn_seq legacy sidecar for session {session_id}"),
        })?;
        let row_key = format!("usage:{}:{}", session_id, legacy_i64_key(turn_seq));
        let id: i64 = row.get("id").context(SqliteSnafu {
            context: format!("mapping usage.id legacy sidecar for row {row_key}"),
        })?;
        let created_at: String = row.get("created_at").context(SqliteSnafu {
            context: format!("mapping usage.created_at legacy sidecar for row {row_key}"),
        })?;
        push_sidecar(
            entries,
            format!("{row_key}:id"),
            id.to_string().into_bytes(),
        );
        push_sidecar(
            entries,
            format!("{row_key}:created_at"),
            created_at.into_bytes(),
        );
    }
    Ok(())
}

fn append_distillation_legacy_sidecars(
    conn: &Connection,
    entries: &mut Vec<LegacySidecarEntry>,
) -> Result<()> {
    let mut stmt = conn
        .prepare("SELECT session_id, id, facts_extracted FROM distillations ORDER BY session_id ASC, id ASC")
        .context(SqliteSnafu {
            context: "preparing distillation legacy sidecar select".to_owned(),
        })?;
    let mut rows = stmt.query([]).context(SqliteSnafu {
        context: "querying distillation legacy sidecars".to_owned(),
    })?;
    let mut local_ids: BTreeMap<String, u64> = BTreeMap::new();
    while let Some(row) = rows.next().context(SqliteSnafu {
        context: "reading distillation legacy sidecar row".to_owned(),
    })? {
        let session_id: String = row.get("session_id").context(SqliteSnafu {
            context: "mapping distillations.session_id legacy sidecar".to_owned(),
        })?;
        let legacy_id: i64 = row.get("id").context(SqliteSnafu {
            context: format!("mapping distillations.id legacy sidecar for session {session_id}"),
        })?;
        let facts_extracted: Option<i64> = row.get("facts_extracted").context(SqliteSnafu {
            context: format!(
                "mapping distillations.facts_extracted legacy sidecar for id {legacy_id}"
            ),
        })?;
        let local_id = local_ids.entry(session_id.clone()).or_insert(0);
        *local_id += 1;
        let row_key = format!("distillations:{}:{}", session_id, pad_u64(*local_id));
        push_sidecar(
            entries,
            format!("{row_key}:id"),
            legacy_id.to_string().into_bytes(),
        );
        push_sidecar(
            entries,
            format!("{row_key}:facts_extracted"),
            optional_i64_bytes(facts_extracted),
        );
    }
    Ok(())
}

fn append_blackboard_legacy_sidecars(
    conn: &Connection,
    entries: &mut Vec<LegacySidecarEntry>,
) -> Result<()> {
    let mut stmt = conn
        .prepare("SELECT key, id FROM blackboard ORDER BY key ASC")
        .context(SqliteSnafu {
            context: "preparing blackboard legacy sidecar select".to_owned(),
        })?;
    let mut rows = stmt.query([]).context(SqliteSnafu {
        context: "querying blackboard legacy sidecars".to_owned(),
    })?;
    while let Some(row) = rows.next().context(SqliteSnafu {
        context: "reading blackboard legacy sidecar row".to_owned(),
    })? {
        let key: String = row.get("key").context(SqliteSnafu {
            context: "mapping blackboard.key legacy sidecar".to_owned(),
        })?;
        let id: String = row.get("id").context(SqliteSnafu {
            context: format!("mapping blackboard.id legacy sidecar for key {key}"),
        })?;
        push_sidecar(entries, format!("blackboard:{key}:id"), id.into_bytes());
    }
    Ok(())
}

fn push_sidecar(entries: &mut Vec<LegacySidecarEntry>, key: String, value: Vec<u8>) {
    entries.push(LegacySidecarEntry { key, value });
}

fn optional_i64_bytes(value: Option<i64>) -> Vec<u8> {
    value.map_or_else(|| b"null".to_vec(), |v| v.to_string().into_bytes())
}

fn legacy_i64_key(value: i64) -> String {
    u64::try_from(value).map_or_else(|_| value.to_string(), pad_u64)
}

fn pad_u64(value: u64) -> String {
    format!("{value:0>20}")
}
