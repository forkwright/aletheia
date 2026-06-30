//! N sessions × M messages, migrate, then
//! iterate via `SessionStore` and confirm content is bit-identical.

#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "integration tests use direct assertions over fixture setup"
)]

use std::collections::BTreeMap;

use graphe::store::SessionStore;
use rusqlite::Connection;

use crate::verify::run_verification;
use crate::{common, run_migration};

const N_SESSIONS: usize = 7;
const M_MESSAGES: usize = 23;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "round-trip fixture asserts full migration"
)]
fn round_trip_n_sessions_m_messages() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    let conn = Connection::open(&src).expect("open SQLite for seeding");

    let mut expected: BTreeMap<String, Vec<(i64, String, String)>> = BTreeMap::new();

    for s in 0..N_SESSIONS {
        let sid = format!("ses-{s:03}");
        let nous = format!("nous-{}", s % 3);
        let key = format!("key-{s}");
        common::insert_session(
            &conn,
            &sid,
            &nous,
            &key,
            "active",
            Some("test-model"),
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        let mut session_msgs = Vec::new();
        for m in 0..M_MESSAGES {
            let seq = i64::try_from(m + 1).unwrap();
            let role = if m % 2 == 0 { "user" } else { "assistant" };
            let body = format!("session-{s}-msg-{m}-content");
            common::insert_message(
                &conn,
                &sid,
                seq,
                role,
                &body,
                false,
                i64::try_from(body.len()).unwrap_or(0) / 4,
                "2026-04-01T00:30:00.000Z",
            );
            session_msgs.push((seq, role.to_owned(), body));
        }
        expected.insert(sid, session_msgs);
    }

    // Add a distillation per session.
    for s in 0..N_SESSIONS {
        let sid = format!("ses-{s:03}");
        common::insert_distillation(
            &conn,
            &sid,
            i64::try_from(M_MESSAGES).unwrap(),
            1,
            1000,
            200,
            Some("haiku"),
            "2026-04-01T01:00:00.000Z",
        );
    }

    // Add usage rows.
    for s in 0..N_SESSIONS {
        let sid = format!("ses-{s:03}");
        for m in 0..M_MESSAGES {
            common::insert_usage(
                &conn,
                &sid,
                i64::try_from(m + 1).unwrap(),
                100,
                50,
                10,
                5,
                Some("test-model"),
            );
        }
    }

    // Add notes for half the sessions.
    for s in 0..N_SESSIONS {
        if s % 2 == 1 {
            continue;
        }
        let sid = format!("ses-{s:03}");
        common::insert_note(
            &conn,
            &sid,
            "syn",
            "context",
            "annotation body",
            "2026-04-01T01:00:00.000Z",
        );
    }
    drop(conn);

    let report = run_migration(&src, &dest, false).expect("migration succeeds");
    assert_eq!(report.counts.sessions, N_SESSIONS);
    assert_eq!(report.counts.messages, N_SESSIONS * M_MESSAGES);
    assert_eq!(report.counts.distillations, N_SESSIONS);
    assert_eq!(report.counts.usage, N_SESSIONS * M_MESSAGES);
    assert_eq!(report.counts.notes, N_SESSIONS.div_ceil(2));

    // Verify via the public API.
    {
        let store = SessionStore::open(&dest).expect("open SessionStore");
        let sessions = store.list_sessions(None).expect("list_sessions");
        assert_eq!(sessions.len(), N_SESSIONS);

        for (sid, want_msgs) in &expected {
            let history = store
                .get_history(sid, None)
                .expect("get_history per session");
            assert_eq!(
                history.len(),
                want_msgs.len(),
                "message count mismatch for {sid}"
            );
            for (got, want) in history.iter().zip(want_msgs.iter()) {
                assert_eq!(got.seq, want.0, "seq mismatch in {sid}");
                assert_eq!(got.role.as_str(), want.1, "role mismatch in {sid}");
                assert_eq!(got.content, want.2, "content mismatch in {sid}");
            }
        }
    } // drop store; release fjall lock before verification reopens.

    // Run --verify against the same source/dest.
    let v = run_verification(&src, &dest, 8).expect("verification");
    assert!(v.ok(), "verification should pass: {v:?}");
}
