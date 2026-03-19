//! Advanced tests for distillation summaries, display names, and message ordering.
#![expect(clippy::expect_used, reason = "test assertions")]
use super::super::SessionStore;
use crate::types::{Role, SessionStatus, UsageRecord};

fn test_store() -> SessionStore {
    SessionStore::open_in_memory().expect("open in-memory store")
}

#[test]
fn get_history_empty_session() {
    let store = test_store();
    store
        .create_session("empty-ses", "syn", "main", None, None)
        .expect("create session");
    let history = store.get_history("empty-ses", None).expect("get history");
    assert!(
        history.is_empty(),
        "get_history on a session with no messages should return empty vec"
    );
    let history_limited = store
        .get_history("empty-ses", Some(10))
        .expect("get history");
    assert!(
        history_limited.is_empty(),
        "get_history with limit on empty session should return empty vec"
    );
}

#[test]
fn blackboard_write_read_delete_cycle() {
    let store = test_store();

    let read_before = store
        .blackboard_read("cycle-key")
        .expect("blackboard write");
    assert!(
        read_before.is_none(),
        "cycle-key should not exist before any write"
    );

    store
        .blackboard_write("cycle-key", "value-1", "agent-alice", 7200)
        .expect("blackboard write");
    let entry = store
        .blackboard_read("cycle-key")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        entry.value, "value-1",
        "blackboard entry value should be 'value-1' after first write"
    );
    assert_eq!(
        entry.author_nous_id, "agent-alice",
        "blackboard entry author should be 'agent-alice'"
    );
    assert_eq!(
        entry.ttl_seconds, 7200,
        "blackboard entry ttl_seconds should match the written TTL"
    );

    store
        .blackboard_write("cycle-key", "value-2", "agent-alice", 3600)
        .expect("blackboard write");
    let updated = store
        .blackboard_read("cycle-key")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        updated.value, "value-2",
        "blackboard entry value should be updated to 'value-2' after second write"
    );
    assert_eq!(
        updated.ttl_seconds, 3600,
        "blackboard entry ttl_seconds should be updated to 3600 after second write"
    );

    let deleted = store
        .blackboard_delete("cycle-key", "agent-alice")
        .expect("blackboard delete");
    assert!(
        deleted,
        "blackboard_delete should return true when cycle-key entry was removed"
    );

    let after_delete = store
        .blackboard_read("cycle-key")
        .expect("blackboard delete");
    assert!(
        after_delete.is_none(),
        "cycle-key should not be readable after deletion"
    );

    let list = store.blackboard_list().expect("blackboard list");
    assert!(
        list.is_empty(),
        "blackboard list should be empty after the only entry was deleted"
    );
}

#[test]
fn add_note_invalid_category() {
    let store = test_store();
    store
        .create_session("ses-cat", "syn", "main", None, None)
        .expect("create session");
    let result = store.add_note("ses-cat", "syn", "totally_bogus_category", "some content");
    assert!(
        result.is_err(),
        "invalid category should be rejected by CHECK constraint"
    );
}

#[test]
fn session_status_transitions() {
    let store = test_store();
    store
        .create_session("ses-status", "syn", "main", None, None)
        .expect("create session");

    let session = store
        .find_session_by_id("ses-status")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.status,
        SessionStatus::Active,
        "newly created session should have Active status"
    );

    store
        .update_session_status("ses-status", SessionStatus::Archived)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-status")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.status,
        SessionStatus::Archived,
        "session status should be Archived after updating to Archived"
    );

    store
        .update_session_status("ses-status", SessionStatus::Distilled)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-status")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.status,
        SessionStatus::Distilled,
        "session status should be Distilled after updating to Distilled"
    );

    store
        .update_session_status("ses-status", SessionStatus::Active)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-status")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.status,
        SessionStatus::Active,
        "session status should be Active after updating back to Active"
    );
}

#[test]
fn insert_distillation_summary_and_cleanup() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    // Add some messages
    store
        .append_message("ses-1", Role::User, "msg1", None, None, 100)
        .expect("append message");
    store
        .append_message("ses-1", Role::Assistant, "msg2", None, None, 200)
        .expect("append message");
    store
        .append_message("ses-1", Role::User, "msg3", None, None, 50)
        .expect("append message");

    // Mark first two as distilled
    store
        .mark_messages_distilled("ses-1", &[1, 2])
        .expect("mark messages distilled");

    // Insert summary (should also delete distilled messages)
    store
        .insert_distillation_summary("ses-1", "[Distillation #1]\n\nSummary text")
        .expect("insert distillation summary");

    let history = store.get_history("ses-1", None).expect("get history");
    // Should have: summary (seq 0) + undistilled msg3 (seq shifted)
    assert_eq!(
        history.len(),
        2,
        "history should contain the summary plus the one undistilled message"
    );
    assert_eq!(
        history[0].role,
        Role::System,
        "distillation summary should be inserted as a System message"
    );
    assert!(
        history[0].content.contains("Distillation #1"),
        "summary message should contain the distillation header"
    );
    assert_eq!(
        history[1].content, "msg3",
        "undistilled message should be preserved after distillation"
    );

    // Session counts should reflect new state
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.metrics.message_count, 2,
        "message_count should reflect summary plus remaining undistilled messages"
    );
}

/// Regression test for Bug #1245: the former implementation shifted undistilled
/// seq values up by 1 before inserting the summary at seq 0. When consecutive
/// undistilled messages exist (e.g. [3,4,5]), shifting 3→4 conflicted with the
/// existing seq 4 and raised a `UNIQUE(session_id, seq)` violation.
#[test]
fn insert_distillation_summary_consecutive_undistilled_no_conflict() {
    let store = test_store();
    store
        .create_session("ses-cd", "syn", "main", None, None)
        .expect("create session");

    // Five messages: first two will be distilled, last three are consecutive undistilled.
    for i in 1..=5_u8 {
        store
            .append_message("ses-cd", Role::User, &format!("msg{i}"), None, None, 10)
            .expect("append message");
    }

    // Mark the first two messages as distilled.
    store
        .mark_messages_distilled("ses-cd", &[1, 2])
        .expect("mark messages distilled");

    // This must not fail with a UNIQUE constraint violation for seqs [3,4,5].
    store
        .insert_distillation_summary("ses-cd", "Summary #1")
        .expect("first distillation summary must not conflict on consecutive undistilled seqs");

    let history = store.get_history("ses-cd", None).expect("get history");
    // Summary at seq 0 plus three undistilled messages.
    assert_eq!(
        history.len(),
        4,
        "history should contain summary at seq 0 plus three undistilled messages"
    );
    assert_eq!(
        history[0].role,
        Role::System,
        "first history entry should be the distillation summary System message"
    );
    assert!(
        history[0].content.contains("Summary #1"),
        "first summary message should contain 'Summary #1'"
    );
    // Remaining messages preserve their original seq ordering.
    assert_eq!(
        history[1].content, "msg3",
        "first undistilled message should be msg3"
    );
    assert_eq!(
        history[2].content, "msg4",
        "second undistilled message should be msg4"
    );
    assert_eq!(
        history[3].content, "msg5",
        "third undistilled message should be msg5"
    );
}

/// Regression test for Bug #1245 (second distillation path): inserting a second
/// summary must not conflict with the previous summary still sitting at seq 0.
#[test]
fn insert_distillation_summary_twice_succeeds() {
    let store = test_store();
    store
        .create_session("ses-2d", "syn", "main", None, None)
        .expect("create session");

    for i in 1..=4_u8 {
        store
            .append_message("ses-2d", Role::User, &format!("msg{i}"), None, None, 10)
            .expect("append message");
    }

    // First distillation: condense messages 1 and 2.
    store
        .mark_messages_distilled("ses-2d", &[1, 2])
        .expect("mark messages distilled");
    store
        .insert_distillation_summary("ses-2d", "Summary #1")
        .expect("first distillation must succeed");

    // Verify state: summary at seq 0, msg3 and msg4 still undistilled.
    let history = store.get_history("ses-2d", None).expect("get history");
    assert_eq!(
        history.len(),
        3,
        "after first distillation history should have summary plus msg3 and msg4"
    );
    let summary_seq = history[0].seq;
    let msg3_seq = history[1].seq;
    assert_eq!(
        history[0].role,
        Role::System,
        "first history entry after distillation should be the System summary"
    );
    assert_eq!(
        history[1].content, "msg3",
        "second history entry should be msg3"
    );
    assert_eq!(
        history[2].content, "msg4",
        "third history entry should be msg4"
    );

    // Second distillation: condense the previous summary and msg3.
    store
        .mark_messages_distilled("ses-2d", &[summary_seq, msg3_seq])
        .expect("mark messages distilled");

    // This must not conflict with the old summary that is still at seq 0 in the DB.
    store
        .insert_distillation_summary("ses-2d", "Summary #2")
        .expect("second distillation must not conflict with old seq-0 summary");

    let history = store.get_history("ses-2d", None).expect("get history");
    assert_eq!(
        history.len(),
        2,
        "after second distillation history should contain only the new summary and msg4"
    );
    assert_eq!(
        history[0].role,
        Role::System,
        "first entry after second distillation should be a System summary"
    );
    assert!(
        history[0].content.contains("Summary #2"),
        "second summary message should contain 'Summary #2'"
    );
    assert_eq!(
        history[1].content, "msg4",
        "remaining undistilled message should be msg4"
    );
}

#[test]
fn update_usage_creates_record() {
    let store = test_store();
    store
        .create_session("ses-usage", "syn", "main", None, None)
        .expect("create session");

    store
        .record_usage(&UsageRecord {
            session_id: "ses-usage".to_owned(),
            turn_seq: 1,
            input_tokens: 500,
            output_tokens: 200,
            cache_read_tokens: 400,
            cache_write_tokens: 100,
            model: Some("test-model".to_owned()),
        })
        .expect("record usage");

    store
        .record_usage(&UsageRecord {
            session_id: "ses-usage".to_owned(),
            turn_seq: 2,
            input_tokens: 600,
            output_tokens: 300,
            cache_read_tokens: 500,
            cache_write_tokens: 150,
            model: None,
        })
        .expect("record usage");

    let count: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM usage WHERE session_id = ?1",
            ["ses-usage"],
            |row| row.get(0),
        )
        .expect("query usage count");
    assert_eq!(
        count, 2,
        "usage table should contain two records after two record_usage calls"
    );
}

#[test]
fn get_history_with_limit_respected() {
    let store = test_store();
    store
        .create_session("ses-lim", "syn", "main", None, None)
        .expect("create session");

    for i in 1..=10 {
        store
            .append_message(
                "ses-lim",
                Role::User,
                &format!("message {i}"),
                None,
                None,
                10,
            )
            .expect("append message");
    }

    let history_3 = store.get_history("ses-lim", Some(3)).expect("get history");
    assert_eq!(
        history_3.len(),
        3,
        "limit of 3 from 10 messages should return exactly 3 messages"
    );
    assert_eq!(
        history_3[0].content, "message 8",
        "with limit 3 from 10 messages, first result should be message 8"
    );
    assert_eq!(
        history_3[2].content, "message 10",
        "with limit 3 from 10 messages, last result should be message 10"
    );

    let history_all = store.get_history("ses-lim", None).expect("get history");
    assert_eq!(
        history_all.len(),
        10,
        "no limit should return all 10 messages"
    );
}

#[test]
fn create_multiple_sessions_same_nous() {
    let store = test_store();
    store
        .create_session("ses-a", "agent-x", "main", None, None)
        .expect("create session");
    store
        .create_session("ses-b", "agent-x", "secondary", None, None)
        .expect("create session");
    store
        .create_session("ses-c", "agent-x", "prosoche-wake", None, None)
        .expect("create session");

    let sessions = store
        .list_sessions(Some("agent-x"))
        .expect("create session");
    assert_eq!(
        sessions.len(),
        3,
        "listing sessions for agent-x should return all 3 created sessions"
    );

    let keys: Vec<&str> = sessions.iter().map(|s| s.session_key.as_str()).collect();
    assert!(
        keys.contains(&"main"),
        "session with key 'main' should be listed for agent-x"
    );
    assert!(
        keys.contains(&"secondary"),
        "session with key 'secondary' should be listed for agent-x"
    );
    assert!(
        keys.contains(&"prosoche-wake"),
        "session with key 'prosoche-wake' should be listed for agent-x"
    );
}

#[test]
fn blackboard_read_nonexistent_key() {
    let store = test_store();
    let result = store
        .blackboard_read("does-not-exist-key")
        .expect("blackboard read");
    assert!(
        result.is_none(),
        "reading a key that was never written should return None"
    );
}

#[test]
fn list_notes_empty() {
    let store = test_store();
    store
        .create_session("ses-no-notes", "syn", "main", None, None)
        .expect("create session");

    let notes = store.get_notes("ses-no-notes").expect("create session");
    assert!(
        notes.is_empty(),
        "session with no notes added should return empty list"
    );
}

#[test]
fn message_ordering_preserved() {
    let store = test_store();
    store
        .create_session("ses-ord", "syn", "main", None, None)
        .expect("create session");

    store
        .append_message("ses-ord", Role::User, "first", None, None, 10)
        .expect("append message");
    store
        .append_message("ses-ord", Role::Assistant, "second", None, None, 10)
        .expect("append message");
    store
        .append_message("ses-ord", Role::User, "third", None, None, 10)
        .expect("append message");

    let history = store.get_history("ses-ord", None).expect("get history");
    assert_eq!(
        history.len(),
        3,
        "history should contain all 3 appended messages"
    );
    assert_eq!(
        history[0].content, "first",
        "first message content should be 'first'"
    );
    assert_eq!(
        history[1].content, "second",
        "second message content should be 'second'"
    );
    assert_eq!(
        history[2].content, "third",
        "third message content should be 'third'"
    );
    assert!(
        history[0].seq < history[1].seq,
        "message seq values must be strictly increasing: first < second"
    );
    assert!(
        history[1].seq < history[2].seq,
        "message seq values must be strictly increasing: second < third"
    );
}

#[test]
fn distill_marks_messages() {
    let store = test_store();
    store
        .create_session("ses-dist", "syn", "main", None, None)
        .expect("create session");

    store
        .append_message("ses-dist", Role::User, "to distill 1", None, None, 50)
        .expect("append message");
    store
        .append_message("ses-dist", Role::User, "to distill 2", None, None, 60)
        .expect("append message");
    store
        .append_message("ses-dist", Role::User, "keep me", None, None, 30)
        .expect("append message");

    store
        .mark_messages_distilled("ses-dist", &[1, 2])
        .expect("mark messages distilled");

    let history = store.get_history("ses-dist", None).expect("get history");
    assert_eq!(history.len(), 1, "distilled messages excluded from history");
    assert_eq!(
        history[0].content, "keep me",
        "the single undistilled message content should be 'keep me'"
    );

    let all_count: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = 'ses-dist'",
            [],
            |row| row.get(0),
        )
        .expect("query row");
    assert_eq!(all_count, 3, "distilled messages still in DB, just flagged");

    let distilled_count: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = 'ses-dist' AND is_distilled = 1",
            [],
            |row| row.get(0),
        )
        .expect("query row");
    assert_eq!(
        distilled_count, 2,
        "exactly 2 messages should be flagged as distilled in the DB"
    );
}

#[test]
fn session_timestamps_set() {
    let store = test_store();
    let session = store
        .create_session("ses-ts", "syn", "main", None, None)
        .expect("create session");

    assert!(
        !session.created_at.is_empty(),
        "created_at must be set on creation"
    );
    assert!(
        !session.updated_at.is_empty(),
        "updated_at must be set on creation"
    );
}

#[test]
fn blackboard_overwrite() {
    let store = test_store();
    store
        .blackboard_write("overwrite-key", "value-one", "syn", 3600)
        .expect("blackboard write");
    store
        .blackboard_write("overwrite-key", "value-two", "syn", 3600)
        .expect("blackboard write");

    let entry = store
        .blackboard_read("overwrite-key")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(entry.value, "value-two", "second write must win");

    let list = store.blackboard_list().expect("blackboard list");
    let matching: Vec<_> = list.iter().filter(|e| e.key == "overwrite-key").collect();
    assert_eq!(matching.len(), 1, "upsert must not create duplicates");
}

#[test]
fn note_list_multiple() {
    let store = test_store();
    store
        .create_session("ses-notes", "syn", "main", None, None)
        .expect("create session");

    store
        .add_note("ses-notes", "syn", "task", "note alpha")
        .expect("add note");
    store
        .add_note("ses-notes", "syn", "context", "note beta")
        .expect("add note");
    store
        .add_note("ses-notes", "syn", "decision", "note gamma")
        .expect("add note");

    let notes = store.get_notes("ses-notes").expect("add note");
    assert_eq!(notes.len(), 3, "should have 3 notes after adding 3");
    assert_eq!(
        notes[0].content, "note alpha",
        "first note content should be 'note alpha'"
    );
    assert_eq!(
        notes[1].content, "note beta",
        "second note content should be 'note beta'"
    );
    assert_eq!(
        notes[2].content, "note gamma",
        "third note content should be 'note gamma'"
    );
}

#[test]
fn update_display_name_sets_name() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert!(
        session.origin.display_name.is_none(),
        "display_name should be None before any update"
    );

    store
        .update_display_name("ses-1", "My Chat")
        .expect("find session by id");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.origin.display_name.as_deref(),
        Some("My Chat"),
        "display_name should be 'My Chat' after update"
    );
}

#[test]
fn display_name_round_trip_via_list() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    store
        .update_display_name("ses-1", "Research Chat")
        .expect("update display name");

    let sessions = store.list_sessions(Some("syn")).expect("list sessions");
    assert_eq!(
        sessions.len(),
        1,
        "list_sessions for 'syn' should return exactly one session"
    );
    assert_eq!(
        sessions[0].origin.display_name.as_deref(),
        Some("Research Chat"),
        "display_name should be returned by list_sessions after update"
    );
}

#[test]
fn update_display_name_overwrites_previous() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    store
        .update_display_name("ses-1", "First")
        .expect("create session");
    store
        .update_display_name("ses-1", "Second")
        .expect("update display name");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.origin.display_name.as_deref(),
        Some("Second"),
        "display_name should be 'Second' after overwriting 'First'"
    );
}
