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
    #[expect(
        clippy::expect_used,
        reason = "timestamp arithmetic uses bounded durations well within i64 range"
    )]
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
        path: archive_dir.to_path_buf(),
    })?;

    for session_id in session_ids {
        let archive = build_session_archive(conn, session_id)?;
        let path = archive_dir.join(format!("{session_id}.json"));
        let json = serde_json::to_string_pretty(&archive).context(error::StoredJsonSnafu)?;
        std::fs::write(&path, json).context(error::IoSnafu {
            path: path.clone(),
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
#[path = "retention_tests.rs"]
mod tests;
