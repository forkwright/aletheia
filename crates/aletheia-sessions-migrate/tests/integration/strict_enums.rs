//! Legacy enum columns must fail loudly when a row carries a value outside
//! the v32 schema contract.

#![expect(
    clippy::expect_used,
    reason = "integration tests use direct assertions over fixture setup"
)]

use rusqlite::Connection;

use crate::{common, run_migration};

#[test]
fn unknown_session_status_reports_row_context() {
    let message = migration_error_after("UPDATE sessions SET status = 'paused' WHERE id = 'ses-a'");

    assert_enum_context(&message, "sessions.status", "ses-a", "paused");
}

#[test]
fn unknown_session_type_reports_row_context() {
    let message = migration_error_after(
        "UPDATE sessions SET session_type = 'conversation' WHERE id = 'ses-a'",
    );

    assert_enum_context(&message, "sessions.session_type", "ses-a", "conversation");
}

#[test]
fn unknown_message_role_reports_row_context() {
    let message = migration_error_after("UPDATE messages SET role = 'developer' WHERE id = 1");

    assert_enum_context(&message, "messages.role", "1", "developer");
}

fn migration_error_after(update_sql: &str) -> String {
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
        conn.execute(update_sql, []).expect("apply enum drift");
    }

    let err = run_migration(&src, &dest, false).expect_err("unknown enum must fail");
    format!("{err:#}")
}

fn assert_enum_context(message: &str, column: &str, row_id: &str, raw_value: &str) {
    assert!(
        message.contains("unknown legacy enum value"),
        "expected enum error, got: {message}"
    );
    assert!(
        message.contains(column),
        "expected column {column}, got: {message}"
    );
    assert!(
        message.contains(row_id),
        "expected row id {row_id}, got: {message}"
    );
    assert!(
        message.contains(raw_value),
        "expected raw value {raw_value}, got: {message}"
    );
}
