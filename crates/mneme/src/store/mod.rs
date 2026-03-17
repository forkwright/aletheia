//! `SQLite` session store.
//!
//! WAL mode, prepared statement caching, transactional message appends.
//!
//! Split into sub-modules by responsibility:
//! - `session`: session CRUD operations
//! - `message`: message history, distillation pipeline, usage recording
//! - `peripherals`: agent notes and blackboard

mod message;
mod peripherals;
mod session;
#[cfg(test)]
mod tests;

use std::path::Path;

use aletheia_koina::disk_space::{DiskSpaceMonitor, DiskStatus};
use rusqlite::Connection;
use snafu::ResultExt;
use tracing::{error, info, instrument, warn};

use crate::error::{self, Result};
use crate::migration;
use crate::types::{Message, Role, Session, SessionStatus, SessionType};

/// The session store: wraps a `SQLite` connection.
pub struct SessionStore {
    conn: Connection,
    disk_monitor: Option<DiskSpaceMonitor>,
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

        Ok(Self {
            conn,
            disk_monitor: None,
        })
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
        Ok(Self {
            conn,
            disk_monitor: None,
        })
    }

    /// Attach a disk space monitor for pre-write checks.
    pub fn set_disk_monitor(&mut self, monitor: DiskSpaceMonitor) {
        self.disk_monitor = Some(monitor);
    }

    /// Emit tracing diagnostics based on current disk status.
    ///
    /// Database writes are essential and always proceed, but warnings and
    /// errors are emitted so operators can respond before the disk fills.
    pub(crate) fn check_disk(&self, operation: &str) {
        if let Some(ref monitor) = self.disk_monitor {
            match monitor.status() {
                DiskStatus::Warning { available_bytes } => {
                    let mb = available_bytes / (1024 * 1024);
                    warn!(
                        available_mb = mb,
                        operation, "disk space low, database write proceeding"
                    );
                }
                DiskStatus::Critical { available_bytes } => {
                    let mb = available_bytes / (1024 * 1024);
                    error!(
                        available_mb = mb,
                        operation, "disk space critical, database write proceeding (essential)"
                    );
                }
                _ => {}
            }
        }
    }

    /// Lightweight liveness check: executes `SELECT 1` against the connection.
    ///
    /// # Errors
    /// Returns an error if the database connection is broken.
    pub fn ping(&self) -> Result<()> {
        self.conn
            .query_row("SELECT 1", [], |_| Ok(()))
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Get a reference to the underlying connection.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

// --- Row Mappers ---

/// Map a `SQLite` row to a [`Session`].
pub(super) fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
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
        display_name: row.get("display_name")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Map a `SQLite` row to a [`Message`].
pub(super) fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
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
pub(super) trait OptionalExt<T> {
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
