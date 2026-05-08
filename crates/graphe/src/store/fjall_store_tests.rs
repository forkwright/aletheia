#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on Vecs with asserted length"
)]

use crate::error::Error;
use crate::test_fixtures::test_store;
use crate::types::{BlackboardRow, Role, SessionStatus};

fn write_raw(store: &super::SessionStore, partition_name: &str, key: &str, value: &[u8]) {
    let partition = store.partition(partition_name).expect("partition opens");
    let mut tx = store.db.write_tx();
    tx.insert(&partition, key, value);
    tx.commit().expect("raw value committed");
}

#[test]
fn create_and_find_session() {
    let store = test_store();
    let session = store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    assert_eq!(session.id, "ses-1");
    assert_eq!(session.nous_id, "syn");
    assert_eq!(session.session_key, "main");
    assert_eq!(session.status, SessionStatus::Active);

    let found = store.find_session("syn", "main").expect("find session");
    assert!(found.is_some(), "session should exist after creation");
    assert_eq!(found.unwrap().id, "ses-1");
}

#[test]
fn find_session_returns_none_for_missing() {
    let store = test_store();
    let found = store
        .find_session("syn", "nonexistent")
        .expect("find session");
    assert!(found.is_none());
}

#[test]
fn create_session_unique_constraint() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("first create");
    let result = store.create_session("ses-2", "syn", "main", None, None);
    assert!(
        result.is_err(),
        "duplicate (nous_id, session_key) must fail"
    );
}

#[test]
fn append_and_retrieve_messages() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");

    let seq1 = store
        .append_message("ses-1", Role::User, "hello", None, None, 10)
        .expect("append");
    let seq2 = store
        .append_message("ses-1", Role::Assistant, "hi there", None, None, 15)
        .expect("append");

    assert_eq!(seq1, 1);
    assert_eq!(seq2, 2);

    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "hello");
    assert_eq!(history[1].content, "hi there");
}

#[test]
fn message_updates_session_counts() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .append_message("ses-1", Role::User, "hello", None, None, 100)
        .expect("append");
    store
        .append_message("ses-1", Role::Assistant, "world", None, None, 200)
        .expect("append");

    let session = store.find_session_by_id("ses-1").expect("query").unwrap();
    assert_eq!(session.metrics.message_count, 2);
    assert_eq!(session.metrics.token_count_estimate, 300);
}

#[test]
fn list_sessions_by_nous_id() {
    let store = test_store();
    store
        .create_session("ses-a", "agent-x", "main", None, None)
        .expect("create a");
    store
        .create_session("ses-b", "agent-x", "secondary", None, None)
        .expect("create b");
    store
        .create_session("ses-c", "agent-y", "main", None, None)
        .expect("create c");

    let agent_x = store.list_sessions(Some("agent-x")).expect("list");
    assert_eq!(agent_x.len(), 2);
    let all = store.list_sessions(None).expect("list all");
    assert_eq!(all.len(), 3);
}

#[test]
fn update_session_status() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .update_session_status("ses-1", SessionStatus::Archived)
        .expect("update status");
    let session = store.find_session_by_id("ses-1").expect("query").unwrap();
    assert_eq!(session.status, SessionStatus::Archived);
}

#[test]
fn find_or_create_reactivates_archived() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .update_session_status("ses-1", SessionStatus::Archived)
        .expect("archive");

    let session = store
        .find_or_create_session("ses-new", "syn", "main", None, None)
        .expect("find_or_create");
    assert_eq!(
        session.id, "ses-1",
        "should return existing, not create new"
    );
    assert_eq!(session.status, SessionStatus::Active);
}

#[test]
fn distillation_marks_and_recalculates() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .append_message("ses-1", Role::User, "old 1", None, None, 100)
        .expect("append");
    store
        .append_message("ses-1", Role::User, "old 2", None, None, 150)
        .expect("append");
    store
        .append_message("ses-1", Role::User, "keep this", None, None, 50)
        .expect("append");

    store
        .mark_messages_distilled("ses-1", &[1, 2])
        .expect("distill");

    let history = store.get_history("ses-1", None).expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content, "keep this");

    let session = store.find_session_by_id("ses-1").expect("query").unwrap();
    assert_eq!(session.metrics.message_count, 1);
    assert_eq!(session.metrics.token_count_estimate, 50);
}

#[test]
fn insert_distillation_summary() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .append_message("ses-1", Role::User, "msg1", None, None, 100)
        .expect("append");
    store
        .append_message("ses-1", Role::Assistant, "msg2", None, None, 200)
        .expect("append");
    store
        .append_message("ses-1", Role::User, "msg3", None, None, 50)
        .expect("append");

    store
        .mark_messages_distilled("ses-1", &[1, 2])
        .expect("distill");
    store
        .insert_distillation_summary("ses-1", "[Distillation #1]\n\nSummary")
        .expect("summary");

    let history = store.get_history("ses-1", None).expect("history");
    assert_eq!(history.len(), 2, "summary + undistilled msg3");
    assert_eq!(history[0].role, Role::System);
    assert!(history[0].content.contains("Distillation #1"));
    assert_eq!(history[1].content, "msg3");
}

#[test]
fn blackboard_crud() {
    let store = test_store();
    store
        .blackboard_write("goal", "finish M0b", "syn", 3600)
        .expect("write");
    let entry = store.blackboard_read("goal").expect("read").unwrap();
    assert_eq!(entry.value, "finish M0b");
    assert_eq!(entry.author_nous_id, "syn");

    store
        .blackboard_write("goal", "updated goal", "syn", 3600)
        .expect("overwrite");
    let updated = store.blackboard_read("goal").expect("read").unwrap();
    assert_eq!(updated.value, "updated goal");

    let deleted = store.blackboard_delete("goal", "syn").expect("delete");
    assert!(deleted);
    assert!(store.blackboard_read("goal").expect("read").is_none());
}

#[test]
fn blackboard_write_rejects_ttl_overflow() {
    let store = test_store();
    let result = store.blackboard_write("goal", "finish M0b", "syn", i64::MAX);
    assert!(
        matches!(
            result,
            Err(Error::TtlOverflow {
                ttl_secs: i64::MAX,
                ..
            })
        ),
        "TTL overflow must be returned to the caller"
    );
    assert!(
        store
            .blackboard_read("goal")
            .expect("read after failed write")
            .is_none(),
        "overflowing TTL write must not create an immortal entry"
    );
}

#[test]
fn cleanup_expired_entries_removes_expired_blackboard_rows() {
    let store = test_store();
    let row = BlackboardRow {
        key: "goal".to_owned(),
        value: "finish M0b".to_owned(),
        author_nous_id: "syn".to_owned(),
        ttl_seconds: 1,
        created_at: "1970-01-01T00:00:00.000Z".to_owned(),
        expires_at: Some("1970-01-01T00:00:01.000Z".to_owned()),
    };
    let data = serde_json::to_vec(&row).expect("blackboard row serializes");
    write_raw(&store, "blackboard", "goal", &data);

    assert_eq!(store.cleanup_expired_entries().expect("cleanup"), 1);
    assert_eq!(
        store
            .cleanup_expired_entries()
            .expect("second cleanup after removal"),
        0
    );
}

#[test]
fn corrupted_message_json_propagates_from_history() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .append_message("ses-1", Role::User, "hello", None, None, 10)
        .expect("append");

    write_raw(
        &store,
        "messages",
        "ses-1:00000000000000000001",
        b"{not valid json",
    );

    let result = store.get_history("ses-1", None);
    assert!(
        matches!(result, Err(Error::StoredJson { .. })),
        "corrupt message JSON must not be silently skipped"
    );
}

#[test]
fn corrupted_note_json_propagates_from_note_list() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .add_note("ses-1", "syn", "task", "do something")
        .expect("add note");

    write_raw(&store, "notes", "ses-1:00000000000000000001", b"not-json");

    let result = store.get_notes("ses-1");
    assert!(
        matches!(result, Err(Error::StoredJson { .. })),
        "corrupt note JSON must not be silently skipped"
    );
}

#[test]
fn corrupted_blackboard_json_propagates_from_list() {
    let store = test_store();
    store
        .blackboard_write("goal", "finish M0b", "syn", 3600)
        .expect("write");

    write_raw(&store, "blackboard", "goal", b"not-json");

    let result = store.blackboard_list();
    assert!(
        matches!(result, Err(Error::StoredJson { .. })),
        "corrupt blackboard JSON must not be silently skipped"
    );
}

#[test]
fn notes_crud() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .add_note("ses-1", "syn", "task", "do something")
        .expect("add note");
    store
        .add_note("ses-1", "syn", "context", "background")
        .expect("add note");

    let notes = store.get_notes("ses-1").expect("get notes");
    assert_eq!(notes.len(), 2);

    let note_id = notes[0].id;
    let deleted = store.delete_note(note_id).expect("delete note");
    assert!(deleted);
    let notes_after = store.get_notes("ses-1").expect("get notes after delete");
    assert_eq!(notes_after.len(), 1);
}

#[test]
fn delete_session_removes_all_data() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .append_message("ses-1", Role::User, "hi", None, None, 10)
        .expect("append");

    let deleted = store.delete_session("ses-1").expect("delete");
    assert!(deleted);
    assert!(store.find_session_by_id("ses-1").expect("query").is_none());
    assert!(
        store
            .get_history("ses-1", None)
            .expect("history")
            .is_empty()
    );
}

#[test]
fn ping_succeeds() {
    let store = test_store();
    store.ping().expect("ping should succeed");
}
