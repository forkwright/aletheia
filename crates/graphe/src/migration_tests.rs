#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

fn fresh_conn() -> Connection {
    Connection::open_in_memory().expect("in-memory SQLite connection should always open")
}

/// Latest schema version known to the test build.
///
/// WHY: hardcoding `31` (or any value) here breaks every time a new
/// migration is added. Reading from `MIGRATIONS` keeps the test in
/// lockstep with the migration list.
fn latest_version() -> u32 {
    MIGRATIONS
        .last()
        .map(|m| m.version)
        .expect("MIGRATIONS slice is non-empty")
}

#[test]
fn fresh_database_gets_all_migrations() {
    let conn = fresh_conn();
    let result = run_migrations(&conn).expect("migrations should apply to fresh DB");

    assert!(
        result.was_fresh,
        "fresh database should be reported as fresh"
    );
    let expected: Vec<u32> = (1..=latest_version()).collect();
    assert_eq!(
        result.applied, expected,
        "every migration should be applied to a fresh database"
    );
    assert_eq!(
        result.current_version,
        latest_version(),
        "current version should match latest migration after run"
    );
}

#[test]
fn already_migrated_skips_applied() {
    let conn = fresh_conn();
    run_migrations(&conn).expect("first migration run should succeed");

    let result = run_migrations(&conn).expect("migrations should apply to fresh DB");
    assert!(
        !result.was_fresh,
        "second run should not report the database as fresh"
    );
    assert!(
        result.applied.is_empty(),
        "second run should apply no migrations"
    );
    assert_eq!(
        result.current_version,
        latest_version(),
        "version should still match latest after idempotent run"
    );
}

#[test]
fn version_recorded_in_schema_version() {
    let conn = fresh_conn();
    run_migrations(&conn).expect("migrations should apply successfully");

    let (version, description): (u32, String) = conn
        .query_row(
            "SELECT version, description FROM schema_version WHERE version = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("schema_version row for version 1 should exist after migration");
    assert_eq!(version, 1, "version 1 should be recorded");
    assert!(!description.is_empty(), "description should be non-empty");
}

#[test]
fn dry_run_reports_pending_without_applying() {
    let conn = fresh_conn();
    // NOTE: Bootstrap table but don't apply migrations
    bootstrap_version_table(&conn).expect("bootstrap_version_table should succeed");

    let pending = check_migrations(&conn).unwrap_or_default();
    assert_eq!(
        pending.len(),
        MIGRATIONS.len(),
        "every migration should be pending on a fresh database"
    );
    assert_eq!(
        pending[0].version, 1,
        "first pending migration should be version 1"
    );

    let version = get_schema_version(&conn);
    assert_eq!(version, 0, "schema version should remain 0 after dry run");
}

#[test]
fn dry_run_empty_when_current() {
    let conn = fresh_conn();
    run_migrations(&conn).expect("migrations should apply successfully");

    let pending = check_migrations(&conn).unwrap_or_default();
    assert!(
        pending.is_empty(),
        "no migrations should be pending after full migration"
    );
}

#[test]
fn migration_order_enforced() {
    for window in MIGRATIONS.windows(2) {
        assert!(
            window[0].version < window[1].version,
            "migration {} must come before {}",
            window[0].version,
            window[1].version,
        );
    }
}

#[test]
fn tables_exist_after_migration() {
    let conn = fresh_conn();
    run_migrations(&conn).expect("migrations should apply successfully");

    for table in &[
        "sessions",
        "messages",
        "usage",
        "distillations",
        "agent_notes",
        "blackboard",
        "threads",
        "thread_summaries",
        "transport_bindings",
        "auth_sessions",
        "contact_requests",
        "approved_contacts",
        "tool_stats",
        "interaction_signals",
        "sub_agent_log",
        "cross_agent_messages",
        "routing_cache",
        "message_queue",
        "distillation_locks",
        "distillation_log",
        "reflection_log",
        "plans",
        "planning_projects",
        "planning_phases",
        "planning_requirements",
        "planning_checkpoints",
        "planning_research",
        "planning_messages",
        "planning_discussions",
        "planning_decisions",
        "planning_annotations",
        "planning_edit_history",
        "planning_spawn_records",
        "planning_turn_counts",
        "audit_log",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("table existence query should succeed");
        assert!(exists, "table {table} should exist after migration");
    }
}

#[test]
fn run_migrations_fresh_db_schema_version() {
    let conn = fresh_conn();
    let result = run_migrations(&conn).expect("migrations should apply to fresh DB");
    assert_eq!(
        result.current_version,
        latest_version(),
        "current_version should match latest after full migration"
    );
    let version = get_schema_version(&conn);
    assert_eq!(
        version,
        latest_version(),
        "get_schema_version should return latest after full migration"
    );
}

#[test]
fn run_migrations_idempotent() {
    let conn = fresh_conn();
    let first = run_migrations(&conn).expect("first migration run should succeed");
    let second = run_migrations(&conn).expect("second migration run should succeed idempotently");
    assert_eq!(
        first.current_version, second.current_version,
        "version should be the same across idempotent runs"
    );
    assert!(
        second.applied.is_empty(),
        "second run should apply no migrations"
    );
}

#[test]
fn check_migrations_reports_pending() {
    let conn = fresh_conn();
    let pending = check_migrations(&conn).unwrap_or_default();
    assert_eq!(
        pending.len(),
        MIGRATIONS.len(),
        "all migrations should be pending on a fresh database"
    );
    assert_eq!(
        pending[0].version, 1,
        "first pending migration should be version 1"
    );
}

#[test]
fn get_schema_version_fresh_db() {
    let conn = fresh_conn();
    bootstrap_version_table(&conn).expect("bootstrap_version_table should succeed on fresh DB");
    let version = get_schema_version(&conn);
    assert_eq!(version, 0, "schema version should be 0 on a fresh database");
}

#[test]
fn pragma_user_version_tracks_schema_version() {
    let conn = fresh_conn();
    run_migrations(&conn).expect("migrations should apply successfully");

    let pragma_version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("PRAGMA user_version should be readable after migration");
    assert_eq!(
        pragma_version,
        latest_version(),
        "PRAGMA user_version should match latest migration version"
    );
}

#[test]
fn pragma_user_version_zero_before_migration() {
    let conn = fresh_conn();

    let pragma_version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("check_migrations should return all pending on fresh DB");
    assert_eq!(
        pragma_version, 0,
        "PRAGMA user_version should be 0 on a fresh database"
    );
}

#[test]
fn backward_compat_existing_v1_database() {
    let conn = fresh_conn();

    // NOTE: Simulate an older database: schema_version without description column
    conn.execute_batch(
        "CREATE TABLE schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        )",
    )
    .expect("creating legacy schema_version table should succeed");
    conn.execute_batch(DDL).unwrap_or_default();
    conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
        .expect("inserting v1 row should succeed");

    let result = run_migrations(&conn).expect("migrations should apply to v1 DB");
    assert!(!result.was_fresh, "upgraded database should not be fresh");
    let expected: Vec<u32> = (2..=latest_version()).collect();
    assert_eq!(
        result.applied, expected,
        "migrations 2..=latest should be applied to a v1 database"
    );
    assert_eq!(
        result.current_version,
        latest_version(),
        "current version should match latest after upgrade"
    );

    assert!(
        has_description_column(&conn),
        "description column should be present after upgrade"
    );
    assert!(
        has_checksum_column(&conn),
        "checksum column should be present after upgrade"
    );
}

#[test]
fn checksum_stored_for_new_migrations() {
    let conn = fresh_conn();
    run_migrations(&conn).expect("migrations should apply successfully");

    for migration in MIGRATIONS {
        let stored: String = conn
            .query_row(
                "SELECT checksum FROM schema_version WHERE version = ?1",
                rusqlite::params![migration.version],
                |row| row.get(0),
            )
            .expect("sqlite_master query should succeed for table existence check");
        assert!(
            !stored.is_empty(),
            "checksum for migration v{} should be non-empty",
            migration.version
        );
        let expected = compute_checksum(migration.up);
        assert_eq!(
            stored, expected,
            "stored checksum for v{} should match computed checksum",
            migration.version
        );
    }
}

#[test]
fn verify_checksums_passes_on_intact_db() {
    let conn = fresh_conn();
    run_migrations(&conn).expect("migrations should apply successfully");

    verify_migration_checksums(&conn, get_schema_version(&conn)).unwrap_or_default();
}

#[test]
fn verify_checksums_detects_tampered_checksum() {
    let conn = fresh_conn();
    run_migrations(&conn).expect("migrations should apply successfully");

    // Tamper with the stored checksum for v1.
    conn.execute(
        "UPDATE schema_version SET checksum = 'deadbeef' WHERE version = 1",
        [],
    )
    .expect("tampering with checksum should succeed");

    let err = verify_migration_checksums(&conn, get_schema_version(&conn))
        .expect_err("verification should fail when checksum is tampered");

    let err_str = err.to_string();
    assert!(
        err_str.contains("v1"),
        "error message should identify the offending migration version"
    );
    assert!(
        err_str.contains("deadbeef"),
        "error message should include the recorded (tampered) checksum"
    );
}

#[test]
fn verify_checksums_skips_empty_checksum_legacy_rows() {
    let conn = fresh_conn();
    // Simulate legacy rows: schema_version with empty checksum.
    bootstrap_version_table(&conn).expect("bootstrap should succeed");
    conn.execute_batch(DDL).unwrap_or_default();
    conn.execute(
        "INSERT INTO schema_version (version, description, checksum) VALUES (1, 'base', '')",
        [],
    )
    .expect("inserting legacy row should succeed");

    // Verification should skip the empty-checksum row without error.
    verify_migration_checksums(&conn, 1).unwrap_or_default();
}

#[test]
fn schema_too_new_returns_error() {
    let conn = fresh_conn();

    // Simulate a database with a newer schema version than the binary supports.
    bootstrap_version_table(&conn).expect("bootstrap_version_table should succeed");
    let future_version = MIGRATIONS.last().map_or(1, |m| m.version + 1);
    conn.execute(
        "INSERT INTO schema_version (version, description, checksum) VALUES (?1, 'future migration', '')",
        rusqlite::params![future_version],
    )
    .expect("creating legacy schema_version table should succeed");
    conn.pragma_update(None, "user_version", future_version)
        .expect("PRAGMA user_version should be readable on fresh DB");

    // Running migrations should fail with SchemaTooNew error.
    let err = run_migrations(&conn)
        .expect_err("should fail when database schema is newer than binary supports");
    let err_str = err.to_string();
    assert!(
        err_str.contains("newer than this binary supports"),
        "error message should indicate schema is too new: {err_str}"
    );
    assert!(
        err_str.contains(&future_version.to_string()),
        "error message should include the current version: {err_str}"
    );
}
