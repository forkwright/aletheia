//! Tiny synthetic source — 1 session, 5
//! messages, 1 distillation. After migrating, the resulting fjall
//! directory must be openable via graphe's `SessionStore` and every row
//! must be queryable through the public API.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "integration tests use direct assertions over fixture setup"
)]

use graphe::store::SessionStore;
use rusqlite::Connection;

use crate::{common, run_migration};

#[test]
fn tiny_synthetic_round_trip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);

    let conn = Connection::open(&src).expect("open writable for fixture seed");
    common::insert_session(
        &conn,
        "ses-1",
        "syn",
        "main",
        "active",
        Some("claude-opus-4-7"),
        "2026-01-01T00:00:00.000Z",
        "2026-01-01T01:00:00.000Z",
    );
    for (seq, role, body) in [
        (1, "user", "what is the meaning of fjall?"),
        (2, "assistant", "an LSM tree, mostly."),
        (3, "user", "explain more"),
        (4, "assistant", "log-structured merge tree, pure-Rust."),
        (5, "system", "session check-in"),
    ] {
        common::insert_message(
            &conn,
            "ses-1",
            seq,
            role,
            body,
            false,
            i64::try_from(body.len()).unwrap_or(0) / 4,
            "2026-01-01T00:30:00.000Z",
        );
    }
    common::insert_distillation(
        &conn,
        "ses-1",
        5,
        1,
        500,
        100,
        Some("haiku"),
        "2026-01-01T01:00:00.000Z",
    );
    drop(conn);

    let report = run_migration(&src, &dest, false).expect("migration succeeds");
    assert_eq!(report.counts.sessions, 1);
    assert_eq!(report.counts.messages, 5);
    assert_eq!(report.counts.distillations, 1);

    // Open via the runtime API and read back.
    let store = SessionStore::open(&dest).expect("open fjall via SessionStore");
    let sessions = store.list_sessions(None).expect("list sessions");
    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!(s.id, "ses-1");
    assert_eq!(s.nous_id, "syn");
    assert_eq!(s.session_key, "main");
    assert_eq!(s.model.as_deref(), Some("claude-opus-4-7"));

    // Messages — get_history filters distilled rows; ours are not distilled.
    let history = store
        .get_history("ses-1", None)
        .expect("get_history succeeds");
    assert_eq!(history.len(), 5);
    let bodies: Vec<&str> = history.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(
        bodies,
        vec![
            "what is the meaning of fjall?",
            "an LSM tree, mostly.",
            "explain more",
            "log-structured merge tree, pure-Rust.",
            "session check-in",
        ]
    );

    // find_session_by_id round-trip.
    let found = store
        .find_session_by_id("ses-1")
        .expect("find_session_by_id")
        .expect("session present");
    assert_eq!(found.id, "ses-1");
}
