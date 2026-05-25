//! Sessions with non-default `thinking_enabled`, `working_state`, or
//! `distillation_priming` columns must not silently lose those values.
//! The migrator routes them to the `migration_legacy` partition; the
//! data is recoverable post-migration.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "integration tests use direct assertions over fixture setup"
)]

mod common;

use aletheia_sessions_migrate::run_migration;
use fjall::{KeyspaceCreateOptions, Readable};
use koina::fjall::FjallDb;
use rusqlite::Connection;

#[test]
fn legacy_extras_are_preserved_in_sidecar() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-thinky",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        // Set non-default extras on this session.
        conn.execute(
            "UPDATE sessions SET thinking_enabled = 1, thinking_budget = 25000,
             working_state = 'partial-state-blob', distillation_priming = 'priming-blob'
             WHERE id = 'ses-thinky'",
            [],
        )
        .expect("update extras");
    }

    let report = run_migration(&src, &dest, false).expect("migration succeeds");
    assert_eq!(report.legacy_extras_preserved, 1);

    // Inspect the migration_legacy partition directly.
    let db = FjallDb::open_existing(&dest).expect("reopen fjall");
    let sidecar = db
        .db
        .keyspace("migration_legacy", KeyspaceCreateOptions::default)
        .expect("legacy partition");
    let snap = db.db.read_tx();

    let bundle = snap
        .get(&sidecar, b"ses-thinky:bundle".as_slice())
        .expect("read bundle")
        .expect("bundle present");
    let bundle_json: serde_json::Value = serde_json::from_slice(&bundle).expect("bundle is JSON");
    assert_eq!(bundle_json["thinking_enabled"], 1);
    assert_eq!(bundle_json["thinking_budget"], 25000);
    assert_eq!(bundle_json["working_state"], "partial-state-blob");
    assert_eq!(bundle_json["distillation_priming"], "priming-blob");

    // Per-field grep keys must also exist.
    let working_state = snap
        .get(&sidecar, b"ses-thinky:working_state".as_slice())
        .expect("read working_state")
        .expect("working_state present");
    assert_eq!(&*working_state, b"partial-state-blob".as_slice());
}

#[test]
fn default_extras_are_not_written_to_sidecar() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-default",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
    }

    let report = run_migration(&src, &dest, false).expect("migration succeeds");
    assert_eq!(report.legacy_extras_preserved, 0);

    let db = FjallDb::open_existing(&dest).expect("reopen fjall");
    let sidecar = db
        .db
        .keyspace("migration_legacy", KeyspaceCreateOptions::default)
        .expect("legacy partition");
    let snap = db.db.read_tx();

    let bundle = snap
        .get(&sidecar, b"ses-default:bundle".as_slice())
        .expect("read bundle");
    assert!(
        bundle.is_none(),
        "default-only session must not produce a sidecar bundle"
    );
}
