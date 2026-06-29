//! `SQLite` schema validation.
//!
//! The legacy aletheia sessions DB was migrated through 32 numbered DDL
//! revisions. We require `user_version = 32` (the final revision before
//! PR #3446 deleted the `SQLite` backend) so that the column shapes assumed
//! by the migrator match what we actually read.

use rusqlite::Connection;
use snafu::ResultExt as _;

use crate::error::{
    Result, SchemaMissingColumnSnafu, SchemaMissingTableSnafu, SchemaUserVersionSnafu, SqliteSnafu,
};

/// Schema version we know how to migrate.
///
/// Anchored on the final `SQLite` revision shipped before PR #3446 removed
/// the `SQLite` backend. The operator's real DB shows `PRAGMA user_version
/// = 32`.
pub const REQUIRED_USER_VERSION: i64 = 32;

/// Tables we expect to read from. Other tables (`planning_*`, `audit_log`,
/// `thread_summaries`, etc.) exist in v32 but are not part of the
/// `SessionStore` surface — the new fjall layout has no analog for them
/// and they were never accessed via the public `SessionStore` API.
pub const REQUIRED_TABLES: &[&str] = &[
    "sessions",
    "messages",
    "usage",
    "distillations",
    "agent_notes",
    "blackboard",
];

/// Required columns per table. Used to detect schema drift before
/// reading. Lists are minimal — extra columns are tolerated.
#[must_use]
pub fn required_columns(table: &str) -> &'static [&'static str] {
    match table {
        "sessions" => &[
            "id",
            "nous_id",
            "session_key",
            "parent_session_id",
            "status",
            "model",
            "token_count_estimate",
            "message_count",
            "created_at",
            "updated_at",
            "last_input_tokens",
            "bootstrap_hash",
            "distillation_count",
            "thinking_enabled",
            "thinking_budget",
            "thread_id",
            "transport",
            "working_state",
            "session_type",
            "last_distilled_at",
            "computed_context_tokens",
            "distillation_priming",
            "display_name",
        ],
        "messages" => &[
            "id",
            "session_id",
            "seq",
            "role",
            "content",
            "tool_call_id",
            "tool_name",
            "token_estimate",
            "is_distilled",
            "created_at",
        ],
        "usage" => &[
            "id",
            "session_id",
            "turn_seq",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "model",
            "created_at",
        ],
        "distillations" => &[
            "id",
            "session_id",
            "messages_before",
            "messages_after",
            "tokens_before",
            "tokens_after",
            "facts_extracted",
            "model",
            "created_at",
        ],
        "agent_notes" => &[
            "id",
            "session_id",
            "nous_id",
            "category",
            "content",
            "created_at",
        ],
        "blackboard" => &[
            "id",
            "key",
            "value",
            "author_nous_id",
            "ttl_seconds",
            "created_at",
            "expires_at",
        ],
        _ => &[],
    }
}

/// Validate that the connection points at a v32 `SQLite` session DB with
/// every required table and column.
///
/// On mismatch, returns a specific error naming the missing item.
///
/// # Errors
///
/// Returns [`crate::error::Error::SchemaUserVersion`] if `PRAGMA user_version`
/// is not [`REQUIRED_USER_VERSION`], [`crate::error::Error::SchemaMissingTable`]
/// or [`crate::error::Error::SchemaMissingColumn`] when the source schema
/// drifts from what the migrator can read.
pub fn validate(conn: &Connection) -> Result<()> {
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .context(SqliteSnafu {
            context: "reading PRAGMA user_version".to_owned(),
        })?;
    if user_version != REQUIRED_USER_VERSION {
        return Err(SchemaUserVersionSnafu {
            expected: REQUIRED_USER_VERSION,
            found: user_version,
        }
        .build());
    }

    for table in REQUIRED_TABLES {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                rusqlite::params![*table],
                |r| r.get(0),
            )
            .context(SqliteSnafu {
                context: format!("checking for table '{table}'"),
            })?;
        if !exists {
            return Err(SchemaMissingTableSnafu {
                table: (*table).to_owned(),
            }
            .build());
        }

        // Validate columns via PRAGMA table_info.
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info('{table}')"))
            .context(SqliteSnafu {
                context: format!("preparing table_info for '{table}'"),
            })?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .context(SqliteSnafu {
                context: "running table_info".to_owned(),
            })?
            .filter_map(std::result::Result::ok)
            .collect();
        for required in required_columns(table) {
            if !cols.iter().any(|c| c == required) {
                return Err(SchemaMissingColumnSnafu {
                    table: (*table).to_owned(),
                    column: (*required).to_owned(),
                    found: cols.clone(),
                }
                .build());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn open_in_memory() -> Connection {
        Connection::open_in_memory().expect("in-memory `SQLite` opens")
    }

    #[test]
    fn rejects_wrong_user_version() {
        let conn = open_in_memory();
        conn.execute_batch("PRAGMA user_version = 31").unwrap();
        // Even if version mismatches before we check tables, the version
        // check fires first.
        let err = validate(&conn).expect_err("wrong version is rejected");
        let msg = format!("{err}");
        assert!(msg.contains("user_version"), "msg: {msg}");
        assert!(msg.contains("31"), "msg: {msg}");
    }

    #[test]
    fn rejects_missing_table() {
        let conn = open_in_memory();
        conn.execute_batch("PRAGMA user_version = 32").unwrap();
        // Create only `sessions`; everything else is missing.
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT, nous_id TEXT, session_key TEXT, parent_session_id TEXT,
                status TEXT, model TEXT, token_count_estimate INTEGER, message_count INTEGER,
                created_at TEXT, updated_at TEXT, last_input_tokens INTEGER,
                bootstrap_hash TEXT, distillation_count INTEGER, thinking_enabled INTEGER,
                thinking_budget INTEGER, thread_id TEXT, transport TEXT, working_state TEXT,
                session_type TEXT, last_distilled_at TEXT, computed_context_tokens INTEGER,
                distillation_priming TEXT, display_name TEXT
            )",
        )
        .unwrap();
        let err = validate(&conn).expect_err("missing table is rejected");
        let msg = format!("{err}");
        assert!(msg.contains("messages"), "msg: {msg}");
    }

    #[test]
    fn rejects_missing_column() {
        let conn = open_in_memory();
        conn.execute_batch("PRAGMA user_version = 32").unwrap();
        // sessions missing `display_name`.
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT, nous_id TEXT, session_key TEXT, parent_session_id TEXT,
                status TEXT, model TEXT, token_count_estimate INTEGER, message_count INTEGER,
                created_at TEXT, updated_at TEXT, last_input_tokens INTEGER,
                bootstrap_hash TEXT, distillation_count INTEGER, thinking_enabled INTEGER,
                thinking_budget INTEGER, thread_id TEXT, transport TEXT, working_state TEXT,
                session_type TEXT, last_distilled_at TEXT, computed_context_tokens INTEGER,
                distillation_priming TEXT
            );
            CREATE TABLE messages (id INTEGER, session_id TEXT, seq INTEGER, role TEXT,
                content TEXT, tool_call_id TEXT, tool_name TEXT, token_estimate INTEGER,
                is_distilled INTEGER, created_at TEXT);
            CREATE TABLE usage (id INTEGER, session_id TEXT, turn_seq INTEGER,
                input_tokens INTEGER, output_tokens INTEGER, cache_read_tokens INTEGER,
                cache_write_tokens INTEGER, model TEXT, created_at TEXT);
            CREATE TABLE distillations (id INTEGER, session_id TEXT, messages_before INTEGER,
                messages_after INTEGER, tokens_before INTEGER, tokens_after INTEGER,
                facts_extracted INTEGER, model TEXT, created_at TEXT);
            CREATE TABLE agent_notes (id INTEGER, session_id TEXT, nous_id TEXT,
                category TEXT, content TEXT, created_at TEXT);
            CREATE TABLE blackboard (id TEXT, key TEXT, value TEXT, author_nous_id TEXT,
                ttl_seconds INTEGER, created_at TEXT, expires_at TEXT);",
        )
        .unwrap();
        let err = validate(&conn).expect_err("missing column is rejected");
        let msg = format!("{err}");
        assert!(msg.contains("display_name"), "msg: {msg}");
    }
}
