//! Handing the migrator a `SQLite` file with
//! mismatched schema must error with a specific message
//! identifying what's wrong.

#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "integration tests use direct assertions over fixture setup"
)]

use rusqlite::Connection;

use crate::{common, run_migration};

#[test]
fn rejects_wrong_user_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    {
        let conn = Connection::open(&src).expect("create source");
        // Apply v32 DDL but stamp user_version to 31.
        conn.execute_batch(common::SCHEMA_SQL).expect("ddl");
        conn.execute_batch("PRAGMA user_version = 31").unwrap();
    }

    let err = run_migration(&src, &dest, false).expect_err("rejects v31");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("user_version"),
        "expected 'user_version' in error; got: {msg}"
    );
    assert!(msg.contains("31"), "expected '31' in error; got: {msg}");
    assert!(
        msg.contains("32") || msg.contains("REQUIRED_USER_VERSION"),
        "expected reference to required version 32; got: {msg}"
    );
}

#[test]
fn rejects_missing_table() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    {
        let conn = Connection::open(&src).expect("create source");
        // v32 stamp, but only the `sessions` table exists.
        conn.execute_batch(
            "PRAGMA user_version = 32;
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY, nous_id TEXT NOT NULL, session_key TEXT NOT NULL,
                parent_session_id TEXT, status TEXT NOT NULL DEFAULT 'active', model TEXT,
                token_count_estimate INTEGER, message_count INTEGER,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                last_input_tokens INTEGER, bootstrap_hash TEXT, distillation_count INTEGER,
                thinking_enabled INTEGER, thinking_budget INTEGER, thread_id TEXT,
                transport TEXT, working_state TEXT, session_type TEXT, last_distilled_at TEXT,
                computed_context_tokens INTEGER, distillation_priming TEXT, display_name TEXT
             );",
        )
        .expect("partial ddl");
    }

    let err = run_migration(&src, &dest, false).expect_err("rejects missing tables");
    let msg = format!("{err:#}");
    // The migrator names the first missing table it finds.
    assert!(
        msg.contains("messages") || msg.contains("usage") || msg.contains("blackboard"),
        "expected missing-table name in error; got: {msg}"
    );
}

#[test]
fn rejects_missing_promised_legacy_extra_column() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    {
        let conn = Connection::open(&src).expect("create source");
        let ddl = common::SCHEMA_SQL.replace("    thinking_enabled INTEGER DEFAULT 0,\n", "");
        conn.execute_batch(&ddl).expect("ddl without legacy extra");
    }

    let err = run_migration(&src, &dest, false).expect_err("rejects missing legacy extra");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("sessions"),
        "expected sessions table in error; got: {msg}"
    );
    assert!(
        msg.contains("thinking_enabled"),
        "expected missing legacy extra column in error; got: {msg}"
    );
}
