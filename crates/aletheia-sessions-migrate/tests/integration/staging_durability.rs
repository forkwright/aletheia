//! Staging-directory durability guarantees:
//!
//! - Migration writes to a staging directory and only publishes after all
//!   writes complete.
//! - A leftover staging directory from a failed run is refused unless the
//!   operator explicitly requests replacement.
//! - Replacement preserves the prior destination in a backup and
//!   restores it if the new migration is abandoned before publish.

#![expect(
    clippy::expect_used,
    reason = "integration tests use direct assertions over fixture setup"
)]

use std::fs;

use rusqlite::Connection;

use crate::migrate::stage_migration;
use crate::{common, run_migration};

#[test]
fn staging_dir_exists_before_publish_and_dest_does_not() {
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
    }

    let staged = stage_migration(&src, &dest, false).expect("stage migration");
    let staging = dest.with_extension("staging");

    assert!(
        staging.exists(),
        "staging directory must exist before publish"
    );
    assert!(
        !dest.exists(),
        "final destination must not exist before publish"
    );
    assert_eq!(staged.report().counts.sessions, 1);

    staged.publish().expect("publish");

    assert!(
        !staging.exists(),
        "staging directory must be removed on publish"
    );
    assert!(dest.exists(), "final destination must exist after publish");
}

#[test]
fn leftover_staging_refused_without_replacement() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");
    let staging = dest.with_extension("staging");

    common::build_empty_v32(&src);
    fs::create_dir_all(&staging).expect("create leftover staging dir");

    let err = run_migration(&src, &dest, false).expect_err("should refuse incomplete store");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("incomplete"),
        "expected 'incomplete' in error; got: {msg}"
    );
    assert!(
        msg.contains("--replace-existing"),
        "expected replacement hint in error; got: {msg}"
    );
    assert!(
        msg.contains("--i-understand-this-replaces-destination"),
        "expected confirmation hint in error; got: {msg}"
    );
}

#[test]
fn leftover_staging_removed_with_replacement() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");
    let staging = dest.with_extension("staging");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-b",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-b",
            1,
            "user",
            "hello",
            false,
            1,
            "2026-04-01T00:30:00.000Z",
        );
    }

    fs::create_dir_all(&staging).expect("create leftover staging dir");

    let report = run_migration(&src, &dest, true).expect("migration with replacement");
    assert_eq!(report.counts.sessions, 1);
    assert!(dest.exists(), "destination must exist after migration");
    assert!(
        !staging.exists(),
        "leftover staging directory must be removed"
    );
}

#[test]
fn replacement_restores_backup_when_staged_migration_dropped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-v1",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-v1",
            1,
            "user",
            "v1",
            false,
            1,
            "2026-04-01T00:30:00.000Z",
        );
    }

    run_migration(&src, &dest, false).expect("first migration");

    // Add a second session to the source so the staged migration would
    // produce different data.
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-v2",
            "syn",
            "second",
            "active",
            None,
            "2026-04-02T00:00:00.000Z",
            "2026-04-02T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-v2",
            1,
            "user",
            "v2",
            false,
            1,
            "2026-04-02T00:30:00.000Z",
        );
    }

    {
        let staged = stage_migration(&src, &dest, true).expect("stage with replacement");
        assert_eq!(staged.report().counts.sessions, 2);
        // Drop without publishing.
    }

    // The prior destination must be restored.
    assert!(
        dest.exists(),
        "destination must be restored after abandoned stage"
    );
    let store = graphe::store::SessionStore::open(&dest).expect("open restored store");
    let sessions = store.list_sessions(None).expect("list sessions");
    assert_eq!(
        sessions.len(),
        1,
        "restored store must contain only the original session"
    );
    assert_eq!(sessions.first().expect("one session").id, "ses-v1");
}

#[test]
fn replacement_staging_write_failure_preserves_existing_dest_and_prior_backup() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");
    let staging = dest.with_extension("staging");
    let prior_backup = dest.with_extension("backup");
    let prior_backup_sentinel = prior_backup.join("sentinel");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-v1",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-v1",
            1,
            "user",
            "v1",
            false,
            1,
            "2026-04-01T00:30:00.000Z",
        );
    }

    run_migration(&src, &dest, false).expect("first migration");
    fs::create_dir_all(&prior_backup).expect("create prior backup");
    fs::write(&prior_backup_sentinel, b"operator backup").expect("write sentinel");

    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-bad",
            "syn",
            "bad",
            "active",
            None,
            "2026-04-02T00:00:00.000Z",
            "2026-04-02T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-bad",
            -1,
            "user",
            "negative sequence cannot be written to fjall",
            false,
            1,
            "2026-04-02T00:30:00.000Z",
        );
    }

    let err = run_migration(&src, &dest, true).expect_err("replacement must fail");
    let message = format!("{err:#}");
    assert!(
        message.contains("message.seq"),
        "expected numeric range error, got: {message}"
    );

    let store = graphe::store::SessionStore::open(&dest).expect("open original store");
    let sessions = store.list_sessions(None).expect("list sessions");
    assert_eq!(sessions.len(), 1, "original destination must remain live");
    assert_eq!(sessions.first().expect("one session").id, "ses-v1");
    assert!(
        prior_backup_sentinel.exists(),
        "operator backup directory must not be removed"
    );
    assert!(
        !staging.exists(),
        "failed staging write should not leave a staging directory"
    );
}

#[test]
fn replacement_publishes_new_data() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("source.db");
    let dest = tmp.path().join("dest.fjall");

    common::build_empty_v32(&src);
    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-v1",
            "syn",
            "main",
            "active",
            None,
            "2026-04-01T00:00:00.000Z",
            "2026-04-01T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-v1",
            1,
            "user",
            "v1",
            false,
            1,
            "2026-04-01T00:30:00.000Z",
        );
    }

    run_migration(&src, &dest, false).expect("first migration");

    {
        let conn = Connection::open(&src).expect("open source");
        common::insert_session(
            &conn,
            "ses-v2",
            "syn",
            "second",
            "active",
            None,
            "2026-04-02T00:00:00.000Z",
            "2026-04-02T01:00:00.000Z",
        );
        common::insert_message(
            &conn,
            "ses-v2",
            1,
            "user",
            "v2",
            false,
            1,
            "2026-04-02T00:30:00.000Z",
        );
    }

    run_migration(&src, &dest, true).expect("replacement");

    let store = graphe::store::SessionStore::open(&dest).expect("open store");
    let sessions = store.list_sessions(None).expect("list sessions");
    assert_eq!(sessions.len(), 2, "replacement must publish new data");
}
