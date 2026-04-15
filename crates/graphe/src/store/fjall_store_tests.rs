#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

use crate::test_fixtures::test_store;
use crate::types::{Role, SessionStatus};

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
