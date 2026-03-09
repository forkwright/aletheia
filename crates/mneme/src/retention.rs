//! Session retention policies — configurable cleanup of old sessions and orphan messages.

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::{debug, info, instrument};

use crate::error::{self, Result};

/// Configurable retention policy for session data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Max age for closed sessions (days). Default: 90.
    pub session_max_age_days: u32,
    /// Max age for orphaned messages with no session (days). Default: 30.
    pub orphan_message_max_age_days: u32,
    /// Max sessions to retain per nous (0 = unlimited). Default: 0.
    pub max_sessions_per_nous: u32,
    /// Whether to archive sessions to JSON before deletion. Default: true.
    pub archive_before_delete: bool,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            session_max_age_days: 90,
            orphan_message_max_age_days: 30,
            max_sessions_per_nous: 0,
            archive_before_delete: true,
        }
    }
}

/// Outcome of a retention pass.
#[derive(Debug, Default)]
pub struct RetentionResult {
    /// Number of sessions removed during this pass.
    pub sessions_deleted: u32,
    /// Number of orphan messages removed during this pass.
    pub messages_deleted: u32,
    /// Estimated bytes freed (based on `SQLite` freelist page delta).
    pub bytes_freed: u64,
}

/// Archived session data (written to JSON).
#[derive(Debug, Serialize)]
struct SessionArchive {
    session: serde_json::Value,
    messages: Vec<serde_json::Value>,
    notes: Vec<serde_json::Value>,
    archived_at: String,
}

impl RetentionPolicy {
    /// Apply this retention policy to the database.
    ///
    /// `archive_dir` is used when `archive_before_delete` is true. One JSON file
    /// per session is written to `{archive_dir}/{session_id}.json`.
    #[instrument(skip(self, conn))]
    pub fn apply(&self, conn: &Connection, archive_dir: &Path) -> Result<RetentionResult> {
        let page_size = get_page_size(conn);
        let free_before = get_free_pages(conn);

        let mut result = RetentionResult::default();

        // 1. Delete old closed sessions
        let cutoff = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(
                i64::from(self.session_max_age_days) * 24,
            ))
            .expect("retention cutoff overflow");
        let cutoff_str = cutoff.strftime("%Y-%m-%dT%H:%M:%S").to_string();

        let expired_sessions = find_expired_sessions(conn, &cutoff_str)?;
        if !expired_sessions.is_empty() {
            if self.archive_before_delete {
                archive_sessions(conn, &expired_sessions, archive_dir)?;
            }
            let deleted = delete_sessions(conn, &expired_sessions)?;
            result.sessions_deleted += deleted;
        }

        // 2. Enforce per-nous session limit
        if self.max_sessions_per_nous > 0 {
            let excess = find_excess_sessions_per_nous(conn, self.max_sessions_per_nous)?;
            if !excess.is_empty() {
                if self.archive_before_delete {
                    archive_sessions(conn, &excess, archive_dir)?;
                }
                let deleted = delete_sessions(conn, &excess)?;
                result.sessions_deleted += deleted;
            }
        }

        // 3. Delete orphan messages
        let orphan_cutoff = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(
                i64::from(self.orphan_message_max_age_days) * 24,
            ))
            .expect("orphan cutoff overflow");
        let orphan_cutoff_str = orphan_cutoff.strftime("%Y-%m-%dT%H:%M:%S").to_string();
        result.messages_deleted = delete_orphan_messages(conn, &orphan_cutoff_str)?;

        // Estimate freed bytes
        let free_after = get_free_pages(conn);
        let freed_pages = free_after.saturating_sub(free_before);
        result.bytes_freed = u64::from(freed_pages) * u64::from(page_size);

        info!(
            sessions = result.sessions_deleted,
            messages = result.messages_deleted,
            bytes_freed = result.bytes_freed,
            "retention pass complete"
        );

        Ok(result)
    }
}

fn find_expired_sessions(conn: &Connection, cutoff: &str) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare_cached("SELECT id FROM sessions WHERE status != 'active' AND updated_at < ?1")
        .context(error::DatabaseSnafu)?;

    let rows = stmt
        .query_map([cutoff], |row| row.get::<_, String>(0))
        .context(error::DatabaseSnafu)?;

    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.context(error::DatabaseSnafu)?);
    }
    Ok(ids)
}

fn find_excess_sessions_per_nous(conn: &Connection, keep: u32) -> Result<Vec<String>> {
    // For each nous_id, find sessions beyond the keep limit (oldest first)
    let mut stmt = conn
        .prepare_cached("SELECT DISTINCT nous_id FROM sessions")
        .context(error::DatabaseSnafu)?;

    let nous_ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .context(error::DatabaseSnafu)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context(error::DatabaseSnafu)?;

    let mut excess = Vec::new();

    let mut excess_stmt = conn
        .prepare_cached(
            "SELECT id FROM sessions WHERE nous_id = ?1
             ORDER BY updated_at DESC LIMIT -1 OFFSET ?2",
        )
        .context(error::DatabaseSnafu)?;

    for nous_id in &nous_ids {
        let rows = excess_stmt
            .query_map(rusqlite::params![nous_id, keep], |row| {
                row.get::<_, String>(0)
            })
            .context(error::DatabaseSnafu)?;

        for row in rows {
            excess.push(row.context(error::DatabaseSnafu)?);
        }
    }

    Ok(excess)
}

fn archive_sessions(conn: &Connection, session_ids: &[String], archive_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(archive_dir).context(error::IoSnafu {
        path: archive_dir.display().to_string(),
    })?;

    for session_id in session_ids {
        let archive = build_session_archive(conn, session_id)?;
        let path = archive_dir.join(format!("{session_id}.json"));
        let json = serde_json::to_string_pretty(&archive).context(error::StoredJsonSnafu)?;
        std::fs::write(&path, json).context(error::IoSnafu {
            path: path.display().to_string(),
        })?;
        debug!(session_id, path = %path.display(), "archived session");
    }

    Ok(())
}

fn build_session_archive(conn: &Connection, session_id: &str) -> Result<SessionArchive> {
    // Session row as JSON value
    let session: serde_json::Value = conn
        .query_row(
            "SELECT id, nous_id, session_key, parent_session_id, status, model,
                    token_count_estimate, message_count, session_type,
                    created_at, updated_at
             FROM sessions WHERE id = ?1",
            [session_id],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "nous_id": row.get::<_, String>(1)?,
                    "session_key": row.get::<_, String>(2)?,
                    "parent_session_id": row.get::<_, Option<String>>(3)?,
                    "status": row.get::<_, String>(4)?,
                    "model": row.get::<_, Option<String>>(5)?,
                    "token_count_estimate": row.get::<_, i64>(6)?,
                    "message_count": row.get::<_, i64>(7)?,
                    "session_type": row.get::<_, String>(8)?,
                    "created_at": row.get::<_, String>(9)?,
                    "updated_at": row.get::<_, String>(10)?,
                }))
            },
        )
        .context(error::DatabaseSnafu)?;

    // Messages
    let mut msg_stmt = conn
        .prepare_cached(
            "SELECT seq, role, content, tool_call_id, tool_name, token_estimate,
                    is_distilled, created_at
             FROM messages WHERE session_id = ?1 ORDER BY seq ASC",
        )
        .context(error::DatabaseSnafu)?;

    let messages: Vec<serde_json::Value> = msg_stmt
        .query_map([session_id], |row| {
            Ok(serde_json::json!({
                "seq": row.get::<_, i64>(0)?,
                "role": row.get::<_, String>(1)?,
                "content": row.get::<_, String>(2)?,
                "tool_call_id": row.get::<_, Option<String>>(3)?,
                "tool_name": row.get::<_, Option<String>>(4)?,
                "token_estimate": row.get::<_, i64>(5)?,
                "is_distilled": row.get::<_, bool>(6)?,
                "created_at": row.get::<_, String>(7)?,
            }))
        })
        .context(error::DatabaseSnafu)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context(error::DatabaseSnafu)?;

    // Notes
    let mut note_stmt = conn
        .prepare_cached(
            "SELECT nous_id, category, content, created_at
             FROM agent_notes WHERE session_id = ?1 ORDER BY id ASC",
        )
        .context(error::DatabaseSnafu)?;

    let notes: Vec<serde_json::Value> = note_stmt
        .query_map([session_id], |row| {
            Ok(serde_json::json!({
                "nous_id": row.get::<_, String>(0)?,
                "category": row.get::<_, String>(1)?,
                "content": row.get::<_, String>(2)?,
                "created_at": row.get::<_, String>(3)?,
            }))
        })
        .context(error::DatabaseSnafu)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context(error::DatabaseSnafu)?;

    Ok(SessionArchive {
        session,
        messages,
        notes,
        archived_at: jiff::Timestamp::now().to_string(),
    })
}

fn delete_sessions(conn: &Connection, session_ids: &[String]) -> Result<u32> {
    let tx = conn.unchecked_transaction().context(error::DatabaseSnafu)?;

    let mut count = 0u32;
    for session_id in session_ids {
        // Delete related rows first (foreign key constraint)
        tx.execute(
            "DELETE FROM agent_notes WHERE session_id = ?1",
            [session_id],
        )
        .context(error::DatabaseSnafu)?;
        tx.execute(
            "DELETE FROM distillations WHERE session_id = ?1",
            [session_id],
        )
        .context(error::DatabaseSnafu)?;
        tx.execute("DELETE FROM usage WHERE session_id = ?1", [session_id])
            .context(error::DatabaseSnafu)?;
        tx.execute("DELETE FROM messages WHERE session_id = ?1", [session_id])
            .context(error::DatabaseSnafu)?;
        let rows = tx
            .execute("DELETE FROM sessions WHERE id = ?1", [session_id])
            .context(error::DatabaseSnafu)?;
        count += u32::try_from(rows).unwrap_or(0);
    }

    tx.commit().context(error::DatabaseSnafu)?;
    Ok(count)
}

fn delete_orphan_messages(conn: &Connection, cutoff: &str) -> Result<u32> {
    let rows = conn
        .execute(
            "DELETE FROM messages WHERE session_id NOT IN (SELECT id FROM sessions) AND created_at < ?1",
            [cutoff],
        )
        .context(error::DatabaseSnafu)?;
    Ok(u32::try_from(rows).unwrap_or(0))
}

fn get_page_size(conn: &Connection) -> u32 {
    conn.query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap_or(4096)
}

fn get_free_pages(conn: &Connection) -> u32 {
    conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migration::run_migrations(&conn).unwrap();
        conn
    }

    fn insert_session(conn: &Connection, id: &str, nous_id: &str, status: &str, age_days: i64) {
        let ts = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(age_days * 24))
            .unwrap();
        let ts_str = ts.strftime("%Y-%m-%dT%H:%M:%S.000Z").to_string();

        conn.execute(
            "INSERT INTO sessions (id, nous_id, session_key, status, created_at, updated_at)
             VALUES (?1, ?2, ?1, ?3, ?4, ?4)",
            rusqlite::params![id, nous_id, status, ts_str],
        )
        .unwrap();
    }

    fn insert_message(conn: &Connection, session_id: &str, seq: i64) {
        conn.execute(
            "INSERT INTO messages (session_id, seq, role, content)
             VALUES (?1, ?2, 'user', 'test message')",
            rusqlite::params![session_id, seq],
        )
        .unwrap();
    }

    fn count_sessions(conn: &Connection) -> u32 {
        conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap()
    }

    fn count_messages(conn: &Connection) -> u32 {
        conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn retention_deletes_old_sessions_keeps_recent() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "old-1", "syn", "archived", 100);
        insert_message(&conn, "old-1", 1);
        insert_session(&conn, "recent-1", "syn", "archived", 10);
        insert_message(&conn, "recent-1", 1);

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 1);
        assert_eq!(count_sessions(&conn), 1);
    }

    #[test]
    fn retention_skips_active_sessions() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "active-old", "syn", "active", 200);
        insert_session(&conn, "archived-old", "syn", "archived", 200);

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 1);
        // Active session preserved even though it's old
        assert_eq!(count_sessions(&conn), 1);
    }

    #[test]
    fn archive_produces_valid_json() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();
        let archive_dir = dir.path().join("archive");

        insert_session(&conn, "old-1", "syn", "archived", 100);
        insert_message(&conn, "old-1", 1);
        insert_message(&conn, "old-1", 2);

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: true,
            ..RetentionPolicy::default()
        };

        policy.apply(&conn, &archive_dir).unwrap();

        let archive_path = archive_dir.join("old-1.json");
        assert!(archive_path.exists(), "archive file should exist");

        let contents = std::fs::read_to_string(&archive_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["session"]["id"], "old-1");
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn orphan_messages_cleaned_up() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "ses-1", "syn", "active", 0);
        insert_message(&conn, "ses-1", 1);

        // Insert orphan message with old timestamp (session deleted after message insert)
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        let old_ts = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(60 * 24))
            .unwrap();
        let ts_str = old_ts.strftime("%Y-%m-%dT%H:%M:%S.000Z").to_string();
        conn.execute(
            "INSERT INTO messages (session_id, seq, role, content, created_at) VALUES ('gone', 1, 'user', 'orphan', ?1)",
            [&ts_str],
        ).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();

        let policy = RetentionPolicy {
            orphan_message_max_age_days: 30,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.messages_deleted, 1);
        // Non-orphan message still exists
        assert_eq!(count_messages(&conn), 1);
    }

    #[test]
    fn max_sessions_per_nous_limit_works() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        for i in 0..5 {
            insert_session(
                &conn,
                &format!("ses-{i}"),
                "syn",
                "archived",
                i64::from(5 - i), // ses-0 is oldest
            );
        }

        let policy = RetentionPolicy {
            max_sessions_per_nous: 2,
            archive_before_delete: false,
            // Set high age so age-based retention doesn't fire
            session_max_age_days: 365,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 3);
        assert_eq!(count_sessions(&conn), 2);
    }

    #[test]
    fn default_policy_retains_everything() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "ses-1", "syn", "archived", 30);
        insert_session(&conn, "ses-2", "syn", "distilled", 60);
        insert_session(&conn, "ses-3", "syn", "active", 10);

        let policy = RetentionPolicy::default();
        let result = policy.apply(&conn, dir.path()).unwrap();

        // Default is 90 days, all sessions are < 90 days old
        assert_eq!(result.sessions_deleted, 0);
        assert_eq!(count_sessions(&conn), 3);
    }

    #[test]
    fn retention_archives_before_delete() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();
        let archive_dir = dir.path().join("archive");

        insert_session(&conn, "expired-1", "alice", "archived", 100);
        insert_message(&conn, "expired-1", 1);

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: true,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, &archive_dir).unwrap();
        assert_eq!(result.sessions_deleted, 1);

        let archive_path = archive_dir.join("expired-1.json");
        assert!(archive_path.exists(), "archive file must be created");

        let contents = std::fs::read_to_string(&archive_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["session"]["id"], "expired-1");
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 1);
        assert!(parsed["archived_at"].is_string());

        assert_eq!(count_sessions(&conn), 0, "session deleted after archive");
    }

    #[test]
    fn retention_policy_respects_age() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "young", "bob", "archived", 30);
        insert_session(&conn, "old", "bob", "archived", 100);

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 1);

        let remaining: String = conn
            .query_row("SELECT id FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, "young", "30-day session survives 90-day policy");
    }

    #[test]
    fn retention_idempotent() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "old-1", "alice", "archived", 100);
        insert_session(&conn, "old-2", "alice", "archived", 120);
        insert_session(&conn, "recent-1", "alice", "archived", 10);

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let first = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(first.sessions_deleted, 2);
        assert_eq!(count_sessions(&conn), 1);

        let second = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(second.sessions_deleted, 0, "second pass deletes nothing");
        assert_eq!(count_sessions(&conn), 1);
    }

    #[test]
    fn retention_concurrent_access() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        for i in 0..10 {
            insert_session(
                &conn,
                &format!("ses-{i}"),
                "charlie",
                "archived",
                100 + i64::from(i),
            );
        }

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let r1 = policy.apply(&conn, dir.path()).unwrap();

        // Second run on same conn after first completed
        let r2 = policy.apply(&conn, dir.path()).unwrap();

        let total_deleted = r1.sessions_deleted + r2.sessions_deleted;
        assert_eq!(total_deleted, 10, "all expired sessions removed");
        assert_eq!(count_sessions(&conn), 0);
    }

    #[test]
    fn retention_empty_store() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(count_sessions(&conn), 0);

        let policy = RetentionPolicy::default();
        let result = policy.apply(&conn, dir.path()).unwrap();

        assert_eq!(result.sessions_deleted, 0);
        assert_eq!(result.messages_deleted, 0);
    }

    #[test]
    fn retention_preserves_active_facts() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        for i in 0..7 {
            insert_session(
                &conn,
                &format!("active-{i}"),
                "alice",
                "active",
                100 + i64::from(i),
            );
        }
        for i in 0..3 {
            insert_session(
                &conn,
                &format!("expired-{i}"),
                "alice",
                "archived",
                100 + i64::from(i),
            );
        }

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 3, "only archived sessions deleted");
        assert_eq!(count_sessions(&conn), 7, "active sessions untouched");
    }

    #[test]
    fn apply_empty_policy_is_noop() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "s1", "alice", "active", 10);
        insert_session(&conn, "s2", "alice", "archived", 30);
        insert_session(&conn, "s3", "bob", "active", 50);
        insert_message(&conn, "s1", 1);
        insert_message(&conn, "s2", 1);

        let policy = RetentionPolicy::default();
        let result = policy.apply(&conn, dir.path()).unwrap();

        assert_eq!(
            result.sessions_deleted, 0,
            "default policy keeps everything under 90 days"
        );
        assert_eq!(result.messages_deleted, 0);
        assert_eq!(count_sessions(&conn), 3);
        assert_eq!(count_messages(&conn), 2);
    }

    #[test]
    fn apply_preserves_recent_sessions() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "recent-a", "alice", "archived", 5);
        insert_session(&conn, "recent-b", "alice", "archived", 15);
        insert_session(&conn, "recent-c", "bob", "archived", 25);

        let policy = RetentionPolicy {
            session_max_age_days: 30,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 0, "all sessions are recent enough");
        assert_eq!(count_sessions(&conn), 3);
    }

    #[test]
    fn apply_removes_old_sessions() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "keep", "alice", "archived", 10);
        insert_session(&conn, "remove-1", "alice", "archived", 60);
        insert_session(&conn, "remove-2", "bob", "archived", 80);
        insert_message(&conn, "remove-1", 1);
        insert_message(&conn, "remove-2", 1);

        let policy = RetentionPolicy {
            session_max_age_days: 30,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 2);
        assert_eq!(count_sessions(&conn), 1);

        let remaining: String = conn
            .query_row("SELECT id FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, "keep");
    }

    #[test]
    fn apply_twice_is_idempotent() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        for i in 0..8 {
            let age = 10 + i64::from(i) * 15;
            insert_session(&conn, &format!("idem-{i}"), "alice", "archived", age);
            insert_message(&conn, &format!("idem-{i}"), 1);
        }

        let policy = RetentionPolicy {
            session_max_age_days: 60,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let first = policy.apply(&conn, dir.path()).unwrap();
        let count_after_first = count_sessions(&conn);
        let msg_after_first = count_messages(&conn);

        let second = policy.apply(&conn, dir.path()).unwrap();
        let count_after_second = count_sessions(&conn);
        let msg_after_second = count_messages(&conn);

        assert_eq!(
            count_after_first, count_after_second,
            "applying same policy twice yields same session count"
        );
        assert_eq!(
            msg_after_first, msg_after_second,
            "applying same policy twice yields same message count"
        );
        assert_eq!(second.sessions_deleted, 0, "second pass should be a no-op");
        assert!(
            first.sessions_deleted > 0,
            "first pass should delete something"
        );
    }

    #[test]
    fn apply_skips_active_sessions() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "active-ancient", "alice", "active", 500);
        insert_session(&conn, "active-old", "bob", "active", 200);
        insert_session(&conn, "archived-old", "alice", "archived", 200);

        let policy = RetentionPolicy {
            session_max_age_days: 1,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 1, "only archived session removed");
        assert_eq!(count_sessions(&conn), 2, "both active sessions survive");

        let ids: Vec<String> = conn
            .prepare("SELECT id FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(ids.contains(&"active-ancient".to_owned()));
        assert!(ids.contains(&"active-old".to_owned()));
    }

    #[test]
    fn policy_max_sessions_respected() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        for i in 0..10 {
            insert_session(
                &conn,
                &format!("max-{i}"),
                "alice",
                "archived",
                i64::from(i),
            );
        }

        let policy = RetentionPolicy {
            max_sessions_per_nous: 3,
            session_max_age_days: 365,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(result.sessions_deleted, 7);
        assert_eq!(count_sessions(&conn), 3, "only 3 most recent kept");
    }

    #[test]
    fn retention_with_zero_max_age() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        insert_session(&conn, "closed-1", "alice", "archived", 1);
        insert_session(&conn, "closed-2", "alice", "distilled", 2);
        insert_session(&conn, "active-1", "alice", "active", 1);

        let policy = RetentionPolicy {
            session_max_age_days: 0,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert_eq!(
            result.sessions_deleted, 2,
            "max_age=0 should delete all non-active sessions"
        );
        assert_eq!(count_sessions(&conn), 1, "active session survives");

        let remaining: String = conn
            .query_row("SELECT id FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, "active-1");
    }

    #[test]
    fn retention_respects_keep_minimum() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();

        for i in 0..6 {
            insert_session(
                &conn,
                &format!("keep-{i}"),
                "bob",
                "archived",
                i64::from(i) + 1,
            );
        }

        let policy = RetentionPolicy {
            session_max_age_days: 0,
            max_sessions_per_nous: 3,
            archive_before_delete: false,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, dir.path()).unwrap();
        assert!(
            result.sessions_deleted >= 3,
            "at least the excess sessions should be deleted"
        );
        let remaining = count_sessions(&conn);
        assert!(
            remaining <= 3,
            "per-nous limit of 3 should be respected, got {remaining}"
        );
    }

    #[test]
    fn policy_preserves_notes_in_archive() {
        let conn = test_conn();
        let dir = tempfile::tempdir().unwrap();
        let archive_dir = dir.path().join("archive");

        insert_session(&conn, "noted", "alice", "archived", 100);
        insert_message(&conn, "noted", 1);
        conn.execute(
            "INSERT INTO agent_notes (session_id, nous_id, category, content) VALUES ('noted', 'alice', 'context', 'important context')",
            [],
        )
        .unwrap();

        let policy = RetentionPolicy {
            session_max_age_days: 90,
            archive_before_delete: true,
            ..RetentionPolicy::default()
        };

        let result = policy.apply(&conn, &archive_dir).unwrap();
        assert_eq!(result.sessions_deleted, 1);

        let archive_path = archive_dir.join("noted.json");
        assert!(archive_path.exists());
        let contents = std::fs::read_to_string(&archive_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["notes"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["notes"][0]["content"], "important context");
        assert_eq!(parsed["notes"][0]["category"], "context");
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn retention_idempotency(
                fact_count in 1_usize..20,
                policy_days in 1_u32..365,
            ) {
                let conn = test_conn();
                let dir = tempfile::tempdir().unwrap();

                for i in 0..fact_count {
                    let age = i64::try_from(i).expect("test index fits i64") * 10 + 5;
                    insert_session(
                        &conn,
                        &format!("prop-ses-{i}"),
                        "alice",
                        "archived",
                        age,
                    );
                }

                let policy = RetentionPolicy {
                    session_max_age_days: policy_days,
                    archive_before_delete: false,
                    ..RetentionPolicy::default()
                };

                policy.apply(&conn, dir.path()).unwrap();
                let after_first = count_sessions(&conn);

                policy.apply(&conn, dir.path()).unwrap();
                let after_second = count_sessions(&conn);

                prop_assert_eq!(
                    after_first, after_second,
                    "second retention pass must not change session count"
                );
            }
        }
    }
}
