//! Database backup and JSON export.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use snafu::ResultExt;
use tracing::{info, instrument};

use crate::error::{self, Result};

/// Validate a backup path contains only safe characters for SQL interpolation.
///
/// `SQLite` `VACUUM INTO` doesn't support parameter binding, so this is the
/// only defense against path injection.
fn validate_backup_path(path: &Path) -> Result<()> {
    let path_str = path.to_str().ok_or_else(|| {
        error::InvalidBackupPathSnafu {
            path: path.to_path_buf(),
        }
        .build()
    })?;

    let has_safe_chars = path_str
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '\\' | ' '));

    let has_sql_comment = path_str.contains("--");

    snafu::ensure!(
        has_safe_chars && !has_sql_comment,
        error::InvalidBackupPathSnafu {
            path: path.to_path_buf(),
        }
    );

    Ok(())
}

/// Manages database backups and exports.
pub struct BackupManager<'a> {
    conn: &'a Connection,
    backup_dir: PathBuf,
}

/// Outcome of creating a backup.
#[derive(Debug)]
pub struct BackupResult {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sessions_count: u32,
    pub messages_count: u32,
}

/// Outcome of a JSON export.
#[derive(Debug)]
pub struct ExportResult {
    pub output_dir: PathBuf,
    pub sessions_exported: u32,
    pub files_written: u32,
}

/// Metadata about an existing backup file.
#[derive(Debug)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub filename: String,
}

impl<'a> BackupManager<'a> {
    /// Create a backup manager for the given connection and backup directory.
    #[instrument(skip(conn, backup_dir))]
    pub fn new(conn: &'a Connection, backup_dir: impl Into<PathBuf>) -> Self {
        Self {
            conn,
            backup_dir: backup_dir.into(),
        }
    }

    /// Create a `SQLite` backup using `VACUUM INTO`.
    ///
    /// # SQL injection defense
    ///
    /// `VACUUM INTO` does not support parameter binding for the target path.
    /// The path is interpolated via `format!` into the SQL string. The sole
    /// defense against path injection is `validate_backup_path`, which
    /// rejects any path containing characters outside the safe set
    /// (alphanumeric, `-`, `_`, `.`, `/`, `\`, space) and blocks `--` comment
    /// sequences. Any future changes to path construction MUST go through
    /// `validate_backup_path` (a private helper in this module).
    #[instrument(skip(self))]
    #[must_use = "this returns a Result that may contain a write error"]
    pub fn create_backup(&self) -> Result<BackupResult> {
        std::fs::create_dir_all(&self.backup_dir).context(error::IoSnafu {
            path: self.backup_dir.clone(),
        })?;

        let timestamp = jiff::Timestamp::now().strftime("%Y%m%dT%H%M%S").to_string();
        let filename = format!("sessions_{timestamp}.db");
        let backup_path = self.backup_dir.join(&filename);
        validate_backup_path(&backup_path)?;

        self.conn
            .execute(&format!("VACUUM INTO '{}'", backup_path.display()), [])
            .context(error::DatabaseSnafu)?;

        let metadata = std::fs::metadata(&backup_path).context(error::IoSnafu {
            path: backup_path.clone(),
        })?;

        let sessions_count: u32 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .context(error::DatabaseSnafu)?;

        let messages_count: u32 = self
            .conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .context(error::DatabaseSnafu)?;

        info!(
            path = %backup_path.display(),
            size = metadata.len(),
            sessions = sessions_count,
            messages = messages_count,
            "backup created"
        );

        Ok(BackupResult {
            path: backup_path,
            size_bytes: metadata.len(),
            sessions_count,
            messages_count,
        })
    }

    /// Export all sessions as individual JSON files.
    #[instrument(skip(self))]
    #[must_use = "this returns a Result that may contain an I/O error"]
    pub fn export_sessions_json(&self, output_dir: &Path) -> Result<ExportResult> {
        std::fs::create_dir_all(output_dir).context(error::IoSnafu {
            path: output_dir.to_path_buf(),
        })?;

        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM sessions ORDER BY updated_at DESC")
            .context(error::DatabaseSnafu)?;

        let session_ids: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .context(error::DatabaseSnafu)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(error::DatabaseSnafu)?;

        let mut files_written = 0u32;
        for session_id in &session_ids {
            let json = build_session_json(self.conn, session_id)?;
            let path = output_dir.join(format!("{session_id}.json"));
            std::fs::write(&path, json).context(error::IoSnafu { path: path.clone() })?;
            files_written += 1;
        }

        let count = u32::try_from(session_ids.len()).unwrap_or(u32::MAX);
        info!(sessions = count, dir = %output_dir.display(), "JSON export complete");

        Ok(ExportResult {
            output_dir: output_dir.to_path_buf(),
            sessions_exported: count,
            files_written,
        })
    }

    /// List available backup files.
    #[instrument(skip(self))]
    #[must_use = "this returns a Result that may contain a query error"]
    pub fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        if !self.backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();
        let entries = std::fs::read_dir(&self.backup_dir).context(error::IoSnafu {
            path: self.backup_dir.clone(),
        })?;

        for entry in entries {
            let entry = entry.context(error::IoSnafu {
                path: self.backup_dir.clone(),
            })?;
            let filename = entry.file_name().to_string_lossy().into_owned();
            if filename.starts_with("sessions_")
                && Path::new(&filename)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("db"))
            {
                let metadata = entry
                    .metadata()
                    .context(error::IoSnafu { path: entry.path() })?;
                backups.push(BackupInfo {
                    path: entry.path(),
                    size_bytes: metadata.len(),
                    filename,
                });
            }
        }

        backups.sort_by(|a, b| b.filename.cmp(&a.filename));
        Ok(backups)
    }

    /// Prune old backups, keeping the N most recent.
    #[instrument(skip(self))]
    #[must_use = "this returns a Result that may contain a deletion error"]
    pub fn prune_backups(&self, keep: usize) -> Result<u32> {
        let backups = self.list_backups()?;
        let mut removed = 0u32;

        for backup in backups.iter().skip(keep) {
            std::fs::remove_file(&backup.path).context(error::IoSnafu {
                path: backup.path.clone(),
            })?;
            removed += 1;
        }

        if removed > 0 {
            info!(removed, kept = keep, "pruned old backups");
        }

        Ok(removed)
    }
}

fn build_session_json(conn: &Connection, session_id: &str) -> Result<String> {
    let session: serde_json::Value = conn
        .query_row(
            "SELECT id, nous_id, session_key, status, model, session_type,
                    token_count_estimate, message_count, created_at, updated_at
             FROM sessions WHERE id = ?1",
            [session_id],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "nous_id": row.get::<_, String>(1)?,
                    "session_key": row.get::<_, String>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "model": row.get::<_, Option<String>>(4)?,
                    "session_type": row.get::<_, String>(5)?,
                    "token_count_estimate": row.get::<_, i64>(6)?,
                    "message_count": row.get::<_, i64>(7)?,
                    "created_at": row.get::<_, String>(8)?,
                    "updated_at": row.get::<_, String>(9)?,
                }))
            },
        )
        .context(error::DatabaseSnafu)?;

    let mut msg_stmt = conn
        .prepare_cached(
            "SELECT seq, role, content, token_estimate, created_at
             FROM messages WHERE session_id = ?1 ORDER BY seq ASC",
        )
        .context(error::DatabaseSnafu)?;

    let messages: Vec<serde_json::Value> = msg_stmt
        .query_map([session_id], |row| {
            Ok(serde_json::json!({
                "seq": row.get::<_, i64>(0)?,
                "role": row.get::<_, String>(1)?,
                "content": row.get::<_, String>(2)?,
                "token_estimate": row.get::<_, i64>(3)?,
                "created_at": row.get::<_, String>(4)?,
            }))
        })
        .context(error::DatabaseSnafu)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context(error::DatabaseSnafu)?;

    let archive = serde_json::json!({
        "session": session,
        "messages": messages,
        "exported_at": jiff::Timestamp::now().to_string(),
    });

    serde_json::to_string_pretty(&archive).context(error::StoredJsonSnafu)
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod tests;
