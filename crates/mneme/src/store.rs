//! `SQLite` session store.
//!
//! WAL mode, prepared statement caching, transactional message appends.

use std::path::Path;

use rusqlite::Connection;
use snafu::ResultExt;
use tracing::{debug, info, instrument};

use crate::error::{self, Result};
use crate::migration;
use crate::types::{
    AgentNote, BlackboardRow, Message, Role, Session, SessionStatus, SessionType, UsageRecord,
};

/// The session store — wraps a `SQLite` connection.
pub struct SessionStore {
    conn: Connection,
}

impl SessionStore {
    /// Open (or create) a session store at the given path.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or initialized.
    #[instrument(skip(path))]
    pub fn open(path: &Path) -> Result<Self> {
        info!("Opening session store at {}", path.display());
        let conn = Connection::open(path).context(error::DatabaseSnafu)?;

        // Performance pragmas
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .context(error::DatabaseSnafu)?;

        migration::run_migrations(&conn)?;

        Ok(Self { conn })
    }

    /// Open an in-memory session store (for testing).
    ///
    /// # Errors
    /// Returns an error if initialization fails.
    #[instrument]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context(error::DatabaseSnafu)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .context(error::DatabaseSnafu)?;
        migration::run_migrations(&conn)?;
        Ok(Self { conn })
    }

    /// Get a reference to the underlying connection.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // --- Sessions ---

    /// Find an active session by nous ID and session key.
    #[instrument(skip(self))]
    pub fn find_session(&self, nous_id: &str, session_key: &str) -> Result<Option<Session>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT * FROM sessions WHERE nous_id = ?1 AND session_key = ?2 AND status = 'active'",
            )
            .context(error::DatabaseSnafu)?;

        let session = stmt
            .query_row([nous_id, session_key], map_session)
            .optional()
            .context(error::DatabaseSnafu)?;

        Ok(session)
    }

    /// Find a session by ID (any status).
    pub fn find_session_by_id(&self, id: &str) -> Result<Option<Session>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT * FROM sessions WHERE id = ?1")
            .context(error::DatabaseSnafu)?;

        let session = stmt
            .query_row([id], map_session)
            .optional()
            .context(error::DatabaseSnafu)?;

        Ok(session)
    }

    /// Create a new session.
    #[instrument(skip(self))]
    pub fn create_session(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        parent_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<Session> {
        let session_type = SessionType::from_key(session_key);

        self.conn
            .execute(
                "INSERT INTO sessions (id, nous_id, session_key, parent_session_id, model, session_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, nous_id, session_key, parent_session_id, model, session_type.as_str()],
            )
            .context(error::DatabaseSnafu)?;

        info!(id, nous_id, session_key, %session_type, "created session");

        self.find_session_by_id(id)?.ok_or_else(|| {
            error::SessionCreateSnafu {
                nous_id: nous_id.to_owned(),
            }
            .build()
        })
    }

    /// Find or create an active session. Reactivates archived sessions if found.
    #[instrument(skip(self))]
    pub fn find_or_create_session(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        model: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<Session> {
        // Check for active session
        if let Some(session) = self.find_session(nous_id, session_key)? {
            return Ok(session);
        }

        // Check for archived/distilled session — reactivate
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id FROM sessions WHERE nous_id = ?1 AND session_key = ?2 AND status != 'active' ORDER BY updated_at DESC LIMIT 1",
            )
            .context(error::DatabaseSnafu)?;

        let archived_id: Option<String> = stmt
            .query_row([nous_id, session_key], |row| row.get(0))
            .optional()
            .context(error::DatabaseSnafu)?;

        if let Some(archived_id) = archived_id {
            self.conn
                .execute(
                    "UPDATE sessions SET status = 'active', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                    [&archived_id],
                )
                .context(error::DatabaseSnafu)?;
            info!(
                id = archived_id,
                nous_id, session_key, "reactivated archived session"
            );
            return self.find_session_by_id(&archived_id)?.ok_or_else(|| {
                error::SessionCreateSnafu {
                    nous_id: nous_id.to_owned(),
                }
                .build()
            });
        }

        // Create new
        self.create_session(id, nous_id, session_key, parent_session_id, model)
    }

    /// List sessions, optionally filtered by nous ID.
    #[instrument(skip(self))]
    pub fn list_sessions(&self, nous_id: Option<&str>) -> Result<Vec<Session>> {
        let mut sessions = Vec::new();

        if let Some(nous_id) = nous_id {
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT * FROM sessions WHERE nous_id = ?1 ORDER BY updated_at DESC",
                )
                .context(error::DatabaseSnafu)?;
            let rows = stmt
                .query_map([nous_id], map_session)
                .context(error::DatabaseSnafu)?;
            for row in rows {
                sessions.push(row.context(error::DatabaseSnafu)?);
            }
        } else {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT * FROM sessions ORDER BY updated_at DESC")
                .context(error::DatabaseSnafu)?;
            let rows = stmt
                .query_map([], map_session)
                .context(error::DatabaseSnafu)?;
            for row in rows {
                sessions.push(row.context(error::DatabaseSnafu)?);
            }
        }

        Ok(sessions)
    }

    /// Update session status.
    #[instrument(skip(self))]
    pub fn update_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET status = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2",
                rusqlite::params![status.as_str(), id],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    // --- Messages ---

    /// Append a message to a session. Returns the sequence number.
    #[instrument(skip(self, content))]
    pub fn append_message(
        &self,
        session_id: &str,
        role: Role,
        content: &str,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        token_estimate: i64,
    ) -> Result<i64> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::DatabaseSnafu)?;

        let next_seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .context(error::DatabaseSnafu)?;

        tx.execute(
            "INSERT INTO messages (session_id, seq, role, content, tool_call_id, tool_name, token_estimate, is_distilled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            rusqlite::params![session_id, next_seq, role.as_str(), content, tool_call_id, tool_name, token_estimate],
        )
        .context(error::DatabaseSnafu)?;

        tx.execute(
            "UPDATE sessions SET message_count = message_count + 1, token_count_estimate = token_count_estimate + ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2",
            rusqlite::params![token_estimate, session_id],
        )
        .context(error::DatabaseSnafu)?;

        tx.commit().context(error::DatabaseSnafu)?;

        debug!(session_id, seq = next_seq, %role, token_estimate, "appended message");
        Ok(next_seq)
    }

    /// Get message history for a session.
    #[instrument(skip(self))]
    pub fn get_history(&self, session_id: &str, limit: Option<i64>) -> Result<Vec<Message>> {
        let mut messages = Vec::new();

        if let Some(limit) = limit {
            // Most recent N messages in chronological order
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT * FROM (SELECT * FROM messages WHERE session_id = ?1 AND is_distilled = 0 ORDER BY seq DESC LIMIT ?2) ORDER BY seq ASC",
                )
                .context(error::DatabaseSnafu)?;
            let rows = stmt
                .query_map(rusqlite::params![session_id, limit], map_message)
                .context(error::DatabaseSnafu)?;
            for row in rows {
                messages.push(row.context(error::DatabaseSnafu)?);
            }
        } else {
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT * FROM messages WHERE session_id = ?1 AND is_distilled = 0 ORDER BY seq ASC",
                )
                .context(error::DatabaseSnafu)?;
            let rows = stmt
                .query_map([session_id], map_message)
                .context(error::DatabaseSnafu)?;
            for row in rows {
                messages.push(row.context(error::DatabaseSnafu)?);
            }
        }

        Ok(messages)
    }

    /// Get message history within a token budget (most recent first, working backward).
    #[instrument(skip(self), level = "debug")]
    pub fn get_history_with_budget(
        &self,
        session_id: &str,
        max_tokens: i64,
    ) -> Result<Vec<Message>> {
        let all = self.get_history(session_id, None)?;
        let mut total: i64 = 0;
        let mut result = Vec::new();

        for msg in all.into_iter().rev() {
            if total + msg.token_estimate > max_tokens && !result.is_empty() {
                break;
            }
            total += msg.token_estimate;
            result.push(msg);
        }

        result.reverse();
        Ok(result)
    }

    /// Mark messages as distilled and recalculate session token count.
    #[instrument(skip(self, seqs), fields(count = seqs.len()))]
    pub fn mark_messages_distilled(&self, session_id: &str, seqs: &[i64]) -> Result<()> {
        if seqs.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::DatabaseSnafu)?;

        // Mark each seq as distilled
        let mut stmt = tx
            .prepare_cached(
                "UPDATE messages SET is_distilled = 1 WHERE session_id = ?1 AND seq = ?2",
            )
            .context(error::DatabaseSnafu)?;
        for seq in seqs {
            stmt.execute(rusqlite::params![session_id, seq])
                .context(error::DatabaseSnafu)?;
        }
        drop(stmt);

        // Recalculate
        let (total_tokens, msg_count): (i64, i64) = tx
            .query_row(
                "SELECT COALESCE(SUM(token_estimate), 0), COUNT(*) FROM messages WHERE session_id = ?1 AND is_distilled = 0",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context(error::DatabaseSnafu)?;

        tx.execute(
            "UPDATE sessions SET token_count_estimate = ?1, message_count = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?3",
            rusqlite::params![total_tokens, msg_count, session_id],
        )
        .context(error::DatabaseSnafu)?;

        tx.commit().context(error::DatabaseSnafu)?;

        info!(
            session_id,
            distilled = seqs.len(),
            total_tokens,
            msg_count,
            "distilled messages"
        );
        Ok(())
    }

    // --- Distillation ---

    /// Insert a distillation summary as a system message, then remove distilled messages.
    ///
    /// In a single transaction:
    /// 1. Shift seq numbers of undistilled messages up by 1
    /// 2. Insert summary at seq 0
    /// 3. Delete all messages marked `is_distilled = 1`
    #[instrument(skip(self, content))]
    pub fn insert_distillation_summary(&self, session_id: &str, content: &str) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::DatabaseSnafu)?;

        // Shift existing undistilled messages up by 1
        tx.execute(
            "UPDATE messages SET seq = seq + 1 WHERE session_id = ?1 AND is_distilled = 0",
            [session_id],
        )
        .context(error::DatabaseSnafu)?;

        // Insert summary at seq 0
        #[expect(clippy::cast_possible_wrap, reason = "summary length fits in i64")]
        let token_estimate = content.len() as i64 / 4;
        tx.execute(
            "INSERT INTO messages (session_id, seq, role, content, is_distilled, token_estimate, created_at)
             VALUES (?1, 0, 'system', ?2, 0, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            rusqlite::params![session_id, content, token_estimate],
        )
        .context(error::DatabaseSnafu)?;

        // Delete distilled messages
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND is_distilled = 1",
            [session_id],
        )
        .context(error::DatabaseSnafu)?;

        // Recalculate session counts
        let (total_tokens, msg_count): (i64, i64) = tx
            .query_row(
                "SELECT COALESCE(SUM(token_estimate), 0), COUNT(*) FROM messages WHERE session_id = ?1 AND is_distilled = 0",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context(error::DatabaseSnafu)?;

        tx.execute(
            "UPDATE sessions SET token_count_estimate = ?1, message_count = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?3",
            rusqlite::params![total_tokens, msg_count, session_id],
        )
        .context(error::DatabaseSnafu)?;

        tx.commit().context(error::DatabaseSnafu)?;

        info!(
            session_id,
            msg_count, total_tokens, "inserted distillation summary"
        );
        Ok(())
    }

    /// Record a distillation event: insert into distillations table, update session counters.
    #[instrument(skip(self))]
    pub fn record_distillation(
        &self,
        session_id: &str,
        messages_before: i64,
        messages_after: i64,
        tokens_before: i64,
        tokens_after: i64,
        model: Option<&str>,
    ) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::DatabaseSnafu)?;

        tx.execute(
            "INSERT INTO distillations (session_id, messages_before, messages_after, tokens_before, tokens_after, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![session_id, messages_before, messages_after, tokens_before, tokens_after, model],
        )
        .context(error::DatabaseSnafu)?;

        tx.execute(
            "UPDATE sessions SET distillation_count = distillation_count + 1, last_distilled_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [session_id],
        )
        .context(error::DatabaseSnafu)?;

        tx.commit().context(error::DatabaseSnafu)?;

        info!(
            session_id,
            messages_before, messages_after, tokens_before, tokens_after, "recorded distillation"
        );
        Ok(())
    }

    // --- Usage ---

    /// Record token usage for a turn.
    #[instrument(skip(self, record), level = "debug")]
    pub fn record_usage(&self, record: &UsageRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO usage (session_id, turn_seq, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    record.session_id,
                    record.turn_seq,
                    record.input_tokens,
                    record.output_tokens,
                    record.cache_read_tokens,
                    record.cache_write_tokens,
                    record.model,
                ],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    // --- Agent Notes ---

    /// Add an agent note.
    #[instrument(skip(self, content))]
    pub fn add_note(
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> Result<i64> {
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
        let rows = self
            .conn
            .execute("DELETE FROM agent_notes WHERE id = ?1", [note_id])
            .context(error::DatabaseSnafu)?;
        Ok(rows > 0)
    }

    // --- Blackboard ---

    /// Write or update a blackboard entry. Upserts on key.
    #[instrument(skip(self, value), level = "debug")]
    pub fn blackboard_write(
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_secs: i64,
    ) -> Result<()> {
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
    pub fn blackboard_delete(&self, key: &str, author: &str) -> Result<bool> {
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

// --- Row Mappers ---

/// Map a `SQLite` row to a [`Session`].
fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let status_str: String = row.get("status")?;
    let type_str: String = row.get("session_type")?;

    Ok(Session {
        id: row.get("id")?,
        nous_id: row.get("nous_id")?,
        session_key: row.get("session_key")?,
        parent_session_id: row.get("parent_session_id")?,
        status: match status_str.as_str() {
            "archived" => SessionStatus::Archived,
            "distilled" => SessionStatus::Distilled,
            _ => SessionStatus::Active,
        },
        model: row.get("model")?,
        token_count_estimate: row.get("token_count_estimate")?,
        message_count: row.get("message_count")?,
        last_input_tokens: row.get("last_input_tokens")?,
        bootstrap_hash: row.get("bootstrap_hash")?,
        distillation_count: row.get("distillation_count")?,
        session_type: match type_str.as_str() {
            "background" => SessionType::Background,
            "ephemeral" => SessionType::Ephemeral,
            _ => SessionType::Primary,
        },
        last_distilled_at: row.get("last_distilled_at")?,
        computed_context_tokens: row.get("computed_context_tokens")?,
        thread_id: row.get("thread_id")?,
        transport: row.get("transport")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Map a `SQLite` row to a [`Message`].
fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let role_str: String = row.get("role")?;
    let distilled: i64 = row.get("is_distilled")?;

    Ok(Message {
        id: row.get("id")?,
        session_id: row.get("session_id")?,
        seq: row.get("seq")?,
        role: match role_str.as_str() {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool_result" => Role::ToolResult,
            _ => Role::System,
        },
        content: row.get("content")?,
        tool_call_id: row.get("tool_call_id")?,
        tool_name: row.get("tool_name")?,
        token_estimate: row.get("token_estimate")?,
        is_distilled: distilled != 0,
        created_at: row.get("created_at")?,
    })
}

/// Extension trait for optional query results.
trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> SessionStore {
        SessionStore::open_in_memory().expect("open in-memory store")
    }

    #[test]
    fn create_and_find_session() {
        let store = test_store();
        let session = store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        assert_eq!(session.id, "ses-1");
        assert_eq!(session.nous_id, "syn");
        assert_eq!(session.session_key, "main");
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.session_type, SessionType::Primary);

        let found = store.find_session("syn", "main").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "ses-1");
    }

    #[test]
    fn find_session_returns_none_for_missing() {
        let store = test_store();
        let found = store.find_session("syn", "nonexistent").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn session_type_classification() {
        let store = test_store();

        let s1 = store
            .create_session("ses-bg", "syn", "prosoche-wake", None, None)
            .unwrap();
        assert_eq!(s1.session_type, SessionType::Background);

        let s2 = store
            .create_session("ses-eph", "syn", "ask:demiurge", None, None)
            .unwrap();
        assert_eq!(s2.session_type, SessionType::Ephemeral);

        let s3 = store
            .create_session("ses-pri", "syn", "main", None, None)
            .unwrap();
        assert_eq!(s3.session_type, SessionType::Primary);
    }

    #[test]
    fn find_or_create_reactivates_archived() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        store
            .update_session_status("ses-1", SessionStatus::Archived)
            .unwrap();

        let session = store
            .find_or_create_session("ses-new", "syn", "main", None, None)
            .unwrap();
        assert_eq!(session.id, "ses-1"); // Reactivated, not created new
        assert_eq!(session.status, SessionStatus::Active);
    }

    #[test]
    fn append_and_retrieve_messages() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        let seq1 = store
            .append_message("ses-1", Role::User, "hello", None, None, 10)
            .unwrap();
        let seq2 = store
            .append_message("ses-1", Role::Assistant, "hi there", None, None, 15)
            .unwrap();

        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);

        let history = store.get_history("ses-1", None).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[1].content, "hi there");
        assert_eq!(history[1].role, Role::Assistant);
    }

    #[test]
    fn message_updates_session_counts() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        store
            .append_message("ses-1", Role::User, "hello", None, None, 100)
            .unwrap();
        store
            .append_message("ses-1", Role::Assistant, "world", None, None, 200)
            .unwrap();

        let session = store.find_session_by_id("ses-1").unwrap().unwrap();
        assert_eq!(session.message_count, 2);
        assert_eq!(session.token_count_estimate, 300);
    }

    #[test]
    fn history_with_limit() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        for i in 1..=5 {
            store
                .append_message("ses-1", Role::User, &format!("msg {i}"), None, None, 10)
                .unwrap();
        }

        let history = store.get_history("ses-1", Some(2)).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "msg 4");
        assert_eq!(history[1].content, "msg 5");
    }

    #[test]
    fn history_with_budget() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        store
            .append_message("ses-1", Role::User, "old", None, None, 100)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "mid", None, None, 100)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "new", None, None, 100)
            .unwrap();

        let history = store.get_history_with_budget("ses-1", 200).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "mid");
        assert_eq!(history[1].content, "new");
    }

    #[test]
    fn distillation_marks_and_recalculates() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        store
            .append_message("ses-1", Role::User, "old msg 1", None, None, 100)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "old msg 2", None, None, 150)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "keep this", None, None, 50)
            .unwrap();

        // Distill the first two messages
        store.mark_messages_distilled("ses-1", &[1, 2]).unwrap();

        // History should only return undistilled
        let history = store.get_history("ses-1", None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "keep this");

        // Session counts should be recalculated
        let session = store.find_session_by_id("ses-1").unwrap().unwrap();
        assert_eq!(session.message_count, 1);
        assert_eq!(session.token_count_estimate, 50);
    }

    #[test]
    fn usage_recording() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        store
            .record_usage(&UsageRecord {
                session_id: "ses-1".to_owned(),
                turn_seq: 1,
                input_tokens: 1000,
                output_tokens: 500,
                cache_read_tokens: 800,
                cache_write_tokens: 200,
                model: Some("claude-opus-4-20250514".to_owned()),
            })
            .unwrap();

        // Verify it was stored
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage WHERE session_id = ?1",
                ["ses-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn agent_notes_crud() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        let id1 = store
            .add_note("ses-1", "syn", "task", "working on M0b")
            .unwrap();
        let id2 = store
            .add_note("ses-1", "syn", "decision", "use snafu for errors")
            .unwrap();

        let notes = store.get_notes("ses-1").unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].content, "working on M0b");
        assert_eq!(notes[1].content, "use snafu for errors");

        store.delete_note(id1).unwrap();
        let notes = store.get_notes("ses-1").unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, id2);
    }

    #[test]
    fn list_sessions_filtered() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        store
            .create_session("ses-2", "demiurge", "main", None, None)
            .unwrap();

        let all = store.list_sessions(None).unwrap();
        assert_eq!(all.len(), 2);

        let syn_only = store.list_sessions(Some("syn")).unwrap();
        assert_eq!(syn_only.len(), 1);
        assert_eq!(syn_only[0].nous_id, "syn");
    }

    #[test]
    fn tool_result_message() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        store
            .append_message(
                "ses-1",
                Role::ToolResult,
                r#"{"result": "ok"}"#,
                Some("tool_123"),
                Some("exec"),
                50,
            )
            .unwrap();

        let history = store.get_history("ses-1", None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, Role::ToolResult);
        assert_eq!(history[0].tool_call_id.as_deref(), Some("tool_123"));
        assert_eq!(history[0].tool_name.as_deref(), Some("exec"));
    }

    // --- Edge cases ---

    #[test]
    fn history_empty_session() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        let history = store.get_history("ses-1", None).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn history_limit_one() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        for i in 1..=5 {
            store
                .append_message("ses-1", Role::User, &format!("msg {i}"), None, None, 10)
                .unwrap();
        }
        let history = store.get_history("ses-1", Some(1)).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "msg 5");
    }

    #[test]
    fn history_limit_exceeds_count() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "only", None, None, 10)
            .unwrap();
        let history = store.get_history("ses-1", Some(100)).unwrap();
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn large_message_content() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        let big = "x".repeat(1_000_000);
        store
            .append_message("ses-1", Role::User, &big, None, None, 250_000)
            .unwrap();
        let history = store.get_history("ses-1", None).unwrap();
        assert_eq!(history[0].content.len(), 1_000_000);
    }

    #[test]
    fn distill_empty_seqs_is_noop() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "keep", None, None, 10)
            .unwrap();
        store.mark_messages_distilled("ses-1", &[]).unwrap();
        let history = store.get_history("ses-1", None).unwrap();
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn delete_nonexistent_note_returns_false() {
        let store = test_store();
        let deleted = store.delete_note(9999).unwrap();
        assert!(!deleted);
    }

    #[test]
    fn message_sequence_always_increases() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        let s1 = store
            .append_message("ses-1", Role::User, "a", None, None, 5)
            .unwrap();
        let s2 = store
            .append_message("ses-1", Role::Assistant, "b", None, None, 5)
            .unwrap();
        let s3 = store
            .append_message("ses-1", Role::User, "c", None, None, 5)
            .unwrap();
        assert!(s1 < s2);
        assert!(s2 < s3);
    }

    #[test]
    fn budget_always_includes_at_least_one() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "big", None, None, 999_999)
            .unwrap();
        let history = store.get_history_with_budget("ses-1", 1).unwrap();
        assert_eq!(history.len(), 1);
    }

    // --- Blackboard ---

    #[test]
    fn blackboard_crud() {
        let store = test_store();
        store
            .blackboard_write("goal", "finish M0b", "syn", 3600)
            .unwrap();

        let entry = store.blackboard_read("goal").unwrap().unwrap();
        assert_eq!(entry.key, "goal");
        assert_eq!(entry.value, "finish M0b");
        assert_eq!(entry.author_nous_id, "syn");

        let list = store.blackboard_list().unwrap();
        assert_eq!(list.len(), 1);

        let deleted = store.blackboard_delete("goal", "syn").unwrap();
        assert!(deleted);

        let gone = store.blackboard_read("goal").unwrap();
        assert!(gone.is_none());
    }

    #[test]
    fn blackboard_upsert() {
        let store = test_store();
        store
            .blackboard_write("status", "starting", "syn", 3600)
            .unwrap();
        store
            .blackboard_write("status", "running", "syn", 3600)
            .unwrap();

        let entry = store.blackboard_read("status").unwrap().unwrap();
        assert_eq!(entry.value, "running");

        let list = store.blackboard_list().unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn blackboard_delete_only_author() {
        let store = test_store();
        store
            .blackboard_write("secret", "value", "syn", 3600)
            .unwrap();

        let deleted = store.blackboard_delete("secret", "other-agent").unwrap();
        assert!(!deleted);

        let still_there = store.blackboard_read("secret").unwrap();
        assert!(still_there.is_some());
    }

    #[test]
    fn blackboard_read_missing_returns_none() {
        let store = test_store();
        let result = store.blackboard_read("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn blackboard_expiry_filtered() {
        let store = test_store();
        store.blackboard_write("temp", "data", "syn", 3600).unwrap();

        // Manually set expires_at to the past
        store
            .conn
            .execute(
                "UPDATE blackboard SET expires_at = datetime('now', '-1 second') WHERE key = 'temp'",
                [],
            )
            .unwrap();

        let result = store.blackboard_read("temp").unwrap();
        assert!(result.is_none());

        let list = store.blackboard_list().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn record_distillation_increments_count() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        let session = store.find_session_by_id("ses-1").unwrap().unwrap();
        assert_eq!(session.distillation_count, 0);
        assert!(session.last_distilled_at.is_none());

        store
            .record_distillation("ses-1", 20, 5, 50000, 2000, Some("sonnet"))
            .unwrap();

        let session = store.find_session_by_id("ses-1").unwrap().unwrap();
        assert_eq!(session.distillation_count, 1);
        assert!(session.last_distilled_at.is_some());

        store
            .record_distillation("ses-1", 15, 3, 30000, 1500, None)
            .unwrap();

        let session = store.find_session_by_id("ses-1").unwrap().unwrap();
        assert_eq!(session.distillation_count, 2);
    }

    #[test]
    fn insert_distillation_summary_and_cleanup() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();

        // Add some messages
        store
            .append_message("ses-1", Role::User, "msg1", None, None, 100)
            .unwrap();
        store
            .append_message("ses-1", Role::Assistant, "msg2", None, None, 200)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "msg3", None, None, 50)
            .unwrap();

        // Mark first two as distilled
        store.mark_messages_distilled("ses-1", &[1, 2]).unwrap();

        // Insert summary (should also delete distilled messages)
        store
            .insert_distillation_summary("ses-1", "[Distillation #1]\n\nSummary text")
            .unwrap();

        let history = store.get_history("ses-1", None).unwrap();
        // Should have: summary (seq 0) + undistilled msg3 (seq shifted)
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, Role::System);
        assert!(history[0].content.contains("Distillation #1"));
        assert_eq!(history[1].content, "msg3");

        // Session counts should reflect new state
        let session = store.find_session_by_id("ses-1").unwrap().unwrap();
        assert_eq!(session.message_count, 2);
    }
}
