//! Tests for [`FjallWorkingCheckpointStore`].

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]

use organon::types::WorkingCheckpointStore;

use super::FjallWorkingCheckpointStore;

#[test]
fn write_and_read_latest_roundtrip() {
    let store = FjallWorkingCheckpointStore::open_in_memory().expect("open store");
    store
        .write_checkpoint("session-1", 1, "first checkpoint")
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
        .write_checkpoint("session-1", 1, "first")
        .expect("write first");
    store
        .write_checkpoint("session-1", 2, "second")
        .expect("write second");
    store
        .write_checkpoint("session-1", 3, "third")
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
            .write_checkpoint("session-1", i, &format!("checkpoint-{i}"))
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
        .write_checkpoint("session-a", 1, "a-content")
        .expect("write a");
    store
        .write_checkpoint("session-b", 1, "b-content")
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
            .write_checkpoint("session-1", i, &format!("checkpoint-{i}"))
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
        .write_checkpoint("session-1", 1, "original")
        .expect("write original");
    store
        .write_checkpoint("session-1", 1, "updated")
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
            .write_checkpoint("session-1", 1, "before restart")
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
            .write_checkpoint("session-1", i, &format!("checkpoint-{i}"))
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
