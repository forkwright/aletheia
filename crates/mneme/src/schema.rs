//! Schema definition constants.
//!
//! The DDL is the v1 baseline. Migration management lives in `migration.rs`.
//! This matches the TS schema exactly for wire-compatible databases.

/// Valid agent note categories. Single source of truth — used by DDL CHECK constraint and import validation.
pub const VALID_CATEGORIES: &[&str] = &["task", "decision", "preference", "correction", "context"];

/// Base DDL — creates all tables for a fresh database (migration v1).
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
";

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use crate::migration;
    use crate::schema::{DDL, VALID_CATEGORIES};

    #[test]
    fn fresh_database_initializes_via_migration() {
        let conn = Connection::open_in_memory().unwrap();
        let result = migration::run_migrations(&conn).unwrap();
        assert_eq!(result.current_version, 2);
    }

    #[test]
    fn idempotent_initialization() {
        let conn = Connection::open_in_memory().unwrap();
        migration::run_migrations(&conn).unwrap();
        migration::run_migrations(&conn).unwrap();

        let version = migration::get_schema_version(&conn);
        assert_eq!(version, 2);
    }

    #[test]
    fn tables_exist_after_init() {
        let conn = Connection::open_in_memory().unwrap();
        migration::run_migrations(&conn).unwrap();

        for table in &[
            "sessions",
            "messages",
            "usage",
            "distillations",
            "agent_notes",
            "blackboard",
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

    #[test]
    fn valid_categories_non_empty() {
        assert!(
            !VALID_CATEGORIES.is_empty(),
            "VALID_CATEGORIES must not be empty"
        );
    }

    #[test]
    fn valid_categories_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for cat in VALID_CATEGORIES {
            assert!(seen.insert(*cat), "duplicate category: {cat}");
        }
    }

    #[test]
    fn valid_categories_lowercase() {
        for cat in VALID_CATEGORIES {
            assert_eq!(
                *cat,
                cat.to_lowercase(),
                "category '{cat}' should be lowercase"
            );
        }
    }

    #[test]
    fn ddl_contains_all_tables() {
        let expected_tables = [
            "sessions",
            "messages",
            "usage",
            "distillations",
            "agent_notes",
        ];
        for table in &expected_tables {
            assert!(
                DDL.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
                "DDL should contain CREATE TABLE for {table}"
            );
        }
    }

    #[test]
    fn valid_categories_matches_ddl_check_constraint() {
        let marker = "CHECK(category IN (";
        let start = DDL
            .find(marker)
            .expect("CHECK constraint for category exists in DDL");
        let inner_start = start + marker.len();
        let inner_end = DDL[inner_start..]
            .find("))")
            .expect("closing parens for CHECK constraint")
            + inner_start;
        let inner = &DDL[inner_start..inner_end];

        let ddl_cats: Vec<&str> = inner.split(", ").map(|s| s.trim_matches('\'')).collect();

        assert_eq!(
            ddl_cats.len(),
            VALID_CATEGORIES.len(),
            "DDL has {} categories but VALID_CATEGORIES has {}: DDL={ddl_cats:?}, const={VALID_CATEGORIES:?}",
            ddl_cats.len(),
            VALID_CATEGORIES.len(),
        );

        for cat in VALID_CATEGORIES {
            assert!(
                ddl_cats.contains(cat),
                "VALID_CATEGORIES has '{cat}' but it is missing from DDL CHECK constraint"
            );
        }
    }
}
