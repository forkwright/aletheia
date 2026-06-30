//! `SQLite` schema validation.
//!
//! The legacy aletheia sessions DB was migrated through 32 numbered DDL
//! revisions. We require `user_version = 32` (the final revision before
//! PR #3446 deleted the `SQLite` backend) and verify a SHA-256 checksum over
//! the required table/column/type contract before reading rows.

use std::collections::BTreeMap;

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use snafu::ResultExt as _;

use crate::error::{
    Result, SchemaChecksumSnafu, SchemaMissingColumnSnafu, SchemaMissingTableSnafu,
    SchemaUserVersionSnafu, SqliteSnafu,
};

/// Schema version we know how to migrate.
///
/// Anchored on the final `SQLite` revision shipped before PR #3446 removed
/// the `SQLite` backend. The operator's real DB shows `PRAGMA user_version
/// = 32`.
pub(crate) const REQUIRED_USER_VERSION: i64 = 32;

/// SHA-256 of the typed v32 schema contract that this migrator accepts.
const REQUIRED_SCHEMA_SHA256: &str =
    "769b56457a9c225b1af4626e6e7df34517a678af4b8a1cc2b1899dddbe59859e";

#[derive(Debug, Clone, Copy)]
struct RequiredColumn {
    name: &'static str,
    sqlite_type: &'static str,
}

/// Tables we expect to read from. Other tables (`planning_*`, `audit_log`,
/// `thread_summaries`, etc.) exist in v32 but are not part of the
/// `SessionStore` surface — the new fjall layout has no analog for them
/// and they were never accessed via the public `SessionStore` API.
const REQUIRED_TABLES: &[&str] = &[
    "sessions",
    "messages",
    "usage",
    "distillations",
    "agent_notes",
    "blackboard",
];

macro_rules! columns {
    ($(($name:literal, $sqlite_type:literal)),+ $(,)?) => {
        &[$(RequiredColumn { name: $name, sqlite_type: $sqlite_type }),+]
    };
}

/// Required columns per table. Used to detect schema drift before
/// reading. Lists are minimal; extra columns are tolerated.
#[must_use]
fn required_columns(table: &str) -> &'static [RequiredColumn] {
    match table {
        "sessions" => columns![
            ("id", "TEXT"),
            ("nous_id", "TEXT"),
            ("session_key", "TEXT"),
            ("parent_session_id", "TEXT"),
            ("status", "TEXT"),
            ("model", "TEXT"),
            ("token_count_estimate", "INTEGER"),
            ("message_count", "INTEGER"),
            ("created_at", "TEXT"),
            ("updated_at", "TEXT"),
            ("last_input_tokens", "INTEGER"),
            ("bootstrap_hash", "TEXT"),
            ("distillation_count", "INTEGER"),
            ("thinking_enabled", "INTEGER"),
            ("thinking_budget", "INTEGER"),
            ("thread_id", "TEXT"),
            ("transport", "TEXT"),
            ("working_state", "TEXT"),
            ("session_type", "TEXT"),
            ("last_distilled_at", "TEXT"),
            ("computed_context_tokens", "INTEGER"),
            ("distillation_priming", "TEXT"),
            ("display_name", "TEXT"),
        ],
        "messages" => columns![
            ("id", "INTEGER"),
            ("session_id", "TEXT"),
            ("seq", "INTEGER"),
            ("role", "TEXT"),
            ("content", "TEXT"),
            ("tool_call_id", "TEXT"),
            ("tool_name", "TEXT"),
            ("token_estimate", "INTEGER"),
            ("is_distilled", "INTEGER"),
            ("created_at", "TEXT"),
        ],
        "usage" => columns![
            ("id", "INTEGER"),
            ("session_id", "TEXT"),
            ("turn_seq", "INTEGER"),
            ("input_tokens", "INTEGER"),
            ("output_tokens", "INTEGER"),
            ("cache_read_tokens", "INTEGER"),
            ("cache_write_tokens", "INTEGER"),
            ("model", "TEXT"),
            ("created_at", "TEXT"),
        ],
        "distillations" => columns![
            ("id", "INTEGER"),
            ("session_id", "TEXT"),
            ("messages_before", "INTEGER"),
            ("messages_after", "INTEGER"),
            ("tokens_before", "INTEGER"),
            ("tokens_after", "INTEGER"),
            ("facts_extracted", "INTEGER"),
            ("model", "TEXT"),
            ("created_at", "TEXT"),
        ],
        "agent_notes" => columns![
            ("id", "INTEGER"),
            ("session_id", "TEXT"),
            ("nous_id", "TEXT"),
            ("category", "TEXT"),
            ("content", "TEXT"),
            ("created_at", "TEXT"),
        ],
        "blackboard" => columns![
            ("id", "TEXT"),
            ("key", "TEXT"),
            ("value", "TEXT"),
            ("author_nous_id", "TEXT"),
            ("ttl_seconds", "INTEGER"),
            ("created_at", "TEXT"),
            ("expires_at", "TEXT"),
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
/// drifts from what the migrator can read, or
/// [`crate::error::Error::SchemaChecksum`] when the required table/column/type
/// checksum differs from the supported v32 contract.
pub(crate) fn validate(conn: &Connection) -> Result<()> {
    debug_assert_eq!(required_schema_checksum(), REQUIRED_SCHEMA_SHA256);

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

        let cols = table_columns(conn, table)?;
        for required in required_columns(table) {
            if !cols.contains_key(required.name) {
                return Err(SchemaMissingColumnSnafu {
                    table: (*table).to_owned(),
                    column: required.name.to_owned(),
                    found: cols.keys().cloned().collect::<Vec<_>>(),
                }
                .build());
            }
        }
    }

    let checksum = schema_checksum(conn)?;
    if checksum != REQUIRED_SCHEMA_SHA256 {
        return Err(SchemaChecksumSnafu {
            expected: REQUIRED_SCHEMA_SHA256.to_owned(),
            found: checksum,
        }
        .build());
    }

    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info('{table}')"))
        .context(SqliteSnafu {
            context: format!("preparing table_info for '{table}'"),
        })?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                normalise_sqlite_type(&r.get::<_, String>(2)?),
            ))
        })
        .context(SqliteSnafu {
            context: "running table_info".to_owned(),
        })?;
    let cols = rows
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .context(SqliteSnafu {
            context: format!("mapping table_info for '{table}'"),
        })?;
    Ok(cols)
}

fn schema_checksum(conn: &Connection) -> Result<String> {
    let mut fingerprint = String::new();
    for table in REQUIRED_TABLES {
        let cols = table_columns(conn, table)?;
        fingerprint.push_str(table);
        fingerprint.push('\n');
        for required in required_columns(table) {
            if let Some(sqlite_type) = cols.get(required.name) {
                fingerprint.push_str(required.name);
                fingerprint.push(':');
                fingerprint.push_str(sqlite_type);
                fingerprint.push('\n');
            }
        }
    }
    Ok(sha256_hex(fingerprint.as_bytes()))
}

fn required_schema_checksum() -> String {
    sha256_hex(required_schema_fingerprint().as_bytes())
}

fn required_schema_fingerprint() -> String {
    let mut fingerprint = String::new();
    for table in REQUIRED_TABLES {
        fingerprint.push_str(table);
        fingerprint.push('\n');
        for required in required_columns(table) {
            fingerprint.push_str(required.name);
            fingerprint.push(':');
            fingerprint.push_str(required.sqlite_type);
            fingerprint.push('\n');
        }
    }
    fingerprint
}

fn normalise_sqlite_type(raw: &str) -> String {
    raw.trim().to_ascii_uppercase()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        push_hex_nibble(&mut out, byte >> 4);
        push_hex_nibble(&mut out, byte & 0x0f);
    }
    out
}

fn push_hex_nibble(out: &mut String, nibble: u8) {
    let digit = match nibble {
        0..=9 => b'0' + nibble,
        10..=15 => b'a' + (nibble - 10),
        _ => return,
    };
    out.push(char::from(digit));
}

#[cfg(test)]
#[expect(clippy::expect_used, clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn open_in_memory() -> Connection {
        Connection::open_in_memory().expect("in-memory `SQLite` opens")
    }

    #[test]
    fn required_schema_checksum_matches_expected_constant() {
        let actual = required_schema_checksum();
        assert_eq!(
            actual, REQUIRED_SCHEMA_SHA256,
            "update REQUIRED_SCHEMA_SHA256 to {actual}"
        );
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
