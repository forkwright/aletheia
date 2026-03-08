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
            path: path.to_string_lossy().into_owned(),
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
            path: path_str.to_owned(),
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
    pub fn new(conn: &'a Connection, backup_dir: impl Into<PathBuf>) -> Self {
        Self {
            conn,
            backup_dir: backup_dir.into(),
        }
    }

    /// Create a `SQLite` backup using `VACUUM INTO`.
    #[instrument(skip(self))]
    pub fn create_backup(&self) -> Result<BackupResult> {
        std::fs::create_dir_all(&self.backup_dir).context(error::IoSnafu {
            path: self.backup_dir.display().to_string(),
        })?;

        let timestamp = jiff::Timestamp::now().strftime("%Y%m%dT%H%M%S").to_string();
        let filename = format!("sessions_{timestamp}.db");
        let backup_path = self.backup_dir.join(&filename);
        validate_backup_path(&backup_path)?;

        self.conn
            .execute(&format!("VACUUM INTO '{}'", backup_path.display()), [])
            .context(error::DatabaseSnafu)?;

        let metadata = std::fs::metadata(&backup_path).context(error::IoSnafu {
            path: backup_path.display().to_string(),
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
    pub fn export_sessions_json(&self, output_dir: &Path) -> Result<ExportResult> {
        std::fs::create_dir_all(output_dir).context(error::IoSnafu {
            path: output_dir.display().to_string(),
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
            std::fs::write(&path, json).context(error::IoSnafu {
                path: path.display().to_string(),
            })?;
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
    pub fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        if !self.backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();
        let entries = std::fs::read_dir(&self.backup_dir).context(error::IoSnafu {
            path: self.backup_dir.display().to_string(),
        })?;

        for entry in entries {
            let entry = entry.context(error::IoSnafu {
                path: self.backup_dir.display().to_string(),
            })?;
            let filename = entry.file_name().to_string_lossy().into_owned();
            if filename.starts_with("sessions_")
                && Path::new(&filename)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("db"))
            {
                let metadata = entry.metadata().context(error::IoSnafu {
                    path: entry.path().display().to_string(),
                })?;
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
    pub fn prune_backups(&self, keep: usize) -> Result<u32> {
        let backups = self.list_backups()?;
        let mut removed = 0u32;

        for backup in backups.iter().skip(keep) {
            std::fs::remove_file(&backup.path).context(error::IoSnafu {
                path: backup.path.display().to_string(),
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
mod tests {
    use super::*;
    use crate::migration;
    use crate::store::SessionStore;
    use crate::types::Role;

    fn test_store() -> SessionStore {
        SessionStore::open_in_memory().unwrap()
    }

    #[test]
    fn json_export_produces_valid_files() {
        let store = test_store();
        store
            .create_session("ses-1", "syn", "main", None, None)
            .unwrap();
        store
            .append_message("ses-1", Role::User, "hello", None, None, 10)
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let export_dir = dir.path().join("export");
        let manager = BackupManager::new(store.conn(), dir.path().join("backups"));

        let result = manager.export_sessions_json(&export_dir).unwrap();
        assert_eq!(result.sessions_exported, 1);
        assert_eq!(result.files_written, 1);

        let json_path = export_dir.join("ses-1.json");
        assert!(json_path.exists());
        let contents = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed["session"]["id"], "ses-1");
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn backup_creates_valid_sqlite_database() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("sessions.db");

        // Need a file-based DB for VACUUM INTO
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migration::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, nous_id, session_key) VALUES ('s1', 'syn', 'main')",
            [],
        )
        .unwrap();

        let backup_dir = dir.path().join("backups");
        let manager = BackupManager::new(&conn, &backup_dir);
        let result = manager.create_backup().unwrap();

        assert!(result.path.exists());
        assert!(result.size_bytes > 0);
        assert_eq!(result.sessions_count, 1);

        // Verify the backup is a valid SQLite database
        let backup_conn = Connection::open(&result.path).unwrap();
        let count: u32 = backup_conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn prune_keeps_correct_number() {
        let dir = tempfile::tempdir().unwrap();
        let backup_dir = dir.path().join("backups");
        std::fs::create_dir_all(&backup_dir).unwrap();

        // Create 5 fake backup files
        for i in 0..5 {
            std::fs::write(
                backup_dir.join(format!("sessions_2026010{i}T120000.db")),
                "fake",
            )
            .unwrap();
        }

        let conn = Connection::open_in_memory().unwrap();
        let manager = BackupManager::new(&conn, &backup_dir);

        let backups = manager.list_backups().unwrap();
        assert_eq!(backups.len(), 5);

        let removed = manager.prune_backups(2).unwrap();
        assert_eq!(removed, 3);

        let remaining = manager.list_backups().unwrap();
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    fn list_backups_returns_correct_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let backup_dir = dir.path().join("backups");
        std::fs::create_dir_all(&backup_dir).unwrap();

        std::fs::write(backup_dir.join("sessions_20260101T120000.db"), "test data").unwrap();
        // Non-matching file should be ignored
        std::fs::write(backup_dir.join("other.txt"), "ignored").unwrap();

        let conn = Connection::open_in_memory().unwrap();
        let manager = BackupManager::new(&conn, &backup_dir);

        let backups = manager.list_backups().unwrap();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].filename, "sessions_20260101T120000.db");
        assert!(backups[0].size_bytes > 0);
    }

    #[test]
    fn list_backups_empty_when_no_dir() {
        let conn = Connection::open_in_memory().unwrap();
        let manager = BackupManager::new(&conn, "/nonexistent/path");
        let backups = manager.list_backups().unwrap();
        assert!(backups.is_empty());
    }

    #[test]
    fn validate_rejects_single_quote() {
        let path = Path::new("/tmp/it's-a-trap.db");
        assert!(validate_backup_path(path).is_err());
    }

    #[test]
    fn validate_rejects_semicolon() {
        let path = Path::new("/tmp/backup;DROP TABLE sessions.db");
        assert!(validate_backup_path(path).is_err());
    }

    #[test]
    fn validate_rejects_backtick() {
        let path = Path::new("/tmp/backup`cmd`.db");
        assert!(validate_backup_path(path).is_err());
    }

    #[test]
    fn validate_rejects_double_dash() {
        let path = Path::new("/tmp/backup--comment.db");
        assert!(validate_backup_path(path).is_err());
    }

    #[test]
    fn validate_accepts_normal_path() {
        let path = Path::new("/tmp/backup-2026-01-01.db");
        assert!(validate_backup_path(path).is_ok());
    }

    #[test]
    fn validate_accepts_path_with_spaces() {
        let path = Path::new("/tmp/my backup.db");
        assert!(validate_backup_path(path).is_ok());
    }

    #[test]
    fn validate_accepts_dotted_path() {
        let path = Path::new("/home/user/.config/backup.db");
        assert!(validate_backup_path(path).is_ok());
    }

    #[test]
    fn restore_backup_preserves_data() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("sessions.db");

        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migration::run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO sessions (id, nous_id, session_key) VALUES ('s1', 'alice', 'main')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, seq, role, content, token_estimate)
             VALUES ('s1', 1, 'user', 'hello world', 10)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, seq, role, content, token_estimate)
             VALUES ('s1', 2, 'assistant', 'hi there', 8)",
            [],
        )
        .unwrap();

        let backup_dir = dir.path().join("backups");
        let manager = BackupManager::new(&conn, &backup_dir);
        let result = manager.create_backup().unwrap();

        let backup_conn = Connection::open(&result.path).unwrap();
        let session_count: u32 = backup_conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(session_count, 1);

        let msg_count: u32 = backup_conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(msg_count, 2);

        let content: String = backup_conn
            .query_row("SELECT content FROM messages WHERE seq = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn backup_path_validation_rejects_injection() {
        let bad_paths = [
            "backup'; DROP TABLE facts; --.db",
            "backup`test`.db",
            "backup;.db",
        ];
        for bad in &bad_paths {
            let path = Path::new(bad);
            assert!(
                validate_backup_path(path).is_err(),
                "path should be rejected: {bad}"
            );
        }
    }

    /// BUG: `validate_backup_path` does not reject directory traversal.
    /// `../../../etc/passwd` passes because it only contains safe SQL chars.
    /// The function guards against SQL injection in `VACUUM INTO`, not path traversal.
    /// Tracked for separate fix.
    #[test]
    fn path_traversal_not_caught_by_sql_validation() {
        let traversal = Path::new("../../../etc/passwd");
        assert!(
            validate_backup_path(traversal).is_ok(),
            "traversal passes SQL-injection validation (known gap)"
        );
    }

    #[test]
    fn backup_path_validation_accepts_safe_paths() {
        let good_paths = [
            "/tmp/backup-2026-01-01.db",
            "/home/user/.config/aletheia/backups/test.db",
            "relative/path/backup.db",
        ];
        for good in &good_paths {
            let path = Path::new(good);
            assert!(
                validate_backup_path(path).is_ok(),
                "path should be accepted: {good}"
            );
        }
    }

    #[test]
    fn json_export_is_valid_json() {
        let store = test_store();
        store
            .create_session("ses-export", "bob", "main", None, None)
            .unwrap();
        store
            .append_message("ses-export", Role::User, "test content", None, None, 5)
            .unwrap();
        store
            .append_message("ses-export", Role::Assistant, "response", None, None, 7)
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let export_dir = dir.path().join("export");
        let manager = BackupManager::new(store.conn(), dir.path().join("backups"));
        let result = manager.export_sessions_json(&export_dir).unwrap();

        assert_eq!(result.sessions_exported, 1);
        assert_eq!(result.files_written, 1);

        let json_path = export_dir.join("ses-export.json");
        let contents = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();

        assert!(parsed.is_object());
        assert!(parsed["session"].is_object());
        assert!(parsed["messages"].is_array());
        assert!(parsed["exported_at"].is_string());
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["messages"][0]["role"], "user");
    }

    #[test]
    fn backup_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("empty.db");

        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migration::run_migrations(&conn).unwrap();

        let backup_dir = dir.path().join("backups");
        let manager = BackupManager::new(&conn, &backup_dir);
        let result = manager.create_backup().unwrap();

        assert!(result.path.exists());
        assert!(result.size_bytes > 0);
        assert_eq!(result.sessions_count, 0);
        assert_eq!(result.messages_count, 0);

        let backup_conn = Connection::open(&result.path).unwrap();
        let count: u32 = backup_conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn restore_from_corrupt_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let corrupt_path = dir.path().join("corrupt.db");
        std::fs::write(&corrupt_path, b"this is not a sqlite database").unwrap();

        if let Ok(c) = Connection::open(&corrupt_path) {
            let result = c.query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                row.get::<_, u32>(0)
            });
            assert!(result.is_err(), "querying corrupt DB should fail");
        }
    }
}
