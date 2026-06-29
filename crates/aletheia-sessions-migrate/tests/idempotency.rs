//! Running the migrator twice on the same
//! source-and-dest must not corrupt data. Without replacement, the second
//! run errors loudly. With replacement, the second run replays the same
//! data over the existing store.

#![expect(
    clippy::expect_used,
    reason = "integration tests use direct assertions over fixture setup"
)]

mod common;

use aletheia_sessions_migrate::run_migration;
use graphe::store::SessionStore;
use rusqlite::Connection;

#[test]
fn second_run_without_replacement_errors_loudly() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open SQLite for seeding");
        common::insert_session(
            &conn,
            "ses-x",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-x",
            1,
            "user",
            "hello",
            false,
            1,
            "2026-04-01T00:30:00.000Z",
        );
    }

    // First run succeeds.
    let r = run_migration(&src, &dest, false).expect("first run");
    assert_eq!(r.counts.sessions, 1);

    // Second run without replacement fails with a clear message.
    let err = run_migration(&src, &dest, false).expect_err("second run errors");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("non-empty"),
        "expected 'non-empty' guard message; got: {msg}"
    );
    assert!(
        msg.contains("--replace-existing"),
        "expected replacement hint in: {msg}"
    );
    assert!(
        msg.contains("--i-understand-this-replaces-destination"),
        "expected confirmation hint in: {msg}"
    );
}

#[test]
fn second_run_with_replacement_overwrites_idempotently() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open SQLite for seeding");
        common::insert_session(
            &conn,
            "ses-y",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        for s in 1..=5 {
            common::insert_message(
                &conn,
                "ses-y",
                s,
                "user",
                &format!("message-{s}"),
                false,
                1,
                "2026-04-01T00:30:00.000Z",
            );
        }
    }

    let _ = run_migration(&src, &dest, false).expect("first run");
    let _ = run_migration(&src, &dest, true).expect("second run with replacement");

    // After the double-run, the data must still be exactly one session
    // with five messages — overwrite semantics, not duplication.
    let store = SessionStore::open(&dest).expect("open SessionStore");
    let sessions = store.list_sessions(None).expect("list");
    assert_eq!(sessions.len(), 1, "session count must stay at 1");
    let history = store.get_history("ses-y", None).expect("history");
    assert_eq!(history.len(), 5, "message count must stay at 5");
    let bodies: Vec<&str> = history.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(
        bodies,
        vec![
            "message-1",
            "message-2",
            "message-3",
            "message-4",
            "message-5"
        ]
    );
}
