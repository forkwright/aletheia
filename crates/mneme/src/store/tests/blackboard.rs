//! Tests for blackboard CRUD and expiry.
#![expect(clippy::expect_used, reason = "test assertions")]
use super::super::SessionStore;
use crate::types::Role;

fn test_store() -> SessionStore {
    SessionStore::open_in_memory().expect("open in-memory store")
}

// --- Blackboard ---

#[test]
fn blackboard_crud() {
    let store = test_store();
    store
        .blackboard_write("goal", "finish M0b", "syn", 3600)
        .expect("blackboard write");

    let entry = store
        .blackboard_read("goal")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        entry.key, "goal",
        "blackboard entry key should match the written key"
    );
    assert_eq!(
        entry.value, "finish M0b",
        "blackboard entry value should match the written value"
    );
    assert_eq!(
        entry.author_nous_id, "syn",
        "blackboard entry author should match the writing agent"
    );

    let list = store.blackboard_list().expect("blackboard list");
    assert_eq!(
        list.len(),
        1,
        "blackboard should contain exactly one entry after one write"
    );

    let deleted = store
        .blackboard_delete("goal", "syn")
        .expect("blackboard delete");
    assert!(
        deleted,
        "blackboard_delete should return true when entry was removed"
    );

    let gone = store.blackboard_read("goal").expect("blackboard delete");
    assert!(
        gone.is_none(),
        "deleted blackboard entry should no longer be readable"
    );
}

#[test]
fn blackboard_upsert() {
    let store = test_store();
    store
        .blackboard_write("status", "starting", "syn", 3600)
        .expect("blackboard write");
    store
        .blackboard_write("status", "running", "syn", 3600)
        .expect("blackboard write");

    let entry = store
        .blackboard_read("status")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        entry.value, "running",
        "second write to same key should overwrite the previous value"
    );

    let list = store.blackboard_list().expect("blackboard list");
    assert_eq!(
        list.len(),
        1,
        "upsert to same key should not create a second entry"
    );
}

#[test]
fn blackboard_delete_only_author() {
    let store = test_store();
    store
        .blackboard_write("secret", "value", "syn", 3600)
        .expect("blackboard write");

    let deleted = store
        .blackboard_delete("secret", "other-agent")
        .expect("blackboard write");
    assert!(!deleted, "delete by non-author should return false");

    let still_there = store.blackboard_read("secret").expect("blackboard delete");
    assert!(
        still_there.is_some(),
        "entry written by syn should still exist after failed delete by other-agent"
    );
}

#[test]
fn blackboard_read_missing_returns_none() {
    let store = test_store();
    let result = store
        .blackboard_read("nonexistent")
        .expect("blackboard read");
    assert!(
        result.is_none(),
        "reading a nonexistent blackboard key should return None"
    );
}

#[test]
fn blackboard_expiry_filtered() {
    let store = test_store();
    store
        .blackboard_write("temp", "data", "syn", 3600)
        .expect("blackboard write");

    // Manually set expires_at to the past
    store
        .conn
        .execute(
            "UPDATE blackboard SET expires_at = datetime('now', '-1 second') WHERE key = 'temp'",
            [],
        )
        .expect("execute sql");

    let result = store.blackboard_read("temp").expect("blackboard read");
    assert!(
        result.is_none(),
        "expired blackboard entry should not be returned by read"
    );

    let list = store.blackboard_list().expect("blackboard list");
    assert!(
        list.is_empty(),
        "expired blackboard entry should not appear in list"
    );
}

#[test]
fn record_distillation_increments_count() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.metrics.distillation_count, 0,
        "distillation_count should be 0 for a newly created session"
    );
    assert!(
        session.metrics.last_distilled_at.is_none(),
        "last_distilled_at should be None before any distillation"
    );

    store
        .record_distillation("ses-1", 20, 5, 50000, 2000, Some("sonnet"))
        .expect("record distillation");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.metrics.distillation_count, 1,
        "distillation_count should be 1 after first distillation"
    );
    assert!(
        session.metrics.last_distilled_at.is_some(),
        "last_distilled_at should be set after first distillation"
    );

    store
        .record_distillation("ses-1", 15, 3, 30000, 1500, None)
        .expect("record distillation");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.metrics.distillation_count, 2,
        "distillation_count should be 2 after second distillation"
    );
}

#[test]
fn open_in_memory_creates_tables() {
    let store = test_store();
    let sessions = store.list_sessions(None).expect("list sessions");
    assert!(
        sessions.is_empty(),
        "fresh in-memory store should have no sessions"
    );
    let session = store
        .create_session("tbl-check", "syn", "main", None, None)
        .expect("create session");
    assert_eq!(
        session.id, "tbl-check",
        "created session should have the expected id"
    );
    let found = store
        .find_session_by_id("tbl-check")
        .expect("create session");
    assert!(
        found.is_some(),
        "session tbl-check should be findable immediately after creation"
    );
}

#[test]
fn create_session_duplicate_id_errors() {
    let store = test_store();
    store
        .create_session("ses-dup", "syn", "main", None, None)
        .expect("create session");
    let result = store.create_session("ses-dup", "syn", "other", None, None);
    assert!(
        result.is_err(),
        "creating a session with a duplicate id should fail"
    );
}

#[test]
fn find_session_nonexistent() {
    let store = test_store();
    let found = store
        .find_session("no-such-nous", "no-such-key")
        .expect("find session");
    assert!(
        found.is_none(),
        "find_session for unknown nous/key pair should return None"
    );
}

#[test]
fn find_session_by_id_nonexistent() {
    let store = test_store();
    let found = store
        .find_session_by_id("non-existent-id-999")
        .expect("find session by id");
    assert!(
        found.is_none(),
        "find_session_by_id for unknown id should return None"
    );
}

#[test]
fn append_message_to_nonexistent_session() {
    let store = test_store();
    let result = store.append_message("no-session", Role::User, "hello", None, None, 10);
    assert!(
        result.is_err(),
        "appending to a non-existent session should fail due to foreign key constraint"
    );
}
