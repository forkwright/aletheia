#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on Vecs with asserted length"
)]

use crate::error::Error;
use crate::test_fixtures::test_store;
use crate::types::{BlackboardRow, Message, Role, SessionStatus};

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
fn create_session_rejects_duplicate_raw_id() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("first create");
    let result = store.create_session("ses-1", "alice", "other", None, None);
    assert!(result.is_err(), "duplicate raw id must fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("UNIQUE constraint failed: session id ses-1 already exists"),
        "got: {msg}"
    );
}

#[test]
fn create_session_rejects_duplicate_raw_id_different_owner() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("first create");
    let result = store.create_session("ses-1", "bob", "main", None, None);
    assert!(
        result.is_err(),
        "duplicate raw id under different nous must fail"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("UNIQUE constraint failed: session id ses-1 already exists"),
        "got: {msg}"
    );
}

#[test]
fn find_or_create_session_idempotent() {
    let store = test_store();
    let first = store
        .find_or_create_session("ses-1", "alice", "main", None, None)
        .expect("first");
    let second = store
        .find_or_create_session("ses-1", "alice", "main", None, None)
        .expect("second");
    assert_eq!(first.id, second.id);
    assert_eq!(first.nous_id, second.nous_id);
    assert_eq!(first.session_key, second.session_key);
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
fn find_or_create_rejects_archived() {
    // Archived sessions must not be implicitly reactivated through the normal
    // message path. The caller must use the explicit unarchive endpoint first.
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .update_session_status("ses-1", SessionStatus::Archived)
        .expect("archive");

    let err = store
        .find_or_create_session("ses-new", "syn", "main", None, None)
        .expect_err("archived session must not be silently reactivated");
    assert!(
        matches!(err, Error::SessionIsArchived { ref id, .. } if id == "ses-1"),
        "expected SessionIsArchived, got: {err}"
    );
}

#[test]
fn find_or_create_reactivates_distilled() {
    // Distilled sessions (not Archived) may still be reactivated implicitly
    // since distillation is an internal lifecycle event, not an operator action.
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .update_session_status("ses-1", SessionStatus::Distilled)
        .expect("distill");

    let session = store
        .find_or_create_session("ses-new", "syn", "main", None, None)
        .expect("distilled session should be reactivated");
    assert_eq!(session.id, "ses-1", "should return existing session");
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
fn distillation_refreshes_nous_index() {
    let store = test_store();
    store
        .create_session("ses-a", "alice", "main", None, None)
        .expect("create a");
    // Give ses-b a slightly older initial timestamp by creating it second.
    store
        .create_session("ses-b", "alice", "secondary", None, None)
        .expect("create b");

    store
        .append_message("ses-a", Role::User, "old 1", None, None, 100)
        .expect("append");
    store
        .append_message("ses-a", Role::User, "old 2", None, None, 150)
        .expect("append");
    store
        .append_message("ses-a", Role::User, "keep this", None, None, 50)
        .expect("append");

    // Run all three distillation paths on ses-a.
    store
        .mark_messages_distilled("ses-a", &[1, 2])
        .expect("mark distilled");
    store
        .insert_distillation_summary("ses-a", "[Distillation]\n\nSummary")
        .expect("insert summary");
    store
        .record_distillation("ses-a", 3, 2, 300, 60, None)
        .expect("record distillation");

    let listed = store.list_sessions(Some("alice")).expect("list");
    assert_eq!(listed.len(), 2, "both sessions must remain indexed");
    let ids: Vec<&str> = listed.iter().map(|session| session.id.as_str()).collect();
    assert!(
        ids.contains(&"ses-a"),
        "distilled session must remain indexed"
    );
    assert!(ids.contains(&"ses-b"), "other session must remain indexed");
    assert_eq!(
        ids.iter().filter(|id| **id == "ses-a").count(),
        1,
        "distilled session must not have stale duplicate index rows"
    );
}

#[test]
fn list_sessions_no_duplicates_after_distillation() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
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
        .expect("mark distilled");
    store
        .insert_distillation_summary("ses-1", "[Distillation]\n\nSummary")
        .expect("insert summary");
    store
        .record_distillation("ses-1", 3, 2, 300, 60, None)
        .expect("record distillation");

    let listed = store.list_sessions(Some("alice")).expect("list");
    assert_eq!(
        listed.len(),
        1,
        "exactly one entry after all distillation writes"
    );
    assert_eq!(listed[0].id, "ses-1");
}

#[test]
fn session_provenance_updates_on_mutation_paths() {
    let store = test_store();
    store
        .create_session("ses-meta", "alice", "main", None, None)
        .expect("create");

    let initial = store
        .find_session_by_id("ses-meta")
        .expect("find")
        .expect("session");
    let initial_meta = initial.artefact_meta.expect("created session is stamped");
    assert_eq!(initial_meta.generated_at, initial.updated_at);
    assert_eq!(initial_meta.row_counts.get("messages"), Some(&0));
    assert_eq!(initial_meta.row_counts.get("distillations"), Some(&0));

    store
        .append_message("ses-meta", Role::User, "old 1", None, None, 100)
        .expect("append");
    store
        .append_message("ses-meta", Role::Assistant, "old 2", None, None, 150)
        .expect("append");
    store
        .append_message("ses-meta", Role::User, "keep this", None, None, 50)
        .expect("append");
    let after_append = store
        .find_session_by_id("ses-meta")
        .expect("find")
        .expect("session");
    let append_meta = after_append.artefact_meta.expect("append is stamped");
    assert_eq!(append_meta.generated_at, after_append.updated_at);
    assert_eq!(append_meta.row_counts.get("messages"), Some(&3));

    store
        .update_session_status("ses-meta", SessionStatus::Archived)
        .expect("archive");
    let after_status = store
        .find_session_by_id("ses-meta")
        .expect("find")
        .expect("session");
    let status_meta = after_status.artefact_meta.expect("status is stamped");
    assert_eq!(status_meta.generated_at, after_status.updated_at);
    assert_eq!(status_meta.row_counts.get("messages"), Some(&3));

    store
        .update_display_name("ses-meta", "Display")
        .expect("rename");
    let after_rename = store
        .find_session_by_id("ses-meta")
        .expect("find")
        .expect("session");
    let rename_meta = after_rename.artefact_meta.expect("rename is stamped");
    assert_eq!(rename_meta.generated_at, after_rename.updated_at);
    assert_eq!(rename_meta.row_counts.get("messages"), Some(&3));

    store
        .mark_messages_distilled("ses-meta", &[1, 2])
        .expect("mark distilled");
    store
        .insert_distillation_summary("ses-meta", "[Distillation]\n\nSummary")
        .expect("summary");
    store
        .record_distillation("ses-meta", 3, 2, 300, 60, None)
        .expect("record");

    let after_distill = store
        .find_session_by_id("ses-meta")
        .expect("find")
        .expect("session");
    let distill_meta = after_distill
        .artefact_meta
        .expect("distillation is stamped");
    assert_eq!(distill_meta.generated_at, after_distill.updated_at);
    assert_eq!(
        distill_meta.row_counts.get("messages"),
        Some(&u64::try_from(after_distill.metrics.message_count).unwrap())
    );
    assert_eq!(distill_meta.row_counts.get("distillations"), Some(&1));
}

#[test]
fn delete_session_removes_all_child_rows() {
    let store = test_store();
    store
        .create_session("ses-del", "alice", "main", None, None)
        .expect("create");

    store
        .append_message("ses-del", Role::User, "hello", None, None, 10)
        .expect("append");
    store
        .append_message("ses-del", Role::Assistant, "world", None, None, 20)
        .expect("append");

    let deleted = store.delete_session("ses-del").expect("delete");
    assert!(deleted, "delete must return true for an existing session");

    let history = store.get_history("ses-del", None).expect("history");
    assert!(
        history.is_empty(),
        "messages must be removed with the session"
    );

    let sessions = store.list_sessions(Some("alice")).expect("list");
    assert!(
        sessions.is_empty(),
        "session must not appear in listing after deletion"
    );

    let second_delete = store.delete_session("ses-del").expect("second delete");
    assert!(
        !second_delete,
        "deleting a non-existent session must return false"
    );
}

#[test]
fn delete_session_aborts_on_corrupt_session_row() {
    // WHY(#4984): delete_session decodes the session row up front; a corrupt
    // (non-JSON) row must abort the whole delete before any child rows are
    // removed, so the store stays internally consistent (all-or-nothing).
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .append_message("ses-1", Role::User, "hello", None, None, 5)
        .expect("append");

    // Corrupt the session row so its JSON decode fails during delete_session.
    write_raw(&store, "sessions", "ses-1", b"NOT_JSON");

    let result = store.delete_session("ses-1");
    assert!(
        result.is_err(),
        "delete_session must abort on a corrupt session row"
    );

    // The message child row must survive the aborted delete.
    let history = store
        .get_history("ses-1", None)
        .expect("history readable after abort");
    assert_eq!(
        history.len(),
        1,
        "child rows must survive a delete_session abort"
    );
}

#[test]
fn delete_session_removes_usage_distillation_and_note_rows() {
    // WHY(#4984): positive-path analog — confirm delete_session leaves NO child
    // rows behind across every partition (messages, usage, distillations, notes).
    let store = test_store();
    store
        .create_session("ses-x", "alice", "main", None, None)
        .expect("create");
    store
        .append_message("ses-x", Role::User, "hello", None, None, 10)
        .expect("append");
    store
        .record_usage(&crate::types::UsageRecord {
            session_id: "ses-x".to_owned(),
            turn_seq: 1,
            input_tokens: 5,
            output_tokens: 7,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            model: None,
        })
        .expect("record usage");
    store
        .record_distillation("ses-x", 2, 1, 100, 40, None)
        .expect("record distillation");
    store
        .add_note("ses-x", "alice", "task", "remember this")
        .expect("add note");

    assert!(
        !store
            .get_usage_for_session("ses-x")
            .expect("usage")
            .is_empty(),
        "usage row should exist before delete"
    );
    assert!(
        !store.get_notes("ses-x").expect("notes").is_empty(),
        "note row should exist before delete"
    );

    let deleted = store.delete_session("ses-x").expect("delete");
    assert!(deleted, "delete must return true for an existing session");

    assert!(
        store
            .get_history("ses-x", None)
            .expect("history")
            .is_empty(),
        "messages must be removed"
    );
    assert!(
        store
            .get_usage_for_session("ses-x")
            .expect("usage")
            .is_empty(),
        "usage rows must be removed"
    );
    assert!(
        store.get_notes("ses-x").expect("notes").is_empty(),
        "note rows must be removed"
    );
    assert!(
        store.find_session_by_id("ses-x").expect("lookup").is_none(),
        "session row must be removed"
    );
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
fn delete_session_removes_notes_via_session_gid_index() {
    // WHY: regression test for issue #5698 — deleting one session must not
    // require scanning the global `gid:` key space and must leave other
    // sessions' notes intact.
    let store = test_store();
    store
        .create_session("ses-a", "syn", "main", None, None)
        .expect("create a");
    store
        .create_session("ses-b", "syn", "secondary", None, None)
        .expect("create b");

    let id_a = store
        .add_note("ses-a", "syn", "task", "note for a")
        .expect("add note to a");
    let id_b = store
        .add_note("ses-b", "syn", "task", "note for b")
        .expect("add note to b");

    let deleted = store.delete_session("ses-a").expect("delete a");
    assert!(deleted);

    assert!(
        store
            .find_session_by_id("ses-a")
            .expect("query a")
            .is_none(),
        "session a must be removed"
    );
    assert!(
        store.get_notes("ses-a").expect("notes a").is_empty(),
        "session a notes must be removed"
    );
    let remaining = store.get_notes("ses-b").expect("notes b");
    assert_eq!(remaining.len(), 1, "session b notes must survive");
    assert_eq!(remaining[0].id, id_b);

    // NOTE: deleting by global id must also clean the reverse index.
    assert!(store.delete_note(id_b).expect("delete b's note"));
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
fn cleanup_orphan_messages_removes_messages_without_session() {
    let store = test_store();
    let message = Message {
        id: 1,
        session_id: "orphan".to_owned(),
        seq: 1,
        role: Role::User,
        content: "orphaned".to_owned(),
        tool_call_id: None,
        tool_name: None,
        token_estimate: 1,
        is_distilled: false,
        created_at: "1970-01-01T00:00:00Z".to_owned(),
    };
    let bytes = serde_json::to_vec(&message).expect("serialize message");
    write_raw(&store, "messages", "orphan:00000000000000000001", &bytes);
    write_raw(&store, "messages", "next_seq:orphan", &1u64.to_be_bytes());

    let cleaned = store
        .cleanup_orphan_messages("2000-01-01T00:00:00Z")
        .expect("cleanup orphan messages");

    assert_eq!(cleaned, 1);
    assert!(
        store
            .get_history_raw("orphan", None)
            .expect("orphan history")
            .is_empty()
    );
}

#[test]
fn ping_succeeds() {
    let store = test_store();
    store.ping().expect("ping should succeed");
}

// ── Portability raw entry points (issue #4163) ─────────────────────────────

#[cfg(feature = "portability")]
mod portability {
    use crate::test_fixtures::test_store;
    use crate::types::{
        Message, Role, Session, SessionMetrics, SessionOrigin, SessionStatus, SessionType,
    };

    fn seed_session(store: &super::super::SessionStore) -> String {
        let session = store
            .create_session("ses-raw", "syn", "main", None, Some("mock-model"))
            .expect("create session");
        store
            .append_message(&session.id, Role::User, "msg-1", None, None, 10)
            .expect("append 1");
        store
            .append_message(&session.id, Role::Assistant, "msg-2", None, None, 10)
            .expect("append 2");
        store
            .append_message(&session.id, Role::User, "msg-3", None, None, 10)
            .expect("append 3");
        session.id
    }

    #[test]
    fn get_history_raw_includes_distilled_messages() {
        let store = test_store();
        let id = seed_session(&store);

        store
            .mark_messages_distilled(&id, &[1, 2])
            .expect("mark distilled");

        let filtered = store.get_history(&id, None).expect("filtered");
        assert_eq!(filtered.len(), 1, "non-raw view drops distilled");
        assert_eq!(filtered[0].seq, 3);

        let raw = store.get_history_raw(&id, None).expect("raw");
        assert_eq!(raw.len(), 3, "raw view keeps all messages");
        let mut seqs: Vec<i64> = raw.iter().map(|m| m.seq).collect();
        seqs.sort_unstable();
        assert_eq!(seqs, vec![1, 2, 3]);
        let distilled_count = raw.iter().filter(|m| m.is_distilled).count();
        assert_eq!(distilled_count, 2, "is_distilled flag preserved");
    }

    #[test]
    fn insert_message_raw_preserves_seq_and_created_at() {
        let store = test_store();
        let s = store
            .create_session("ses-raw2", "syn", "main", None, None)
            .expect("create");

        let msg = Message {
            id: 99,
            session_id: s.id.clone(),
            seq: 42,
            role: Role::User,
            content: "raw insert".to_owned(),
            tool_call_id: None,
            tool_name: None,
            token_estimate: 7,
            is_distilled: true,
            created_at: "2024-06-15T12:00:00Z".to_owned(),
        };
        store.insert_message_raw(&msg).expect("raw insert");

        let raw = store.get_history_raw(&s.id, None).expect("read raw");
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].seq, 42);
        assert_eq!(raw[0].created_at, "2024-06-15T12:00:00Z");
        assert!(raw[0].is_distilled);
        assert_eq!(raw[0].content, "raw insert");
        assert_eq!(raw[0].id, 99);
    }

    #[test]
    fn insert_message_raw_does_not_touch_session_updated_at() {
        let store = test_store();
        let s = store
            .create_session("ses-raw3", "syn", "main", None, None)
            .expect("create");
        let original_updated_at = s.updated_at.clone();

        let msg = Message {
            id: 1,
            session_id: s.id.clone(),
            seq: 1,
            role: Role::User,
            content: "preserve".to_owned(),
            tool_call_id: None,
            tool_name: None,
            token_estimate: 3,
            is_distilled: false,
            created_at: "2024-01-01T00:00:00Z".to_owned(),
        };
        store.insert_message_raw(&msg).expect("raw insert");

        let after = store
            .find_session_by_id(&s.id)
            .expect("query")
            .expect("session");
        assert_eq!(
            after.updated_at, original_updated_at,
            "raw insert must not bump session.updated_at"
        );
        assert_eq!(
            after.metrics.message_count, 0,
            "raw insert must not bump message_count"
        );
        assert_eq!(
            after.metrics.token_count_estimate, 0,
            "raw insert must not bump token_count_estimate"
        );
    }

    #[test]
    fn insert_message_raw_then_append_does_not_collide() {
        // After raw insert at seq=5, a subsequent append must advance to seq=6.
        let store = test_store();
        let s = store
            .create_session("ses-raw4", "syn", "main", None, None)
            .expect("create");

        let raw_msg = Message {
            id: 50,
            session_id: s.id.clone(),
            seq: 5,
            role: Role::User,
            content: "imported".to_owned(),
            tool_call_id: None,
            tool_name: None,
            token_estimate: 1,
            is_distilled: false,
            created_at: "2024-01-01T00:00:00Z".to_owned(),
        };
        store.insert_message_raw(&raw_msg).expect("raw insert");

        let next_seq = store
            .append_message(&s.id, Role::Assistant, "fresh", None, None, 1)
            .expect("append");
        assert_eq!(next_seq, 6, "append must not collide with raw seq");
    }

    fn import_session_record(id: &str, status: SessionStatus, updated_at: &str) -> Session {
        Session {
            id: id.to_owned(),
            nous_id: "syn".to_owned(),
            session_key: format!("key-{id}"),
            status,
            model: Some("mock-model".to_owned()),
            session_type: SessionType::Primary,
            created_at: "2024-01-01T00:00:00Z".to_owned(),
            updated_at: updated_at.to_owned(),
            metrics: SessionMetrics {
                token_count_estimate: 1234,
                message_count: 56,
                last_input_tokens: 11,
                bootstrap_hash: Some("deadbeef".to_owned()),
                distillation_count: 2,
                last_distilled_at: Some("2024-02-01T00:00:00Z".to_owned()),
                computed_context_tokens: 999,
            },
            origin: SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: Some("signal".to_owned()),
                display_name: Some("Archived Run".to_owned()),
            },
            artefact_meta: None,
        }
    }

    #[test]
    fn import_session_preserves_status_timestamps_and_metrics() {
        let store = test_store();
        let original =
            import_session_record("ses-imp1", SessionStatus::Archived, "2024-03-15T12:00:00Z");

        store.import_session(&original, false).expect("import");

        let restored = store
            .find_session_by_id("ses-imp1")
            .expect("query")
            .expect("session");
        assert_eq!(restored.status, SessionStatus::Archived);
        assert_eq!(restored.created_at, "2024-01-01T00:00:00Z");
        assert_eq!(restored.updated_at, "2024-03-15T12:00:00Z");
        assert_eq!(restored.metrics.message_count, 56);
        assert_eq!(restored.metrics.token_count_estimate, 1234);
        assert_eq!(restored.metrics.distillation_count, 2);
        assert_eq!(
            restored.origin.display_name.as_deref(),
            Some("Archived Run")
        );
    }

    #[test]
    fn import_session_refuses_overwrite_without_force() {
        let store = test_store();
        let s = import_session_record("ses-imp2", SessionStatus::Active, "2024-04-01T00:00:00Z");

        store.import_session(&s, false).expect("first import");
        let err = store
            .import_session(&s, false)
            .expect_err("second without force must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("already exists") || msg.contains("already owned"),
            "error should mention idempotency, got: {msg}"
        );
    }

    #[test]
    fn import_session_with_force_overwrites_cleanly() {
        let store = test_store();
        let mut s =
            import_session_record("ses-imp3", SessionStatus::Active, "2024-04-01T00:00:00Z");
        store.import_session(&s, false).expect("first");
        s.status = SessionStatus::Archived;
        s.updated_at = "2024-05-01T00:00:00Z".to_owned();
        store.import_session(&s, true).expect("force overwrite");

        let restored = store
            .find_session_by_id("ses-imp3")
            .expect("query")
            .expect("session");
        assert_eq!(restored.status, SessionStatus::Archived);
        assert_eq!(restored.updated_at, "2024-05-01T00:00:00Z");
    }

    #[test]
    fn import_session_rejects_session_key_collision_with_different_id() {
        let store = test_store();
        let s1 = import_session_record("ses-imp-a", SessionStatus::Active, "2024-04-01T00:00:00Z");
        store.import_session(&s1, false).expect("first");

        let mut s2 = s1.clone();
        s2.id = "ses-imp-b".to_owned();
        let err = store
            .import_session(&s2, false)
            .expect_err("different id colliding session_key must fail");
        assert!(err.to_string().contains("already owned"), "error: {err}");
    }

    #[test]
    fn import_session_with_past_updated_at_appears_in_listing() {
        // Sweeper-safety: a past updated_at must produce a valid nous index
        // entry such that list_sessions returns the imported row.
        let store = test_store();
        let s = import_session_record(
            "ses-imp-old",
            SessionStatus::Archived,
            "2020-01-01T00:00:00Z",
        );
        store.import_session(&s, false).expect("import");

        let listed = store
            .list_sessions(Some("syn"))
            .expect("list")
            .into_iter()
            .filter(|row| row.id == "ses-imp-old")
            .collect::<Vec<_>>();
        assert_eq!(listed.len(), 1, "imported session must be discoverable");
        assert_eq!(listed[0].updated_at, "2020-01-01T00:00:00Z");
        assert_eq!(listed[0].status, SessionStatus::Archived);
    }

    #[test]
    fn force_import_with_changed_session_key_removes_stale_key_index() {
        // Regression: force-overwrite with a new session_key left the old
        // idx:key entry pointing at the moved session, causing find_session to
        // ghost-find a session that reported a different key.
        let store = test_store();
        let mut original = import_session_record(
            "ses-key-move",
            SessionStatus::Active,
            "2024-06-01T00:00:00Z",
        );
        original.session_key = "original-key".to_owned();
        store
            .import_session(&original, false)
            .expect("first import");

        let mut moved = original.clone();
        moved.session_key = "new-key".to_owned();
        moved.updated_at = "2024-06-02T00:00:00Z".to_owned();
        store.import_session(&moved, true).expect("force overwrite");

        // Stale key index must be gone.
        let ghost = store
            .find_session("syn", "original-key")
            .expect("find on stale key");
        assert!(
            ghost.is_none(),
            "stale idx:key must be removed after force-overwrite with new session_key"
        );

        // New key index must resolve correctly.
        let live = store
            .find_session("syn", "new-key")
            .expect("find on new key");
        assert!(live.is_some(), "new session_key must be findable");
        assert_eq!(live.unwrap().id, "ses-key-move");
    }

    #[test]
    fn force_import_with_changed_nous_id_removes_stale_nous_index() {
        let store = test_store();
        let mut original = import_session_record(
            "ses-nous-move",
            SessionStatus::Active,
            "2024-06-01T00:00:00Z",
        );
        original.nous_id = "nous-a".to_owned();
        original.session_key = "mk".to_owned();
        store
            .import_session(&original, false)
            .expect("first import");

        let mut retargeted = original.clone();
        retargeted.nous_id = "nous-b".to_owned();
        store
            .import_session(&retargeted, true)
            .expect("force overwrite");

        // Old nous-a should have no sessions left.
        let old_listing = store.list_sessions(Some("nous-a")).expect("list nous-a");
        assert!(
            old_listing.iter().all(|s| s.id != "ses-nous-move"),
            "stale idx:nous entry must be removed after nous_id change"
        );

        // New nous-b must list the session.
        let new_listing = store.list_sessions(Some("nous-b")).expect("list nous-b");
        assert!(
            new_listing.iter().any(|s| s.id == "ses-nous-move"),
            "session must appear under new nous_id after force-overwrite"
        );
    }
}
