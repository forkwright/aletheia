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

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use snafu::ResultExt;
use tracing::{error, info, instrument, warn};

use aletheia_koina::disk_space::{DiskSpaceMonitor, DiskStatus};

use crate::error::{self, Result};
use crate::migration;
use crate::recovery::{self, RecoveryConfig, StoreMode};
use crate::types::{
    Message, Role, Session, SessionMetrics, SessionOrigin, SessionStatus, SessionType,
};

/// Hook called at connection lifecycle boundaries.
///
/// Implement this trait to observe or instrument connection acquire and release
/// events. Both methods receive a shared reference, so implementations must
/// use interior mutability (e.g. `Mutex`, `AtomicU64`) for any mutable state.
///
/// # Thread safety
///
/// Implementations must be `Send + Sync` because `SessionStore` is `Send` and
/// the hook is held for the store's lifetime, which may span thread boundaries.
pub trait ConnectionHook: Send + Sync {
    /// Called immediately before the connection is made available for use.
    ///
    /// Invoked once per [`SessionStore`] instance, after the underlying
    /// `SQLite` connection has been successfully opened and configured.
    fn before_acquire(&self);

    /// Called when the connection is released.
    ///
    /// Invoked once, during [`SessionStore`] drop. Any clean-up or final
    /// metrics flushing should happen here.
    fn after_release(&self);
}

/// The session store: wraps a `SQLite` connection with optional degraded mode.
pub struct SessionStore {
    conn: Connection,
    disk_monitor: Option<DiskSpaceMonitor>,
    mode: StoreMode,
    path: Option<PathBuf>,
    hook: Option<Box<dyn ConnectionHook>>,
}

impl SessionStore {
    /// Open (or create) a session store at the given path.
    ///
    /// When recovery is enabled, runs `PRAGMA integrity_check` on open and
    /// handles corruption automatically (backup, repair, read-only fallback).
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or initialized.
    #[instrument(skip(path))]
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_recovery(path, &RecoveryConfig::default())
    }

    /// Open a session store with explicit recovery configuration.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or initialized.
    #[instrument(skip(path, recovery_config))]
    pub fn open_with_recovery(path: &Path, recovery_config: &RecoveryConfig) -> Result<Self> {
        info!("Opening session store at {}", path.display());
        let conn = Connection::open(path).context(error::DatabaseSnafu)?;

        // PERF: WAL mode + NORMAL synchronous for write throughput without sacrificing crash safety.
        // WHY: busy_timeout prevents SQLITE_BUSY errors under concurrent writes.
        conn.execute_batch(
            "PRAGMA busy_timeout = 5000;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .context(error::DatabaseSnafu)?;

        // SAFETY: Integrity check detects corruption before any reads occur.
        if recovery_config.enabled && recovery_config.integrity_check_on_open && path.exists() {
            match recovery::check_integrity(&conn) {
                Ok(true) => { /* healthy */ }
                Ok(false) => {
                    error!(
                        path = %path.display(),
                        "integrity check failed, starting recovery"
                    );
                    drop(conn);
                    let (recovered_conn, mode) = recovery::recover_database(path, recovery_config)?;

                    if mode == StoreMode::ReadOnly {
                        warn!(path = %path.display(), "database opened in read-only (degraded) mode");
                    }

                    return Ok(Self {
                        conn: recovered_conn,
                        disk_monitor: None,
                        mode,
                        path: Some(path.to_path_buf()),
                        hook: None,
                    });
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        path = %path.display(),
                        "integrity check query failed, proceeding optimistically"
                    );
                }
            }
        }

        migration::run_migrations(&conn)?;

        Ok(Self {
            conn,
            disk_monitor: None,
            mode: StoreMode::Normal,
            path: Some(path.to_path_buf()),
            hook: None,
        })
    }

    /// Open a session store with an attached connection lifecycle hook.
    ///
    /// [`ConnectionHook::before_acquire`] is called before the connection is
    /// opened. [`ConnectionHook::after_release`] is called when the store is
    /// dropped.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or initialized.
    #[instrument(skip(path, hook))]
    pub fn open_with_hook(path: &Path, hook: Box<dyn ConnectionHook>) -> Result<Self> {
        hook.before_acquire();
        let mut store = Self::open_with_recovery(path, &RecoveryConfig::default())?;
        store.hook = Some(hook);
        Ok(store)
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
            mode: StoreMode::Normal,
            path: None,
            hook: None,
        })
    }

    /// Open an in-memory session store with an attached connection lifecycle hook.
    ///
    /// [`ConnectionHook::before_acquire`] is called before the connection is
    /// opened. [`ConnectionHook::after_release`] is called when the store is
    /// dropped.
    ///
    /// # Errors
    /// Returns an error if initialization fails.
    #[instrument(skip(hook))]
    pub fn open_in_memory_with_hook(hook: Box<dyn ConnectionHook>) -> Result<Self> {
        hook.before_acquire();
        let mut store = Self::open_in_memory()?;
        store.hook = Some(hook);
        Ok(store)
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

    /// Current operating mode of the store.
    #[must_use]
    pub fn mode(&self) -> StoreMode {
        self.mode
    }

    /// Whether the store is in degraded (read-only) mode.
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        self.mode == StoreMode::ReadOnly
    }

    /// Force a WAL checkpoint, flushing all pending writes to the main database file.
    ///
    /// Called during graceful shutdown so the WAL is explicitly flushed rather than
    /// relying on the implicit checkpoint that occurs when the connection is dropped (#1723).
    ///
    /// # Errors
    /// Returns an error if the checkpoint query fails (e.g., in read-only mode).
    pub fn checkpoint_wal(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Guard that rejects write operations when the store is degraded.
    ///
    /// # Errors
    /// Returns [`error::Error::DatabaseDegraded`] when in read-only mode.
    pub(crate) fn require_writable(&self) -> Result<()> {
        if self.mode == StoreMode::ReadOnly {
            return Err(error::DatabaseDegradedSnafu {
                path: self
                    .path
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("<in-memory>")),
            }
            .build());
        }
        Ok(())
    }
}

impl Drop for SessionStore {
    fn drop(&mut self) {
        if let Some(ref hook) = self.hook {
            hook.after_release();
        }
    }
}

// NOTE: Row mappers are `pub(super)` so sub-modules (session, message, peripherals) can reuse them.

/// Map a `SQLite` row to a [`Session`].
pub(super) fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let status_str: String = row.get("status")?;
    let type_str: String = row.get("session_type")?;

    Ok(Session {
        id: row.get("id")?,
        nous_id: row.get("nous_id")?,
        session_key: row.get("session_key")?,
        status: match status_str.as_str() {
            "archived" => SessionStatus::Archived,
            "distilled" => SessionStatus::Distilled,
            _ => SessionStatus::Active,
        },
        model: row.get("model")?,
        session_type: match type_str.as_str() {
            "background" => SessionType::Background,
            "ephemeral" => SessionType::Ephemeral,
            _ => SessionType::Primary,
        },
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        metrics: SessionMetrics {
            token_count_estimate: row.get("token_count_estimate")?,
            message_count: row.get("message_count")?,
            last_input_tokens: row.get("last_input_tokens")?,
            bootstrap_hash: row.get("bootstrap_hash")?,
            distillation_count: row.get("distillation_count")?,
            last_distilled_at: row.get("last_distilled_at")?,
            computed_context_tokens: row.get("computed_context_tokens")?,
        },
        origin: SessionOrigin {
            parent_session_id: row.get("parent_session_id")?,
            thread_id: row.get("thread_id")?,
            transport: row.get("transport")?,
            display_name: row.get("display_name")?,
        },
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
