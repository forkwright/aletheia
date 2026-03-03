//! Schema definition and migration management.
//!
//! The DDL is the v1 baseline. Migrations are applied incrementally.
//! This matches the TS schema exactly for wire-compatible databases.

use rusqlite::Connection;
use snafu::ResultExt;
use tracing::info;

use crate::error::{self, Result};

/// Current schema version (base DDL).
pub const SCHEMA_VERSION: u32 = 1;

/// Base DDL — creates all tables for a fresh database.
pub const DDL: &str = r"
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  nous_id TEXT NOT NULL,
  session_key TEXT NOT NULL,
  parent_session_id TEXT,
  status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'archived', 'distilled')),
  model TEXT,
  token_count_estimate INTEGER DEFAULT 0,
  message_count INTEGER DEFAULT 0,
  last_input_tokens INTEGER DEFAULT 0,
  bootstrap_hash TEXT,
  distillation_count INTEGER DEFAULT 0,
  session_type TEXT DEFAULT 'primary',
  last_distilled_at TEXT,
  computed_context_tokens INTEGER DEFAULT 0,
  thread_id TEXT,
  transport TEXT,
  working_state TEXT,
  distillation_priming TEXT,
  thinking_enabled INTEGER DEFAULT 0,
  thinking_budget INTEGER DEFAULT 10000,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE(nous_id, session_key)
);

CREATE INDEX IF NOT EXISTS idx_sessions_nous ON sessions(nous_id);
CREATE INDEX IF NOT EXISTS idx_sessions_key ON sessions(nous_id, session_key);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_type ON sessions(session_type);
CREATE INDEX IF NOT EXISTS idx_sessions_thread ON sessions(thread_id);

CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  seq INTEGER NOT NULL,
  role TEXT NOT NULL CHECK(role IN ('system', 'user', 'assistant', 'tool_result')),
  content TEXT NOT NULL,
  tool_call_id TEXT,
  tool_name TEXT,
  token_estimate INTEGER DEFAULT 0,
  is_distilled INTEGER DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE(session_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, seq);

CREATE TABLE IF NOT EXISTS usage (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  turn_seq INTEGER NOT NULL,
  input_tokens INTEGER DEFAULT 0,
  output_tokens INTEGER DEFAULT 0,
  cache_read_tokens INTEGER DEFAULT 0,
  cache_write_tokens INTEGER DEFAULT 0,
  model TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_usage_session ON usage(session_id);

CREATE TABLE IF NOT EXISTS distillations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  messages_before INTEGER NOT NULL,
  messages_after INTEGER NOT NULL,
  tokens_before INTEGER NOT NULL,
  tokens_after INTEGER NOT NULL,
  facts_extracted INTEGER DEFAULT 0,
  model TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS agent_notes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  nous_id TEXT NOT NULL,
  category TEXT NOT NULL DEFAULT 'context' CHECK(category IN ('task', 'decision', 'preference', 'correction', 'context')),
  content TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_notes_session ON agent_notes(session_id);
CREATE INDEX IF NOT EXISTS idx_notes_nous ON agent_notes(nous_id);

CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
";

/// Initialize the database: apply base DDL or run pending migrations.
///
/// Idempotent — safe to call on an already-initialized database.
///
/// # Errors
/// Returns an error if the DDL execution or schema version INSERT fails.
pub fn initialize(conn: &Connection) -> Result<()> {
    let version = get_schema_version(conn);

    if version == 0 {
        info!("Initializing fresh database with schema v{SCHEMA_VERSION}");
        conn.execute_batch(DDL).context(error::DatabaseSnafu)?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION],
        )
        .context(error::DatabaseSnafu)?;
    }

    Ok(())
}

/// Get the current schema version, or 0 if uninitialized.
fn get_schema_version(conn: &Connection) -> u32 {
    conn.query_row(
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_initializes() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();

        let version = get_schema_version(&conn);
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn idempotent_initialization() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();
        initialize(&conn).unwrap();

        let version = get_schema_version(&conn);
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn tables_exist_after_init() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();

        // Verify key tables exist
        for table in &[
            "sessions",
            "messages",
            "usage",
            "distillations",
            "agent_notes",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "table {table} should exist");
        }
    }
}
