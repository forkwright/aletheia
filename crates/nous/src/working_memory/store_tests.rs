//! Tests for [`FjallWorkingCheckpointStore`].

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]

use koina::ulid::Ulid;
use organon::types::WorkingCheckpointStore;

use super::FjallWorkingCheckpointStore;

/// Deterministic, strictly-increasing ULID for a given test iteration.
///
/// WHY: `Ulid::new()` mints from wall-clock milliseconds plus random tail
/// bits, so two calls in the same millisecond (as in a tight test loop) are
/// not guaranteed to sort in call order. `Ulid`'s encoding preserves numeric
/// ordering of the raw value, so a small monotonic integer is sufficient and
/// removes the flake.
fn test_turn_id(i: u64) -> Ulid {
    Ulid::from_u128(u128::from(i))
}

#[test]
fn write_and_read_latest_roundtrip() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    store
        .write_checkpoint("session-1", test_turn_id(1), 1, "first checkpoint")
        .expect("write checkpoint");

    let latest = store
        .read_latest("session-1")
        .expect("read latest")
        .expect("checkpoint exists");
    assert_eq!(latest.session_id, "session-1");
    assert_eq!(latest.turn_number, 1);
    assert_eq!(latest.content, "first checkpoint");
}

#[test]
fn read_latest_returns_most_recent() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    store
        .write_checkpoint("session-1", test_turn_id(1), 1, "first")
        .expect("write first");
    store
        .write_checkpoint("session-1", test_turn_id(2), 2, "second")
        .expect("write second");
    store
        .write_checkpoint("session-1", test_turn_id(3), 3, "third")
        .expect("write third");

    let latest = store
        .read_latest("session-1")
        .expect("read latest")
        .expect("checkpoint exists");
    assert_eq!(latest.turn_number, 3);
    assert_eq!(latest.content, "third");
}

#[test]
fn read_latest_for_missing_session_returns_none() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    let result = store.read_latest("no-such-session").expect("read succeeds");
    assert!(result.is_none());
}

#[test]
fn read_recent_returns_newest_first() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    for i in 1..=5 {
        store
            .write_checkpoint("session-1", test_turn_id(i), i, &format!("checkpoint-{i}"))
            .expect("write checkpoint");
    }

    let recent = store.read_recent("session-1", 3).expect("read recent");
    assert_eq!(recent.len(), 3);
    assert_eq!(recent[0].turn_number, 5);
    assert_eq!(recent[1].turn_number, 4);
    assert_eq!(recent[2].turn_number, 3);
}

#[test]
fn sessions_are_isolated() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    store
        .write_checkpoint("session-a", test_turn_id(1), 1, "a-content")
        .expect("write a");
    store
        .write_checkpoint("session-b", test_turn_id(1), 1, "b-content")
        .expect("write b");

    let a = store
        .read_latest("session-a")
        .expect("read a")
        .expect("a exists");
    let b = store
        .read_latest("session-b")
        .expect("read b")
        .expect("b exists");

    assert_eq!(a.content, "a-content");
    assert_eq!(b.content, "b-content");
}

#[test]
fn read_recent_returns_empty_for_missing_session() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    let recent = store
        .read_recent("no-such-session", 5)
        .expect("read recent succeeds");
    assert!(
        recent.is_empty(),
        "read_recent for missing session should return empty vec"
    );
}

#[test]
fn read_recent_respects_limit() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    for i in 1..=10 {
        store
            .write_checkpoint("session-1", test_turn_id(i), i, &format!("checkpoint-{i}"))
            .expect("write checkpoint");
    }

    let recent = store.read_recent("session-1", 2).expect("read recent");
    assert_eq!(recent.len(), 2, "limit=2 should return exactly 2 entries");
    assert_eq!(recent[0].turn_number, 10, "first entry should be newest");
    assert_eq!(
        recent[1].turn_number, 9,
        "second entry should be second newest"
    );
}

#[test]
fn overwrite_checkpoint_same_turn_updates_content() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    store
        .write_checkpoint("session-1", test_turn_id(1), 1, "original")
        .expect("write original");
    store
        .write_checkpoint("session-1", test_turn_id(1), 1, "updated")
        .expect("write update");

    let latest = store
        .read_latest("session-1")
        .expect("read latest")
        .expect("checkpoint exists");
    assert_eq!(
        latest.content, "updated",
        "same-turn overwrite should update content"
    );
}

#[test]
fn checkpoint_survives_store_reopen_at_same_path() {
    // WHY(#4588): proves the durability half of the acceptance criteria —
    // `open_in_memory` above is ephemeral by design and cannot catch a
    // regression where a checkpoint only survives within one process
    // lifetime. This opens against a real on-disk path, drops the store
    // (simulating process exit), then reopens fresh at the same path
    // (simulating restart) and reads back.
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let store = FjallWorkingCheckpointStore::open(dir.path()).expect("open store");
        store
            .write_checkpoint("session-1", test_turn_id(1), 1, "before restart")
            .expect("write checkpoint");
    } // store dropped here — nothing kept alive across the "restart"

    let reopened = FjallWorkingCheckpointStore::open(dir.path()).expect("reopen store");
    let latest = reopened
        .read_latest("session-1")
        .expect("read latest")
        .expect("checkpoint survived reopen");
    assert_eq!(latest.turn_number, 1);
    assert_eq!(latest.content, "before restart");
}

#[test]
fn write_checkpoint_prunes_old_entries() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    for i in 1..=25 {
        store
            .write_checkpoint("session-1", test_turn_id(i), i, &format!("checkpoint-{i}"))
            .expect("write checkpoint");
    }

    let recent = store.read_recent("session-1", 100).expect("read recent");
    assert!(
        recent.len() <= 20,
        "prune should keep at most 20 checkpoints, got {}",
        recent.len()
    );
    assert_eq!(recent.first().map(|r| r.turn_number), Some(25));
}

// ── Per-session deletion (aletheia#7341) ────────────────────────────────────

#[test]
fn delete_session_removes_all_checkpoints_for_that_session() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    for i in 1..=5 {
        store
            .write_checkpoint("session-1", test_turn_id(i), i, &format!("checkpoint-{i}"))
            .expect("write checkpoint");
    }

    let removed = store.delete_session("session-1").expect("delete session");
    assert_eq!(removed, 5, "must report every removed row");

    let recent = store
        .read_recent("session-1", 100)
        .expect("read recent succeeds");
    assert!(
        recent.is_empty(),
        "no checkpoints must remain for a deleted session"
    );
    assert!(
        store
            .read_latest("session-1")
            .expect("read latest succeeds")
            .is_none()
    );
}

#[test]
fn delete_session_leaves_other_sessions_untouched() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    store
        .write_checkpoint("session-a", test_turn_id(1), 1, "a-content")
        .expect("write a");
    store
        .write_checkpoint("session-b", test_turn_id(1), 1, "b-content")
        .expect("write b");

    store.delete_session("session-a").expect("delete a");

    assert!(
        store
            .read_latest("session-a")
            .expect("read a succeeds")
            .is_none(),
        "deleted session must have no remaining checkpoint"
    );
    let b = store
        .read_latest("session-b")
        .expect("read b succeeds")
        .expect("session-b checkpoint survives");
    assert_eq!(b.content, "b-content");
}

#[test]
fn delete_session_for_a_session_with_no_checkpoints_is_a_zero_row_no_op() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    let removed = store
        .delete_session("no-such-session")
        .expect("delete succeeds");
    assert_eq!(removed, 0);
}

#[test]
fn delete_session_survives_store_reopen_at_same_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let store = FjallWorkingCheckpointStore::open(dir.path()).expect("open store");
        store
            .write_checkpoint("session-1", test_turn_id(1), 1, "before delete")
            .expect("write checkpoint");
        store.delete_session("session-1").expect("delete session");
    }

    let reopened = FjallWorkingCheckpointStore::open(dir.path()).expect("reopen store");
    assert!(
        reopened
            .read_latest("session-1")
            .expect("read latest succeeds")
            .is_none(),
        "deletion must be durable across reopen"
    );
}

// ── Legacy key migration (#4853) ────────────────────────────────────────────

/// Insert a checkpoint row directly under the pre-#4853 zero-padded-ordinal
/// key, bypassing `write_checkpoint` (which only ever writes the current
/// ULID-keyed shape). Simulates data written by the previous code version.
fn seed_legacy_row(store: &FjallWorkingCheckpointStore, session_id: &str, turn_number: u64) {
    let partition = store.partition().expect("partition");
    let key = format!("nous:working_checkpoint:{session_id}:{turn_number:020}");
    let record = super::WorkingCheckpointRecord {
        session_id: session_id.to_owned(),
        turn_number,
        content: format!("legacy-{turn_number}"),
        created_at: jiff::Timestamp::now().to_string(),
    };
    let value = serde_json::to_vec(&record).expect("serialize legacy record");
    let mut tx = store.db.write_tx();
    tx.insert(&partition, key.as_str(), value.as_slice());
    tx.commit().expect("commit legacy row");
}

#[test]
fn migration_removes_legacy_ordinal_keys() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    seed_legacy_row(&store, "session-1", 3);

    let legacy_prefix = super::FjallWorkingCheckpointStore::prefix_key("session-1");
    let partition = store.partition().expect("partition");
    let count_matching = |store: &FjallWorkingCheckpointStore| {
        use fjall::Readable as _;
        let snap = store.db.read_tx();
        snap.prefix(&partition, legacy_prefix.as_bytes()).count()
    };
    assert_eq!(count_matching(&store), 1, "legacy row seeded");

    store
        .migrate_legacy_ordinal_keys()
        .expect("migration succeeds");

    assert_eq!(
        count_matching(&store),
        0,
        "migration must remove the legacy-format row"
    );
}

#[test]
fn migration_is_idempotent_and_leaves_current_format_rows_untouched() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    store
        .write_checkpoint("session-1", test_turn_id(1), 1, "current-format")
        .expect("write current-format checkpoint");
    seed_legacy_row(&store, "session-1", 3);

    store
        .migrate_legacy_ordinal_keys()
        .expect("first migration succeeds");
    // Idempotent: running again on an already-migrated store is a no-op,
    // not an error.
    store
        .migrate_legacy_ordinal_keys()
        .expect("second migration succeeds");

    let latest = store
        .read_latest("session-1")
        .expect("read latest")
        .expect("current-format checkpoint survives migration");
    assert_eq!(
        latest.content, "current-format",
        "migration must not remove ULID-keyed rows"
    );
}

#[test]
fn reopen_migrates_legacy_keys_written_by_a_previous_open() {
    // WHY(#4853): the migration must run on `open`, not only when called
    // directly, so a store carrying rows from before this code shipped
    // self-heals on the next process start.
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let store = FjallWorkingCheckpointStore::open(dir.path()).expect("open store");
        seed_legacy_row(&store, "session-1", 7);
    }

    let reopened = FjallWorkingCheckpointStore::open(dir.path()).expect("reopen store");
    let prefix = super::FjallWorkingCheckpointStore::prefix_key("session-1");
    let partition = reopened.partition().expect("partition");
    let remaining = {
        use fjall::Readable as _;
        let snap = reopened.db.read_tx();
        snap.prefix(&partition, prefix.as_bytes()).count()
    };
    assert_eq!(
        remaining, 0,
        "legacy row from a prior open must be migrated away on reopen"
    );
}

#[test]
fn legacy_key_detection_matches_only_the_old_shape() {
    assert!(super::is_legacy_ordinal_key(
        b"nous:working_checkpoint:s1:00000000000000000003"
    ));
    assert!(
        !super::is_legacy_ordinal_key(b"nous:working_checkpoint:s1:00000000000000000AB3"),
        "non-digit suffix bytes must not match"
    );
    assert!(
        !super::is_legacy_ordinal_key(
            format!("nous:working_checkpoint:s1:{}", test_turn_id(3)).as_bytes()
        ),
        "a real 26-char ULID suffix must not match"
    );
    assert!(!super::is_legacy_ordinal_key(b"no-colon-at-all"));
}
