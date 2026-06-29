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

use aletheia_sessions_migrate::{run_migration, run_verification};
use fjall::{KeyspaceCreateOptions, PersistMode, Readable};
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

#[test]
fn legacy_only_table_fields_are_preserved_in_sidecar_and_verified() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-sidecar",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        conn.execute(
            "INSERT INTO usage
             (id, session_id, turn_seq, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, model, created_at)
             VALUES (42, 'ses-sidecar', 1, 10, 5, 1, 2, 'test-model', '2026-04-01T00:10:00.000Z')",
            [],
        )
        .expect("insert usage with legacy-only fields");
        conn.execute(
            "INSERT INTO distillations
             (id, session_id, messages_before, messages_after, tokens_before, tokens_after, facts_extracted, model, created_at)
             VALUES (7, 'ses-sidecar', 4, 1, 100, 25, 3, 'test-model', '2026-04-01T00:20:00.000Z')",
            [],
        )
        .expect("insert distillation with legacy-only fields");
        conn.execute(
            "INSERT INTO blackboard (id, key, value, author_nous_id, ttl_seconds, created_at)
             VALUES ('bb-legacy-id', 'goal', 'finish migration', 'syn', 60, '2026-04-01T00:30:00.000Z')",
            [],
        )
        .expect("insert blackboard with legacy-only id");
    }

    let report = run_migration(&src, &dest, false).expect("migration succeeds");
    assert_eq!(report.legacy_sidecar_entries_preserved, 5);

    {
        let db = FjallDb::open_existing(&dest).expect("reopen fjall");
        assert_sidecar_value(&db, "usage:ses-sidecar:00000000000000000001:id", b"42");
        assert_sidecar_value(
            &db,
            "usage:ses-sidecar:00000000000000000001:created_at",
            b"2026-04-01T00:10:00.000Z",
        );
        assert_sidecar_value(
            &db,
            "distillations:ses-sidecar:00000000000000000001:id",
            b"7",
        );
        assert_sidecar_value(
            &db,
            "distillations:ses-sidecar:00000000000000000001:facts_extracted",
            b"3",
        );
        assert_sidecar_value(&db, "blackboard:goal:id", b"bb-legacy-id");
    }

    {
        let db = FjallDb::open_existing(&dest).expect("reopen fjall for tamper");
        let sidecar = db
            .db
            .keyspace("migration_legacy", KeyspaceCreateOptions::default)
            .expect("legacy partition");
        let mut tx = db.db.write_tx();
        tx.remove(
            &sidecar,
            "usage:ses-sidecar:00000000000000000001:created_at",
        );
        tx.commit().expect("commit sidecar tamper");
        db.db.persist(PersistMode::SyncAll).expect("persist tamper");
    }

    let verification = run_verification(&src, &dest, 4).expect("verification runs");
    assert!(
        !verification.ok(),
        "missing legacy-only sidecar field must fail verification"
    );
    assert!(
        verification
            .mismatches
            .iter()
            .any(|mismatch| mismatch.contains("partition migration_legacy")),
        "expected migration_legacy mismatch, got: {:?}",
        verification.mismatches
    );
}

fn assert_sidecar_value(db: &FjallDb, key: &str, expected: &[u8]) {
    let sidecar = db
        .db
        .keyspace("migration_legacy", KeyspaceCreateOptions::default)
        .expect("legacy partition");
    let snap = db.db.read_tx();
    let value = snap
        .get(&sidecar, key)
        .expect("read legacy sidecar")
        .expect("legacy sidecar value present");
    assert_eq!(&*value, expected, "sidecar value mismatch for {key}");
}
