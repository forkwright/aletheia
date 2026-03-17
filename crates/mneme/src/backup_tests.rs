#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;
use crate::migration;
use crate::store::SessionStore;
use crate::types::Role;

fn test_store() -> SessionStore {
    SessionStore::open_in_memory().expect("open in-memory session store")
}

#[test]
fn json_export_produces_valid_files() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session ses-1");
    store
        .append_message("ses-1", Role::User, "hello", None, None, 10)
        .expect("append user message");

    let dir = tempfile::tempdir().expect("create temp dir");
    let export_dir = dir.path().join("export");
    let manager = BackupManager::new(store.conn(), dir.path().join("backups"));

    let result = manager
        .export_sessions_json(&export_dir)
        .expect("export sessions as JSON")
        .expect("export should not be skipped without disk monitor");
    assert_eq!(result.sessions_exported, 1);
    assert_eq!(result.files_written, 1);

    let json_path = export_dir.join("ses-1.json");
    assert!(json_path.exists());
    let contents = std::fs::read_to_string(&json_path).expect("read exported JSON file");
    let parsed: serde_json::Value = serde_json::from_str(&contents).expect("parse exported JSON");
    assert_eq!(parsed["session"]["id"], "ses-1");
    assert_eq!(
        parsed["messages"]
            .as_array()
            .expect("messages is array")
            .len(),
        1
    );
}

#[test]
fn backup_creates_valid_sqlite_database() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("sessions.db");

    // Need a file-based DB for VACUUM INTO
    let conn = Connection::open(&db_path).expect("open file-based SQLite connection");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    migration::run_migrations(&conn).expect("run migrations");
    conn.execute(
        "INSERT INTO sessions (id, nous_id, session_key) VALUES ('s1', 'syn', 'main')",
        [],
    )
    .expect("insert test session");

    let backup_dir = dir.path().join("backups");
    let manager = BackupManager::new(&conn, &backup_dir);
    let result = manager
        .create_backup()
        .expect("create backup")
        .expect("backup should not be skipped without disk monitor");

    assert!(result.path.exists());
    assert!(result.size_bytes > 0);
    assert_eq!(result.sessions_count, 1);

    // Verify the backup is a valid SQLite database
    let backup_conn = Connection::open(&result.path).expect("open backup SQLite database");
    let count: u32 = backup_conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("query session count from backup");
    assert_eq!(count, 1);
}

#[test]
fn prune_keeps_correct_number() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).expect("create backup dir");

    // Create 5 fake backup files
    for i in 0..5 {
        std::fs::write(
            backup_dir.join(format!("sessions_2026010{i}T120000.db")),
            "fake",
        )
        .expect("write fake backup file");
    }

    let conn = Connection::open_in_memory().expect("open in-memory SQLite connection");
    let manager = BackupManager::new(&conn, &backup_dir);

    let backups = manager.list_backups().expect("list backups");
    assert_eq!(backups.len(), 5);

    let removed = manager.prune_backups(2).expect("prune backups keeping 2");
    assert_eq!(removed, 3);

    let remaining = manager.list_backups().expect("list remaining backups");
    assert_eq!(remaining.len(), 2);
}

#[test]
fn list_backups_returns_correct_metadata() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).expect("create backup dir");

    std::fs::write(backup_dir.join("sessions_20260101T120000.db"), "test data")
        .expect("write backup file");
    // Non-matching file should be ignored
    std::fs::write(backup_dir.join("other.txt"), "ignored").expect("write non-matching file");

    let conn = Connection::open_in_memory().expect("open in-memory SQLite connection");
    let manager = BackupManager::new(&conn, &backup_dir);

    let backups = manager.list_backups().expect("list backups");
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].filename, "sessions_20260101T120000.db");
    assert!(backups[0].size_bytes > 0);
}

#[test]
fn list_backups_empty_when_no_dir() {
    let conn = Connection::open_in_memory().expect("open in-memory SQLite connection");
    let manager = BackupManager::new(&conn, "/nonexistent/path");
    let backups = manager
        .list_backups()
        .expect("list backups for nonexistent dir");
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
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("sessions.db");

    let conn = Connection::open(&db_path).expect("open file-based SQLite connection");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    migration::run_migrations(&conn).expect("run migrations");

    conn.execute(
        "INSERT INTO sessions (id, nous_id, session_key) VALUES ('s1', 'alice', 'main')",
        [],
    )
    .expect("insert test session");
    conn.execute(
        "INSERT INTO messages (session_id, seq, role, content, token_estimate)
         VALUES ('s1', 1, 'user', 'hello world', 10)",
        [],
    )
    .expect("insert first test message");
    conn.execute(
        "INSERT INTO messages (session_id, seq, role, content, token_estimate)
         VALUES ('s1', 2, 'assistant', 'hi there', 8)",
        [],
    )
    .expect("insert second test message");

    let backup_dir = dir.path().join("backups");
    let manager = BackupManager::new(&conn, &backup_dir);
    let result = manager
        .create_backup()
        .expect("create backup")
        .expect("backup should not be skipped without disk monitor");

    let backup_conn = Connection::open(&result.path).expect("open backup SQLite database");
    let session_count: u32 = backup_conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("query session count from backup");
    assert_eq!(session_count, 1);

    let msg_count: u32 = backup_conn
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("query message count from backup");
    assert_eq!(msg_count, 2);

    let content: String = backup_conn
        .query_row("SELECT content FROM messages WHERE seq = 1", [], |row| {
            row.get(0)
        })
        .expect("query first message content from backup");
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
fn validate_rejects_null_byte() {
    let path = Path::new("/tmp/backup\x00.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_parentheses() {
    let path = Path::new("/tmp/backup(1).db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_dollar_sign() {
    let path = Path::new("/tmp/$HOME/backup.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_pipe() {
    let path = Path::new("/tmp/backup|evil.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_angle_brackets() {
    let path = Path::new("/tmp/backup<script>.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_curly_braces() {
    let path = Path::new("/tmp/backup{0}.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_at_sign() {
    let path = Path::new("/tmp/backup@host.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_exclamation() {
    let path = Path::new("/tmp/backup!.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_hash() {
    let path = Path::new("/tmp/backup#1.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_percent() {
    let path = Path::new("/tmp/backup%20.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_double_quote() {
    let path = Path::new("/tmp/backup\"quoted\".db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_ampersand() {
    let path = Path::new("/tmp/backup&cmd.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_asterisk() {
    let path = Path::new("/tmp/backup*.db");
    assert!(validate_backup_path(path).is_err());
}

#[test]
fn validate_rejects_all_sqlite_metacharacters() {
    let metachar_paths = [
        "backup's.db",
        "backup;DROP TABLE facts.db",
        "backup--evil.db",
        "backup`test`.db",
        "backup\"quoted\".db",
        "backup|pipe.db",
        "backup$var.db",
        "backup(paren).db",
        "backup{brace}.db",
        "backup<angle>.db",
        "backup&amp.db",
        "backup*glob.db",
        "backup?wildcard.db",
        "backup~tilde.db",
        "backup^caret.db",
        "backup[bracket].db",
    ];
    for bad in &metachar_paths {
        let path = Path::new(bad);
        assert!(
            validate_backup_path(path).is_err(),
            "path should be rejected: {bad}"
        );
    }
}

#[test]
fn validate_rejects_unicode_control_chars() {
    let paths_with_unicode = [
        "/tmp/backup\u{200B}evil.db", // zero-width space
        "/tmp/backup\u{202E}cod.db",  // RTL override
        "/tmp/backup\u{FEFF}bom.db",  // BOM / zero-width no-break space
        "/tmp/backup\u{200D}zwj.db",  // zero-width joiner
    ];
    for bad in &paths_with_unicode {
        let path = Path::new(bad);
        assert!(
            validate_backup_path(path).is_err(),
            "path with unicode control char should be rejected: {bad:?}"
        );
    }
}

#[test]
fn validate_accepts_unicode_alphanumeric() {
    let path = Path::new("/tmp/backup-données.db");
    assert!(
        validate_backup_path(path).is_ok(),
        "unicode alphanumeric chars should be accepted"
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
        .expect("create session ses-export");
    store
        .append_message("ses-export", Role::User, "test content", None, None, 5)
        .expect("append user message");
    store
        .append_message("ses-export", Role::Assistant, "response", None, None, 7)
        .expect("append assistant message");

    let dir = tempfile::tempdir().expect("create temp dir");
    let export_dir = dir.path().join("export");
    let manager = BackupManager::new(store.conn(), dir.path().join("backups"));
    let result = manager
        .export_sessions_json(&export_dir)
        .expect("export sessions as JSON")
        .expect("export should not be skipped without disk monitor");

    assert_eq!(result.sessions_exported, 1);
    assert_eq!(result.files_written, 1);

    let json_path = export_dir.join("ses-export.json");
    let contents = std::fs::read_to_string(&json_path).expect("read exported JSON file");
    let parsed: serde_json::Value = serde_json::from_str(&contents).expect("parse exported JSON");

    assert!(parsed.is_object());
    assert!(parsed["session"].is_object());
    assert!(parsed["messages"].is_array());
    assert!(parsed["exported_at"].is_string());
    assert_eq!(
        parsed["messages"]
            .as_array()
            .expect("messages is array")
            .len(),
        2
    );
    assert_eq!(parsed["messages"][0]["role"], "user");
}

#[test]
fn backup_empty_store() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("empty.db");

    let conn = Connection::open(&db_path).expect("open file-based SQLite connection");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    migration::run_migrations(&conn).expect("run migrations");

    let backup_dir = dir.path().join("backups");
    let manager = BackupManager::new(&conn, &backup_dir);
    let result = manager
        .create_backup()
        .expect("create backup of empty store")
        .expect("backup should not be skipped without disk monitor");

    assert!(result.path.exists());
    assert!(result.size_bytes > 0);
    assert_eq!(result.sessions_count, 0);
    assert_eq!(result.messages_count, 0);

    let backup_conn = Connection::open(&result.path).expect("open backup SQLite database");
    let count: u32 = backup_conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("query session count from backup");
    assert_eq!(count, 0);
}

#[test]
fn backup_path_with_spaces() {
    let path = Path::new("/home/my user/backup dir/sessions 2026.db");
    assert!(
        validate_backup_path(path).is_ok(),
        "paths with spaces should be accepted"
    );
}

#[test]
fn backup_path_with_dots() {
    let traversal = Path::new("../../etc/shadow.db");
    assert!(
        validate_backup_path(traversal).is_ok(),
        ".. components pass SQL-injection validation (path traversal is a separate concern)"
    );

    let dotted = Path::new("/home/user/.local/share/aletheia/backup.2026.01.01.db");
    assert!(
        validate_backup_path(dotted).is_ok(),
        "dotted paths are safe for SQL"
    );
}

#[test]
fn backup_path_empty_string() {
    let path = Path::new("");
    // Empty string passes SQL-injection validation (all chars are vacuously safe).
    // This documents current behavior: empty paths would fail at the filesystem
    // level during VACUUM INTO, not at validation time.
    assert!(
        validate_backup_path(path).is_ok(),
        "empty path passes SQL validation (filesystem rejects it later)"
    );
}

#[test]
fn prune_keeps_zero() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).expect("create backup dir");

    for i in 0..4 {
        std::fs::write(
            backup_dir.join(format!("sessions_2026020{i}T120000.db")),
            "data",
        )
        .expect("write fake backup file");
    }

    let conn = Connection::open_in_memory().expect("open in-memory SQLite connection");
    let manager = BackupManager::new(&conn, &backup_dir);

    let removed = manager.prune_backups(0).expect("prune all backups");
    assert_eq!(removed, 4);

    let remaining = manager
        .list_backups()
        .expect("list remaining backups after prune");
    assert!(remaining.is_empty(), "keep=0 should remove all backups");
}

#[test]
fn list_backups_empty_dir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let backup_dir = dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).expect("create backup dir");

    let conn = Connection::open_in_memory().expect("open in-memory SQLite connection");
    let manager = BackupManager::new(&conn, &backup_dir);

    let backups = manager.list_backups().expect("list backups in empty dir");
    assert!(
        backups.is_empty(),
        "existing but empty dir should return empty vec"
    );
}

#[test]
fn export_sessions_json_empty_store() {
    let store = test_store();
    let dir = tempfile::tempdir().expect("create temp dir");
    let export_dir = dir.path().join("export");
    let manager = BackupManager::new(store.conn(), dir.path().join("backups"));

    let result = manager
        .export_sessions_json(&export_dir)
        .expect("export empty store as JSON")
        .expect("export should not be skipped without disk monitor");
    assert_eq!(result.sessions_exported, 0);
    assert_eq!(result.files_written, 0);
    assert!(
        export_dir.exists(),
        "output dir should be created even when empty"
    );

    let entries: Vec<_> = std::fs::read_dir(&export_dir)
        .expect("read export dir")
        .collect();
    assert!(entries.is_empty(), "no JSON files should be written");
}

#[test]
fn restore_from_corrupt_file_errors() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let corrupt_path = dir.path().join("corrupt.db");
    std::fs::write(&corrupt_path, b"this is not a sqlite database").expect("write corrupt file");

    if let Ok(c) = Connection::open(&corrupt_path) {
        let result = c.query_row("SELECT COUNT(*) FROM sessions", [], |row| {
            row.get::<_, u32>(0)
        });
        assert!(result.is_err(), "querying corrupt DB should fail");
    }
}

#[test]
fn validate_path_rejects_semicolons_in_filename() {
    let path = Path::new("/backups/data;rm -rf.db");
    assert!(
        validate_backup_path(path).is_err(),
        "semicolon in filename must be rejected"
    );
}

#[test]
fn validate_path_rejects_backticks_in_filename() {
    let path = Path::new("/backups/`whoami`.db");
    assert!(
        validate_backup_path(path).is_err(),
        "backticks in filename must be rejected"
    );
}

#[test]
fn validate_path_rejects_single_quotes_in_dir() {
    let path = Path::new("/tmp/bob's dir/backup.db");
    assert!(
        validate_backup_path(path).is_err(),
        "single quotes in directory must be rejected"
    );
}

#[test]
fn validate_path_accepts_normal_nested() {
    let path = Path::new("/var/lib/aletheia/backups/sessions_20260309T120000.db");
    assert!(
        validate_backup_path(path).is_ok(),
        "normal nested path with underscores and digits must be accepted"
    );
}
