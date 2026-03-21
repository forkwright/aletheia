//! Agent notes and blackboard operations.

use snafu::ResultExt;
use tracing::instrument;

use super::{OptionalExt, SessionStore};
use crate::error::{self, Result};
use crate::types::{AgentNote, BlackboardRow};

impl SessionStore {
    /// Add an agent note.
    #[instrument(skip(self, content))]
    pub fn add_note(
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> Result<i64> {
        self.require_writable()?;
        let id = self
            .conn
            .query_row(
                "INSERT INTO agent_notes (session_id, nous_id, category, content) VALUES (?1, ?2, ?3, ?4) RETURNING id",
                rusqlite::params![session_id, nous_id, category, content],
                |row| row.get(0),
            )
            .context(error::DatabaseSnafu)?;
        Ok(id)
    }

    /// Get notes for a session.
    #[instrument(skip(self))]
    pub fn get_notes(&self, session_id: &str) -> Result<Vec<AgentNote>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, session_id, nous_id, category, content, created_at FROM agent_notes WHERE session_id = ?1 ORDER BY id ASC",
            )
            .context(error::DatabaseSnafu)?;

        let rows = stmt
            .query_map([session_id], |row| {
                Ok(AgentNote {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    nous_id: row.get(2)?,
                    category: row.get(3)?,
                    content: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .context(error::DatabaseSnafu)?;

        let mut notes = Vec::new();
        for row in rows {
            notes.push(row.context(error::DatabaseSnafu)?);
        }
        Ok(notes)
    }

    /// Delete a note by ID.
    #[instrument(skip(self))]
    pub fn delete_note(&self, note_id: i64) -> Result<bool> {
        self.require_writable()?;
        let rows = self
            .conn
            .execute("DELETE FROM agent_notes WHERE id = ?1", [note_id])
            .context(error::DatabaseSnafu)?;
        Ok(rows > 0)
    }

    /// Write or update a blackboard entry. Upserts on key.
    #[instrument(skip(self, value), level = "debug")]
    pub fn blackboard_write(
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        self.require_writable()?;
        let id = ulid::Ulid::new().to_string();
        self.conn
            .execute(
                "INSERT INTO blackboard (id, key, value, author_nous_id, ttl_seconds, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now', '+' || ?5 || ' seconds'))
                 ON CONFLICT(key) DO UPDATE SET
                   value = excluded.value,
                   author_nous_id = excluded.author_nous_id,
                   ttl_seconds = excluded.ttl_seconds,
                   expires_at = excluded.expires_at",
                rusqlite::params![id, key, value, author, ttl_secs],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Read a blackboard entry by key, filtering expired entries.
    #[instrument(skip(self))]
    pub fn blackboard_read(&self, key: &str) -> Result<Option<BlackboardRow>> {
        let result = self
            .conn
            .query_row(
                "SELECT key, value, author_nous_id, ttl_seconds, created_at, expires_at
                 FROM blackboard
                 WHERE key = ?1 AND (expires_at IS NULL OR expires_at > datetime('now'))",
                [key],
                |row| {
                    Ok(BlackboardRow {
                        key: row.get(0)?,
                        value: row.get(1)?,
                        author_nous_id: row.get(2)?,
                        ttl_seconds: row.get(3)?,
                        created_at: row.get(4)?,
                        expires_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .context(error::DatabaseSnafu)?;
        Ok(result)
    }

    /// List all non-expired blackboard entries.
    #[instrument(skip(self))]
    pub fn blackboard_list(&self) -> Result<Vec<BlackboardRow>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT key, value, author_nous_id, ttl_seconds, created_at, expires_at
                 FROM blackboard
                 WHERE expires_at IS NULL OR expires_at > datetime('now')
                 ORDER BY key ASC",
            )
            .context(error::DatabaseSnafu)?;

        let rows = stmt
            .query_map([], |row| {
                Ok(BlackboardRow {
                    key: row.get(0)?,
                    value: row.get(1)?,
                    author_nous_id: row.get(2)?,
                    ttl_seconds: row.get(3)?,
                    created_at: row.get(4)?,
                    expires_at: row.get(5)?,
                })
            })
            .context(error::DatabaseSnafu)?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.context(error::DatabaseSnafu)?);
        }
        Ok(entries)
    }

    /// Delete a blackboard entry. Only the original author can delete.
    #[instrument(skip(self))]
    pub fn blackboard_delete(&self, key: &str, author: &str) -> Result<bool> {
        self.require_writable()?;
        let rows = self
            .conn
            .execute(
                "DELETE FROM blackboard WHERE key = ?1 AND author_nous_id = ?2",
                rusqlite::params![key, author],
            )
            .context(error::DatabaseSnafu)?;
        Ok(rows > 0)
    }
}
