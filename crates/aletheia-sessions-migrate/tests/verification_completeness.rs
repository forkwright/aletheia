//! Verification must cover every migrated partition, not just message bodies.

#![expect(
    clippy::expect_used,
    reason = "integration tests use direct assertions over fixture setup"
)]

mod common;

use std::path::Path;

use aletheia_sessions_migrate::{run_migration, run_verification};
use fjall::{KeyspaceCreateOptions, PersistMode};
use koina::fjall::FjallDb;
use rusqlite::Connection;

#[test]
fn verification_fails_on_auxiliary_partition_hash_mismatch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    seed_source_with_auxiliary_rows(&src);
    run_migration(&src, &dest, false).expect("migration succeeds");

    put_raw(
        &dest,
        "usage",
        "ses-a:00000000000000000001",
        b"tampered usage row",
    );

    let report = run_verification(&src, &dest, 4).expect("verification runs");
    assert!(!report.ok(), "tampered usage row must fail verification");
    assert!(
        report
            .mismatches
            .iter()
            .any(|mismatch| mismatch.contains("partition usage hash mismatch")),
        "expected usage hash mismatch, got: {:?}",
        report.mismatches
    );
}

#[test]
fn verification_fails_on_stale_destination_partition_row() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    seed_source_with_auxiliary_rows(&src);
    run_migration(&src, &dest, false).expect("migration succeeds");

    put_raw(
        &dest,
        "blackboard",
        "stale-key",
        br#"{"key":"stale-key","value":"old","author_nous_id":"syn","ttl_seconds":60,"created_at":"2026-04-01T00:00:00.000Z","expires_at":null}"#,
    );

    let report = run_verification(&src, &dest, 4).expect("verification runs");
    assert!(!report.ok(), "stale blackboard row must fail verification");
    assert!(
        report
            .mismatches
            .iter()
            .any(|mismatch| { mismatch.contains("partition blackboard entry count mismatch") }),
        "expected blackboard count mismatch, got: {:?}",
        report.mismatches
    );
}

#[test]
fn malformed_legacy_extra_errors_instead_of_defaulting() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-a",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        conn.execute(
            "UPDATE sessions SET thinking_enabled = 'not-an-integer' WHERE id = 'ses-a'",
            [],
        )
        .expect("write malformed legacy extra");
    }

    let err = run_migration(&src, &dest, false).expect_err("malformed legacy extra must fail");
    let message = format!("{err:#}");
    assert!(
        message.contains("legacy session column 'thinking_enabled'"),
        "expected legacy-column error, got: {message}"
    );
    assert!(
        message.contains("ses-a"),
        "expected session id in legacy-column error, got: {message}"
    );
}

fn seed_source_with_auxiliary_rows(path: &Path) {
    common::build_empty_v32(path);
    let conn = Connection::open(path).expect("open source");
    common::insert_session(
        &conn,
        "ses-a",
        "syn",
        "main",
        "active",
        Some("test-model"),
        "2026-04-01T00:00:00.000Z",
        "2026-04-01T01:00:00.000Z",
    );
    common::insert_message(
        &conn,
        "ses-a",
        1,
        "user",
        "hello",
        false,
        1,
        "2026-04-01T00:30:00.000Z",
    );
    common::insert_usage(&conn, "ses-a", 1, 100, 50, 10, 5, Some("test-model"));
    common::insert_distillation(
        &conn,
        "ses-a",
        1,
        1,
        100,
        25,
        Some("test-model"),
        "2026-04-01T01:00:00.000Z",
    );
    common::insert_note(
        &conn,
        "ses-a",
        "syn",
        "context",
        "remember this",
        "2026-04-01T01:05:00.000Z",
    );
    conn.execute(
        "INSERT INTO blackboard (id, key, value, author_nous_id, ttl_seconds, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "bb-a",
            "current-key",
            "current value",
            "syn",
            60,
            "2026-04-01T01:10:00.000Z"
        ],
    )
    .expect("insert blackboard");
    conn.execute(
        "UPDATE sessions SET working_state = '{\"phase\":\"active\"}' WHERE id = 'ses-a'",
        [],
    )
    .expect("write legacy extra");
}

fn put_raw(dest: &Path, partition: &str, key: &str, value: &[u8]) {
    let db = FjallDb::open_existing(dest).expect("open fjall");
    let partition = db
        .db
        .keyspace(partition, KeyspaceCreateOptions::default)
        .expect("open partition");
    let mut tx = db.db.write_tx();
    tx.insert(&partition, key, value);
    tx.commit().expect("commit tamper");
    db.db.persist(PersistMode::SyncAll).expect("persist tamper");
}
