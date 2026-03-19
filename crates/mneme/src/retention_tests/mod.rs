#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;
use crate::migration;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database should open");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enabling foreign keys should succeed");
    migration::run_migrations(&conn).expect("migrations should run on fresh database");
    conn
}

fn insert_session(conn: &Connection, id: &str, nous_id: &str, status: &str, age_days: i64) {
    let ts = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(age_days * 24))
        .expect("test timestamp subtraction should not overflow");
    let ts_str = ts.strftime("%Y-%m-%dT%H:%M:%S.000Z").to_string();

    conn.execute(
        "INSERT INTO sessions (id, nous_id, session_key, status, created_at, updated_at)
         VALUES (?1, ?2, ?1, ?3, ?4, ?4)",
        rusqlite::params![id, nous_id, status, ts_str],
    )
    .expect("inserting test session should succeed");
}

fn insert_message(conn: &Connection, session_id: &str, seq: i64) {
    conn.execute(
        "INSERT INTO messages (session_id, seq, role, content)
         VALUES (?1, ?2, 'user', 'test message')",
        rusqlite::params![session_id, seq],
    )
    .expect("inserting test message should succeed");
}

fn count_sessions(conn: &Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("counting sessions should succeed")
}

fn count_messages(conn: &Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("counting messages should succeed")
}

mod archive;
mod core;
mod policy;
