//! Orphan messages (whose parent session row was already deleted in
//! the legacy `SQLite` DB) must be preserved under a synthesised
//! `orphan-recovery` session, not silently dropped.

#![expect(
    clippy::expect_used,
    reason = "integration tests use direct assertions over fixture setup"
)]

use graphe::store::SessionStore;
use rusqlite::Connection;

use crate::{common, run_migration};

#[test]
fn orphan_messages_are_preserved_under_synthesised_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        // Real session.
        common::insert_session(
            &conn,
            "ses-real",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-real",
            1,
            "user",
            "real session message",
            false,
            1,
            "2026-04-01T00:30:00.000Z",
        );
        // Orphan messages — no parent session row.
        for s in 1..=3 {
            common::insert_message(
                &conn,
                "ses-orphan",
                s,
                "user",
                &format!("orphan-{s}"),
                false,
                1,
                "2026-04-01T00:30:00.000Z",
            );
        }
    }

    let report = run_migration(&src, &dest, false).expect("migration succeeds");
    assert_eq!(
        report.orphan_messages_recovered, 3,
        "expected 3 orphan messages recovered"
    );
    assert_eq!(
        report.orphan_sessions_synthesised, 1,
        "expected 1 synthesised orphan-recovery session"
    );

    let store = SessionStore::open(&dest).expect("open SessionStore");
    let sessions = store.list_sessions(None).expect("list sessions");
    assert_eq!(sessions.len(), 2, "real + orphan-recovery session");

    // The synthesised session has nous_id = "orphan-recovery".
    let orphan = sessions
        .iter()
        .find(|s| s.nous_id == "orphan-recovery")
        .expect("orphan-recovery session exists");
    assert_eq!(orphan.id, "ses-orphan");
    let orphan_history = store.get_history("ses-orphan", None).expect("history");
    assert_eq!(orphan_history.len(), 3);
    let bodies: Vec<&str> = orphan_history.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(bodies, vec!["orphan-1", "orphan-2", "orphan-3"]);
}
