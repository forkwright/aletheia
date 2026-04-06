#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;
use crate::migration;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap_or_default();
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .unwrap_or_default();
    migration::run_migrations(&conn).unwrap_or_default();
    conn
}

fn insert_session(conn: &Connection, id: &str, nous_id: &str, status: &str, age_days: i64) {
    let ts = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(age_days * 24))
        .unwrap_or_default();
    let ts_str = ts.strftime("%Y-%m-%dT%H:%M:%S.000Z").to_string();

    conn.execute(
        "INSERT INTO sessions (id, nous_id, session_key, status, created_at, updated_at)
         VALUES (?1, ?2, ?1, ?3, ?4, ?4)",
        rusqlite::params![id, nous_id, status, ts_str],
    )
    .unwrap_or_default();
}

fn insert_message(conn: &Connection, session_id: &str, seq: i64) {
    conn.execute(
        "INSERT INTO messages (session_id, seq, role, content)
         VALUES (?1, ?2, 'user', 'test message')",
        rusqlite::params![session_id, seq],
    )
    .unwrap_or_default();
}

fn count_sessions(conn: &Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap_or_default()
}

fn count_messages(conn: &Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .unwrap_or_default()
}

mod archive;
mod core;
mod policy;
