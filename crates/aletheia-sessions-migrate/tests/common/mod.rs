//! Shared test scaffolding: build a `SQLite` v32 fixture from scratch.

#![expect(
    clippy::expect_used,
    reason = "integration test fixtures use direct setup assertions"
)]
// per-test binary uses only a subset of these helpers

use std::path::Path;

use rusqlite::Connection;

/// Minimal v32 schema covering every column the migrator reads.
///
/// This is intentionally hand-written rather than imported from the
/// historical migration runner — the migrator only knows about column
/// presence + types, so the simplest fixture is the one that mirrors
/// the live shape one-to-one.
pub const SCHEMA_SQL: &str = "
PRAGMA user_version = 32;

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    nous_id TEXT NOT NULL,
    session_key TEXT NOT NULL,
    parent_session_id TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    model TEXT,
    token_count_estimate INTEGER DEFAULT 0,
    message_count INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_input_tokens INTEGER DEFAULT 0,
    bootstrap_hash TEXT,
    distillation_count INTEGER DEFAULT 0,
    thinking_enabled INTEGER DEFAULT 0,
    thinking_budget INTEGER DEFAULT 10000,
    thread_id TEXT,
    transport TEXT,
    working_state TEXT,
    session_type TEXT DEFAULT 'primary',
    last_distilled_at TEXT,
    computed_context_tokens INTEGER DEFAULT 0,
    distillation_priming TEXT,
    display_name TEXT,
    UNIQUE(nous_id, session_key)
);

CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tool_call_id TEXT,
    tool_name TEXT,
    token_estimate INTEGER DEFAULT 0,
    is_distilled INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    UNIQUE(session_id, seq)
);

CREATE TABLE usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    turn_seq INTEGER NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    model TEXT,
    created_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE distillations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    messages_before INTEGER NOT NULL,
    messages_after INTEGER NOT NULL,
    tokens_before INTEGER NOT NULL,
    tokens_after INTEGER NOT NULL,
    facts_extracted INTEGER DEFAULT 0,
    model TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE agent_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    nous_id TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'context',
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE blackboard (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    author_nous_id TEXT NOT NULL,
    ttl_seconds INTEGER DEFAULT 3600,
    created_at TEXT NOT NULL,
    expires_at TEXT
);
";

/// Create an empty v32 `SQLite` DB file at `path`.
#[allow(dead_code, reason = "shared fixture helper used by a subset of tests")]
pub fn build_empty_v32(path: &Path) {
    let conn = Connection::open(path).expect("open writable SQLite");
    conn.execute_batch(SCHEMA_SQL).expect("apply v32 DDL");
}

/// Insert one session row.
#[allow(dead_code, reason = "shared fixture helper used by a subset of tests")]
pub fn insert_session(
    conn: &Connection,
    id: &str,
    nous_id: &str,
    session_key: &str,
    status: &str,
    model: Option<&str>,
    created_at: &str,
    updated_at: &str,
) {
    conn.execute(
        "INSERT INTO sessions (id, nous_id, session_key, status, model, created_at, updated_at, session_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'primary')",
        rusqlite::params![id, nous_id, session_key, status, model, created_at, updated_at],
    )
    .expect("insert session");
}

/// Insert one message row.
#[allow(dead_code, reason = "shared fixture helper used by a subset of tests")]
pub fn insert_message(
    conn: &Connection,
    session_id: &str,
    seq: i64,
    role: &str,
    content: &str,
    is_distilled: bool,
    token_estimate: i64,
    created_at: &str,
) {
    conn.execute(
        "INSERT INTO messages (session_id, seq, role, content, is_distilled, token_estimate, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![session_id, seq, role, content, i64::from(is_distilled), token_estimate, created_at],
    )
    .expect("insert message");
}

/// Insert one distillation row.
#[allow(dead_code, reason = "shared fixture helper used by a subset of tests")]
pub fn insert_distillation(
    conn: &Connection,
    session_id: &str,
    messages_before: i64,
    messages_after: i64,
    tokens_before: i64,
    tokens_after: i64,
    model: Option<&str>,
    created_at: &str,
) {
    conn.execute(
        "INSERT INTO distillations (session_id, messages_before, messages_after, tokens_before, tokens_after, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![session_id, messages_before, messages_after, tokens_before, tokens_after, model, created_at],
    )
    .expect("insert distillation");
}

/// Insert one `agent_note` row.
#[allow(dead_code, reason = "shared fixture helper used by a subset of tests")]
pub fn insert_note(
    conn: &Connection,
    session_id: &str,
    nous_id: &str,
    category: &str,
    content: &str,
    created_at: &str,
) {
    conn.execute(
        "INSERT INTO agent_notes (session_id, nous_id, category, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![session_id, nous_id, category, content, created_at],
    )
    .expect("insert agent_note");
}

/// Insert one usage row.
#[allow(dead_code, reason = "shared fixture helper used by a subset of tests")]
pub fn insert_usage(
    conn: &Connection,
    session_id: &str,
    turn_seq: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read: i64,
    cache_write: i64,
    model: Option<&str>,
) {
    conn.execute(
        "INSERT INTO usage (session_id, turn_seq, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, model)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![session_id, turn_seq, input_tokens, output_tokens, cache_read, cache_write, model],
    )
    .expect("insert usage");
}
