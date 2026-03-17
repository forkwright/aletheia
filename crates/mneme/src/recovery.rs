//! SQLite corruption detection, read-only fallback, and auto-repair.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use snafu::ResultExt;
use tracing::{error, info, warn};

use crate::error::{self, Result};

/// Database operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    /// Normal read-write operation.
    Normal,
    /// Degraded: corruption detected, writes are rejected.
    ReadOnly,
}

/// Recovery configuration matching taxis `SqliteRecoverySettings`.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "config struct: each bool is an independent toggle"
)]
pub struct RecoveryConfig {
    /// Whether corruption recovery is active.
    pub enabled: bool,
    /// Run `PRAGMA integrity_check` when opening a database.
    pub integrity_check_on_open: bool,
    /// Attempt to dump readable data into a new database on corruption.
    pub auto_repair: bool,
    /// Copy the corrupt file to `{path}.corrupt.{timestamp}` before repair.
    pub backup_corrupt: bool,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            integrity_check_on_open: true,
            auto_repair: true,
            backup_corrupt: true,
        }
    }
}

/// Run `PRAGMA integrity_check` and return whether the database is healthy.
///
/// Returns `Ok(true)` if the database passes, `Ok(false)` if corruption
/// is detected. Returns `Err` only for connection-level failures.
pub fn check_integrity(conn: &Connection) -> Result<bool> {
    let result: String = conn
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .context(error::DatabaseSnafu)?;

    Ok(result == "ok")
}

/// Check whether a `rusqlite::Error` indicates database corruption.
#[must_use]
pub fn is_corruption_error(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi_err, _) => matches!(
            ffi_err.code,
            rusqlite::ErrorCode::DatabaseCorrupt
                | rusqlite::ErrorCode::NotADatabase
                | rusqlite::ErrorCode::DiskFull
        ),
        _ => false,
    }
}

/// Open a database in read-only mode for data recovery.
///
/// # Errors
/// Returns an error if the read-only connection cannot be opened.
pub fn open_read_only(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context(error::DatabaseSnafu)?;
    Ok(conn)
}

/// Back up a corrupt database file by copying it to `{path}.corrupt.{timestamp}`.
///
/// Returns the path of the backup file.
///
/// # Errors
/// Returns an error if the copy fails.
pub fn backup_corrupt_file(path: &Path) -> Result<PathBuf> {
    let timestamp = jiff::Zoned::now().strftime("%Y%m%dT%H%M%S");
    let backup_name = format!(
        "{}.corrupt.{timestamp}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    let backup_path = path.with_file_name(backup_name);

    std::fs::copy(path, &backup_path).context(error::IoSnafu {
        path: path.to_path_buf(),
    })?;

    info!(
        backup = %backup_path.display(),
        original = %path.display(),
        "backed up corrupt database"
    );

    Ok(backup_path)
}

/// Attempt to recover readable data from a corrupt database into a new one.
///
/// Iterates all user tables in the corrupt database and copies rows that
/// are still readable into a freshly initialized database at `new_path`.
///
/// Returns `true` if recovery produced a usable database (migrations applied
/// and at least the schema was recreated), `false` if recovery failed entirely.
///
/// # Errors
/// Returns an error only for fatal I/O failures. Partial read failures from
/// the corrupt database are logged and skipped.
pub fn attempt_recovery(corrupt_path: &Path, new_path: &Path) -> Result<bool> {
    // WHY: Open the corrupt database read-only so we don't modify it.
    let old_conn = match open_read_only(corrupt_path) {
        Ok(c) => c,
        Err(e) => {
            error!(
                path = %corrupt_path.display(),
                error = %e,
                "cannot open corrupt database for recovery"
            );
            return Ok(false);
        }
    };

    // WHY: SQLite opens garbage files without error; the failure comes when
    // you actually query. Verify the source is a real database before
    // creating the recovery target.
    if old_conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .is_err()
    {
        error!(
            path = %corrupt_path.display(),
            "source file is not a valid SQLite database"
        );
        return Ok(false);
    }

    // Create a fresh database and run migrations to set up schema.
    let new_conn = Connection::open(new_path).context(error::DatabaseSnafu)?;
    new_conn
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = OFF;",
        )
        .context(error::DatabaseSnafu)?;

    crate::migration::run_migrations(&new_conn)?;

    // Disable FK checks during recovery so we can insert in any order.
    let tables = list_user_tables(&old_conn);

    let mut total_rows = 0u64;
    let mut failed_tables = Vec::new();

    for table in &tables {
        match copy_table(&old_conn, &new_conn, table) {
            Ok(count) => {
                total_rows = total_rows.saturating_add(count);
                info!(table, rows = count, "recovered table");
            }
            Err(e) => {
                warn!(table, error = %e, "skipped unreadable table during recovery");
                failed_tables.push(table.as_str());
            }
        }
    }

    // Re-enable FK checks.
    let _ = new_conn.execute_batch("PRAGMA foreign_keys = ON;");

    if failed_tables.len() == tables.len() && !tables.is_empty() {
        error!("recovery failed: all tables unreadable");
        // Clean up the failed recovery file.
        let _ = std::fs::remove_file(new_path);
        return Ok(false);
    }

    info!(
        total_rows,
        recovered_tables = tables.len() - failed_tables.len(),
        skipped_tables = failed_tables.len(),
        "database recovery complete"
    );

    Ok(true)
}

/// List user tables in the database (excludes `sqlite` internals and `schema_version`).
fn list_user_tables(conn: &Connection) -> Vec<String> {
    // SAFETY: This query is read-only and uses a system table.
    let mut tables = Vec::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    ) else {
        return tables;
    };

    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return tables;
    };

    for name in rows.flatten() {
        tables.push(name);
    }
    tables
}

/// Copy all readable rows from one table to another.
///
/// Returns the number of rows copied.
fn copy_table(
    src: &Connection,
    dst: &Connection,
    table: &str,
) -> std::result::Result<u64, rusqlite::Error> {
    // Get column names for the table.
    let columns = {
        let mut stmt = src.prepare(&format!(
            "PRAGMA table_info('{}')",
            table.replace('\'', "''")
        ))?;
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(std::result::Result::ok)
            .collect();
        cols
    };

    if columns.is_empty() {
        return Ok(0);
    }

    let col_list = columns.join(", ");
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
    let placeholder_list = placeholders.join(", ");

    let select_sql = format!("SELECT {col_list} FROM {table}");
    let insert_sql =
        format!("INSERT OR IGNORE INTO {table} ({col_list}) VALUES ({placeholder_list})");

    let mut select_stmt = src.prepare(&select_sql)?;
    let column_count = columns.len();

    let tx = dst.unchecked_transaction()?;
    let mut insert_stmt = tx.prepare(&insert_sql)?;
    let mut count = 0u64;

    let rows = select_stmt.query_map([], |row| {
        let mut values: Vec<rusqlite::types::Value> = Vec::with_capacity(column_count);
        for i in 0..column_count {
            values.push(row.get(i)?);
        }
        Ok(values)
    })?;

    for row in rows {
        let Ok(values) = row else { continue };
        let params: Vec<&dyn rusqlite::types::ToSql> = values
            .iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();
        if insert_stmt.execute(params.as_slice()).is_ok() {
            count = count.saturating_add(1);
        }
    }

    drop(insert_stmt);
    tx.commit()?;
    Ok(count)
}

/// Perform the full recovery workflow for a corrupt database.
///
/// 1. Back up the corrupt file (if configured)
/// 2. Attempt data recovery into a new file
/// 3. If recovery succeeds, replace the original with the recovered file
/// 4. Return a connection to the recovered (or read-only) database
///
/// Returns `(Connection, StoreMode)`: the usable connection and its mode.
pub fn recover_database(path: &Path, config: &RecoveryConfig) -> Result<(Connection, StoreMode)> {
    let path_display = path.display().to_string();

    error!(
        path = %path_display,
        "database corruption detected, starting recovery"
    );

    // Step 1: Back up corrupt file.
    if config.backup_corrupt {
        match backup_corrupt_file(path) {
            Ok(backup_path) => {
                info!(backup = %backup_path.display(), "corrupt file backed up");
            }
            Err(e) => {
                warn!(error = %e, "failed to back up corrupt file, continuing recovery");
            }
        }
    }

    // Step 2: Attempt auto-repair.
    if config.auto_repair {
        let recovery_path = path.with_extension("recovery");

        match attempt_recovery(path, &recovery_path) {
            Ok(true) => {
                // Step 3: Swap recovered database into place.
                if let Err(e) = std::fs::rename(&recovery_path, path) {
                    warn!(
                        error = %e,
                        "failed to swap recovered database, falling back to read-only"
                    );
                } else {
                    info!(path = %path_display, "recovered database swapped into place");

                    // Reopen the recovered database in normal mode.
                    let conn = Connection::open(path).context(error::DatabaseSnafu)?;
                    conn.execute_batch(
                        "PRAGMA journal_mode = WAL;
                         PRAGMA synchronous = NORMAL;
                         PRAGMA foreign_keys = ON;",
                    )
                    .context(error::DatabaseSnafu)?;

                    return Ok((conn, StoreMode::Normal));
                }
            }
            Ok(false) => {
                warn!(path = %path_display, "auto-repair failed, falling back to read-only");
                let _ = std::fs::remove_file(&recovery_path);
            }
            Err(e) => {
                warn!(
                    error = %e,
                    path = %path_display,
                    "auto-repair error, falling back to read-only"
                );
                let _ = std::fs::remove_file(&recovery_path);
            }
        }
    }

    // Step 4: Fall back to read-only mode.
    let conn = open_read_only(path)?;
    Ok((conn, StoreMode::ReadOnly))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn healthy_database_passes_integrity_check() {
        let conn = Connection::open_in_memory().expect("open in-memory");
        assert!(
            check_integrity(&conn).expect("integrity check should succeed"),
            "fresh database should pass integrity check"
        );
    }

    #[test]
    fn is_corruption_error_detects_corrupt_code() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::DatabaseCorrupt,
                extended_code: 11,
            },
            Some("database disk image is malformed".to_owned()),
        );
        assert!(
            is_corruption_error(&err),
            "DatabaseCorrupt should be detected as corruption"
        );
    }

    #[test]
    fn is_corruption_error_detects_not_a_db() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::NotADatabase,
                extended_code: 26,
            },
            Some("file is not a database".to_owned()),
        );
        assert!(
            is_corruption_error(&err),
            "NotADatabase should be detected as corruption"
        );
    }

    #[test]
    fn is_corruption_error_ignores_normal_errors() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: 19,
            },
            Some("UNIQUE constraint failed".to_owned()),
        );
        assert!(
            !is_corruption_error(&err),
            "ConstraintViolation should not be detected as corruption"
        );
    }

    #[test]
    fn backup_corrupt_file_creates_copy() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let db_path = tmp.path().join("test.db");
        std::fs::write(&db_path, b"corrupt data").expect("write test file");

        let backup_path = backup_corrupt_file(&db_path).expect("backup should succeed");

        assert!(backup_path.exists(), "backup file should exist");
        assert_eq!(
            std::fs::read(&backup_path).expect("read backup"),
            b"corrupt data",
            "backup contents should match original"
        );

        let backup_name = backup_path
            .file_name()
            .expect("has filename")
            .to_string_lossy();
        assert!(
            backup_name.starts_with("test.db.corrupt."),
            "backup should have .corrupt.timestamp suffix, got: {backup_name}"
        );
    }

    #[test]
    fn recovery_from_valid_database() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let src_path = tmp.path().join("source.db");
        let dst_path = tmp.path().join("recovered.db");

        // Create a valid source database with some data.
        {
            let conn = Connection::open(&src_path).expect("open source");
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
                .expect("set pragmas");
            crate::migration::run_migrations(&conn).expect("run migrations");
            conn.execute(
                "INSERT INTO sessions (id, nous_id, session_key) VALUES ('s1', 'test', 'main')",
                [],
            )
            .expect("insert session");
        }

        let recovered = attempt_recovery(&src_path, &dst_path).expect("recovery should succeed");
        assert!(recovered, "recovery from valid database should succeed");
        assert!(dst_path.exists(), "recovered database file should exist");

        // Verify the recovered database has the session.
        let conn = Connection::open(&dst_path).expect("open recovered");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .expect("count sessions");
        assert_eq!(count, 1, "recovered database should contain the session");
    }

    #[test]
    fn recovery_from_garbage_file_returns_false() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let corrupt_path = tmp.path().join("garbage.db");
        let new_path = tmp.path().join("recovered.db");

        std::fs::write(&corrupt_path, b"this is not a database at all")
            .expect("write garbage file");

        let recovered =
            attempt_recovery(&corrupt_path, &new_path).expect("recovery should not error");
        assert!(
            !recovered,
            "recovery from total garbage should return false"
        );
    }

    #[test]
    fn open_read_only_prevents_writes() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let db_path = tmp.path().join("readonly.db");

        // Create a database first.
        {
            let conn = Connection::open(&db_path).expect("create db");
            conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY)")
                .expect("create table");
        }

        let ro_conn = open_read_only(&db_path).expect("open read-only");
        let result = ro_conn.execute("INSERT INTO t VALUES (1)", []);
        assert!(
            result.is_err(),
            "writes should fail on read-only connection"
        );
    }

    #[test]
    fn full_recovery_workflow_with_valid_db() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let db_path = tmp.path().join("sessions.db");

        // Create a valid database.
        {
            let conn = Connection::open(&db_path).expect("open db");
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
                .expect("set pragmas");
            crate::migration::run_migrations(&conn).expect("run migrations");
            conn.execute(
                "INSERT INTO sessions (id, nous_id, session_key) VALUES ('s1', 'test', 'main')",
                [],
            )
            .expect("insert session");
        }

        let config = RecoveryConfig {
            enabled: true,
            integrity_check_on_open: true,
            auto_repair: true,
            backup_corrupt: true,
        };

        let (conn, mode) = recover_database(&db_path, &config).expect("recovery should succeed");
        assert_eq!(mode, StoreMode::Normal, "should recover to normal mode");

        // Verify data survived.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .expect("count sessions");
        assert_eq!(count, 1, "session should survive recovery");

        // Verify backup was created.
        let backups: Vec<_> = std::fs::read_dir(tmp.path())
            .expect("read dir")
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt."))
            .collect();
        assert_eq!(backups.len(), 1, "exactly one backup file should exist");
    }
}
