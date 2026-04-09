//! Tests for session CRUD, messages, history, and usage recording.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
#![expect(
    clippy::similar_names,
    reason = "seq_a1/seq_b1/seq_a2/seq_b2 mirror paired-session assertions"
)]
use super::super::SessionStore;
use crate::types::{Role, SessionStatus, SessionType, UsageRecord};

fn test_store() -> SessionStore {
    SessionStore::open_in_memory().expect("open in-memory store")
}

#[test]
fn create_and_find_session() {
    let store = test_store();
    let session = store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    assert_eq!(
        session.id, "ses-1",
        "session id should match the id passed to create_session"
    );
    assert_eq!(
        session.nous_id, "syn",
        "session nous_id should match the nous passed to create_session"
    );
    assert_eq!(
        session.session_key, "main",
        "session key should match the key passed to create_session"
    );
    assert_eq!(
        session.status,
        SessionStatus::Active,
        "newly created session should be active"
    );
    assert_eq!(
        session.session_type,
        SessionType::Primary,
        "session key 'main' should classify as Primary"
    );

    let found = store.find_session("syn", "main").expect("find session");
    assert!(found.is_some(), "session ses-1 should exist after creation");
    assert_eq!(
        found.expect("session must exist").id,
        "ses-1",
        "found session should have the expected id"
    );
}

#[test]
fn find_session_returns_none_for_missing() {
    let store = test_store();
    let found = store
        .find_session("syn", "nonexistent")
        .expect("find session");
    assert!(found.is_none(), "nonexistent session should return None");
}

#[test]
fn session_type_classification() {
    let store = test_store();

    let s1 = store
        .create_session("ses-bg", "syn", "prosoche-wake", None, None)
        .expect("create session");
    assert_eq!(
        s1.session_type,
        SessionType::Background,
        "session key 'prosoche-wake' should classify as Background"
    );

    let s2 = store
        .create_session("ses-eph", "syn", "ask:demiurge", None, None)
        .expect("create session");
    assert_eq!(
        s2.session_type,
        SessionType::Ephemeral,
        "session key 'ask:demiurge' should classify as Ephemeral"
    );

    let s3 = store
        .create_session("ses-pri", "syn", "main", None, None)
        .expect("create session");
    assert_eq!(
        s3.session_type,
        SessionType::Primary,
        "session key 'main' should classify as Primary"
    );
}

#[test]
fn find_or_create_reactivates_archived() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    store
        .update_session_status("ses-1", SessionStatus::Archived)
        .expect("update session status");

    let session = store
        .find_or_create_session("ses-new", "syn", "main", None, None)
        .expect("create session");
    assert_eq!(
        session.id, "ses-1",
        "find_or_create should reactivate the archived session, not create a new one"
    );
    assert_eq!(
        session.status,
        SessionStatus::Active,
        "reactivated session should have Active status"
    );
}

#[test]
fn append_and_retrieve_messages() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    let seq1 = store
        .append_message("ses-1", Role::User, "hello", None, None, 10)
        .expect("append message");
    let seq2 = store
        .append_message("ses-1", Role::Assistant, "hi there", None, None, 15)
        .expect("append message");

    assert_eq!(seq1, 1, "first appended message should have seq 1");
    assert_eq!(seq2, 2, "second appended message should have seq 2");

    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(
        history.len(),
        2,
        "history should contain both appended messages"
    );
    assert_eq!(
        history[0].content, "hello",
        "first message content should match"
    );
    assert_eq!(
        history[0].role,
        Role::User,
        "first message role should be User"
    );
    assert_eq!(
        history[1].content, "hi there",
        "second message content should match"
    );
    assert_eq!(
        history[1].role,
        Role::Assistant,
        "second message role should be Assistant"
    );
}

#[test]
fn message_updates_session_counts() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    store
        .append_message("ses-1", Role::User, "hello", None, None, 100)
        .expect("append message");
    store
        .append_message("ses-1", Role::Assistant, "world", None, None, 200)
        .expect("append message");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.metrics.message_count, 2,
        "session message_count should be 2 after two appended messages"
    );
    assert_eq!(
        session.metrics.token_count_estimate, 300,
        "session token_count_estimate should sum token counts of all messages"
    );
}

#[test]
fn history_with_limit() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    for i in 1..=5 {
        store
            .append_message("ses-1", Role::User, &format!("msg {i}"), None, None, 10)
            .expect("append message");
    }

    let history = store.get_history("ses-1", Some(2)).expect("get history");
    assert_eq!(
        history.len(),
        2,
        "limit of 2 should return exactly 2 messages"
    );
    assert_eq!(
        history[0].content, "msg 4",
        "with limit 2 from 5 messages, first result should be the 4th message"
    );
    assert_eq!(
        history[1].content, "msg 5",
        "with limit 2 from 5 messages, second result should be the 5th message"
    );
}

#[test]
fn history_with_budget() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    store
        .append_message("ses-1", Role::User, "old", None, None, 100)
        .expect("append message");
    store
        .append_message("ses-1", Role::User, "mid", None, None, 100)
        .expect("append message");
    store
        .append_message("ses-1", Role::User, "new", None, None, 100)
        .expect("append message");

    let history = store
        .get_history_with_budget("ses-1", 200)
        .expect("get history with budget");
    assert_eq!(
        history.len(),
        2,
        "budget of 200 should fit the 2 most recent messages at 100 tokens each"
    );
    assert_eq!(
        history[0].content, "mid",
        "first message within budget should be 'mid'"
    );
    assert_eq!(
        history[1].content, "new",
        "second message within budget should be 'new'"
    );
}

#[test]
fn distillation_marks_and_recalculates() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    store
        .append_message("ses-1", Role::User, "old msg 1", None, None, 100)
        .expect("append message");
    store
        .append_message("ses-1", Role::User, "old msg 2", None, None, 150)
        .expect("append message");
    store
        .append_message("ses-1", Role::User, "keep this", None, None, 50)
        .expect("append message");

    store
        .mark_messages_distilled("ses-1", &[1, 2])
        .expect("mark messages distilled");

    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(
        history.len(),
        1,
        "history should only contain undistilled messages"
    );
    assert_eq!(
        history[0].content, "keep this",
        "the undistilled message should be 'keep this'"
    );

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.metrics.message_count, 1,
        "message_count should reflect only undistilled messages"
    );
    assert_eq!(
        session.metrics.token_count_estimate, 50,
        "token_count_estimate should reflect only undistilled messages"
    );
}

#[test]
fn usage_recording() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    store
        .record_usage(&UsageRecord {
            session_id: "ses-1".to_owned(),
            turn_seq: 1,
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 800,
            cache_write_tokens: 200,
            model: Some("claude-opus-4-20250514".to_owned()),
        })
        .expect("record usage");

    let count: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM usage WHERE session_id = ?1",
            ["ses-1"],
            |row| row.get(0),
        )
        .expect("query usage count");
    assert_eq!(
        count, 1,
        "usage table should contain exactly one record after one record_usage call"
    );
}

#[test]
fn agent_notes_crud() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    let id1 = store
        .add_note("ses-1", "syn", "task", "working on M0b")
        .expect("add note");
    let id2 = store
        .add_note("ses-1", "syn", "decision", "use snafu for errors")
        .expect("add note");

    let notes = store.get_notes("ses-1").expect("add note");
    assert_eq!(notes.len(), 2, "should have 2 notes after adding 2");
    assert_eq!(
        notes[0].content, "working on M0b",
        "first note content should match"
    );
    assert_eq!(
        notes[1].content, "use snafu for errors",
        "second note content should match"
    );

    store.delete_note(id1).expect("delete note");
    let notes = store.get_notes("ses-1").expect("get notes");
    assert_eq!(notes.len(), 1, "should have 1 note after deleting one");
    assert_eq!(
        notes[0].id, id2,
        "remaining note should be the second one (id2)"
    );
}

#[test]
fn list_sessions_filtered() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    store
        .create_session("ses-2", "demiurge", "main", None, None)
        .expect("create session");

    let all = store.list_sessions(None).expect("create session");
    assert_eq!(
        all.len(),
        2,
        "listing all sessions should return both created sessions"
    );

    let syn_only = store.list_sessions(Some("syn")).expect("list sessions");
    assert_eq!(
        syn_only.len(),
        1,
        "filtering by nous_id 'syn' should return exactly one session"
    );
    assert_eq!(
        syn_only[0].nous_id, "syn",
        "the returned session should belong to 'syn'"
    );
}

#[test]
fn tool_result_message() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    store
        .append_message(
            "ses-1",
            Role::ToolResult,
            r#"{"result": "ok"}"#,
            Some("tool_123"),
            Some("exec"),
            50,
        )
        .expect("append message");

    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(
        history.len(),
        1,
        "history should contain the single tool result message"
    );
    assert_eq!(
        history[0].role,
        Role::ToolResult,
        "message role should be ToolResult"
    );
    assert_eq!(
        history[0].tool_call_id.as_deref(),
        Some("tool_123"),
        "tool_call_id should be preserved"
    );
    assert_eq!(
        history[0].tool_name.as_deref(),
        Some("exec"),
        "tool_name should be preserved"
    );
}

#[test]
fn update_display_name_changes_session_name() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // Initially display_name should be None
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.origin.display_name, None);

    // Update display name
    store
        .update_display_name("ses-1", "Project Planning Session")
        .expect("update display name");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.origin.display_name,
        Some("Project Planning Session".to_owned()),
        "display_name should be updated"
    );

    // Update again to verify it can be changed
    store
        .update_display_name("ses-1", "Renamed Session")
        .expect("update display name");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.origin.display_name,
        Some("Renamed Session".to_owned()),
        "display_name should be updated again"
    );
}

#[test]
fn delete_session_removes_session_and_messages() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");
    store
        .append_message("ses-1", Role::User, "hello", None, None, 10)
        .expect("append message");
    store
        .append_message("ses-1", Role::Assistant, "hi", None, None, 10)
        .expect("append message");

    // Verify session and messages exist
    let session = store.find_session_by_id("ses-1").expect("query succeeds");
    assert!(session.is_some(), "session should exist before deletion");
    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(history.len(), 2, "session should have 2 messages");

    // Delete messages first to avoid FK constraints from other tables
    // Then delete session
    store
        .conn
        .execute("DELETE FROM messages WHERE session_id = 'ses-1'", [])
        .expect("delete messages");
    let deleted = store.delete_session("ses-1").expect("delete session");
    assert!(deleted, "delete_session should return true for existing session");

    // Verify session is gone
    let session = store.find_session_by_id("ses-1").expect("query succeeds");
    assert!(session.is_none(), "session should not exist after deletion");
}

#[test]
fn delete_session_returns_false_for_nonexistent() {
    let store = test_store();
    let deleted = store.delete_session("nonexistent-session").expect("delete session");
    assert!(
        !deleted,
        "delete_session should return false for non-existent session"
    );
}

#[test]
fn get_history_filtered_with_before_seq() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    for i in 1..=5 {
        store
            .append_message("ses-1", Role::User, &format!("msg {i}"), None, None, 10)
            .expect("append message");
    }

    // Get history before seq 4 (should return msgs 1, 2, 3)
    let history = store
        .get_history_filtered("ses-1", None, Some(4))
        .expect("get history filtered");
    assert_eq!(history.len(), 3, "should return 3 messages before seq 4");
    assert_eq!(history[0].content, "msg 1", "first message should be msg 1");
    assert_eq!(history[1].content, "msg 2", "second message should be msg 2");
    assert_eq!(history[2].content, "msg 3", "third message should be msg 3");

    // Get history before seq 4 with limit 2
    let history = store
        .get_history_filtered("ses-1", Some(2), Some(4))
        .expect("get history filtered with limit");
    assert_eq!(history.len(), 2, "should return 2 messages with limit");
    assert_eq!(history[0].content, "msg 2", "first should be msg 2 (most recent 2 before 4)");
    assert_eq!(history[1].content, "msg 3", "second should be msg 3");
}

#[test]
fn get_history_filtered_empty_before_first_seq() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");
    store
        .append_message("ses-1", Role::User, "only message", None, None, 10)
        .expect("append message");

    // Get history before seq 1 (should return nothing since first seq is 1)
    let history = store
        .get_history_filtered("ses-1", None, Some(1))
        .expect("get history filtered");
    assert!(history.is_empty(), "should return no messages before seq 1");
}

#[test]
fn distillation_summary_operations() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // Initially no summary
    let summary = store
        .get_distillation_summary("ses-1")
        .expect("get distillation summary");
    assert_eq!(summary, None, "new session should have no distillation summary");

    // Add some messages and mark some as distilled
    store
        .append_message("ses-1", Role::User, "old message 1", None, None, 100)
        .expect("append message");
    store
        .append_message("ses-1", Role::User, "old message 2", None, None, 100)
        .expect("append message");
    store
        .append_message("ses-1", Role::Assistant, "keep this", None, None, 50)
        .expect("append message");

    store
        .mark_messages_distilled("ses-1", &[1, 2])
        .expect("mark messages distilled");

    // Insert distillation summary
    store
        .insert_distillation_summary("ses-1", "Summary of distilled messages")
        .expect("insert distillation summary");

    // Verify summary is stored
    let summary = store
        .get_distillation_summary("ses-1")
        .expect("get distillation summary");
    assert_eq!(
        summary,
        Some("Summary of distilled messages".to_owned()),
        "summary should be stored"
    );

    // Verify distilled messages are deleted and summary is added
    // History contains: seq=0 (summary) and seq=3 (undistilled message)
    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(history.len(), 2, "summary + undistilled message should remain");
    assert_eq!(history[0].role, Role::System, "first message should be system summary");
    assert_eq!(history[0].content, "Summary of distilled messages");
    assert_eq!(history[1].content, "keep this", "second message should be undistilled");

    // Verify session metrics are updated (summary + 1 message)
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.metrics.message_count, 2);
}

#[test]
fn insert_distillation_summary_replaces_existing() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");
    store
        .append_message("ses-1", Role::User, "msg", None, None, 10)
        .expect("append message");
    store
        .mark_messages_distilled("ses-1", &[1])
        .expect("mark messages distilled");

    // Insert first summary
    store
        .insert_distillation_summary("ses-1", "First summary")
        .expect("insert distillation summary");

    // Add more messages and distill again
    store
        .append_message("ses-1", Role::User, "msg2", None, None, 10)
        .expect("append message");
    store
        .append_message("ses-1", Role::User, "msg3", None, None, 10)
        .expect("append message");
    store
        .mark_messages_distilled("ses-1", &[2])
        .expect("mark messages distilled");

    // Insert second summary - should replace first
    store
        .insert_distillation_summary("ses-1", "Second summary")
        .expect("insert distillation summary");

    let summary = store
        .get_distillation_summary("ses-1")
        .expect("get distillation summary");
    assert_eq!(
        summary,
        Some("Second summary".to_owned()),
        "second summary should replace first"
    );
}

#[test]
fn record_distillation_updates_session_metrics() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // Record a distillation event
    store
        .record_distillation("ses-1", 10, 5, 1000, 500, Some("claude-3-opus"))
        .expect("record distillation");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.metrics.distillation_count, 1,
        "distillation_count should be incremented"
    );
    assert!(
        session.metrics.last_distilled_at.is_some(),
        "last_distilled_at should be set"
    );

    // Record another distillation
    store
        .record_distillation("ses-1", 5, 3, 500, 300, None)
        .expect("record distillation");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(
        session.metrics.distillation_count, 2,
        "distillation_count should be 2"
    );
}

#[test]
fn usage_exists_for_turn_detects_duplicates() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // Initially should not exist
    let exists = store
        .usage_exists_for_turn("ses-1", 1)
        .expect("check usage exists");
    assert!(!exists, "usage should not exist for turn 1 initially");

    // Record usage
    store
        .record_usage(&UsageRecord {
            session_id: "ses-1".to_owned(),
            turn_seq: 1,
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            model: Some("claude-3-haiku".to_owned()),
        })
        .expect("record usage");

    // Now should exist
    let exists = store
        .usage_exists_for_turn("ses-1", 1)
        .expect("check usage exists");
    assert!(exists, "usage should exist for turn 1 after recording");

    // Different turn should not exist
    let exists = store
        .usage_exists_for_turn("ses-1", 2)
        .expect("check usage exists");
    assert!(!exists, "usage should not exist for turn 2");

    // Different session should not exist
    let exists = store
        .usage_exists_for_turn("ses-other", 1)
        .expect("check usage exists");
    assert!(!exists, "usage should not exist for different session");
}

#[test]
fn create_session_with_parent_and_model() {
    let store = test_store();
    let session = store
        .create_session("ses-1", "alice", "main", Some("parent-123"), Some("claude-opus-4"))
        .expect("create session with parent and model");

    assert_eq!(session.id, "ses-1");
    assert_eq!(session.origin.parent_session_id, Some("parent-123".to_owned()));
    assert_eq!(session.model, Some("claude-opus-4".to_owned()));

    let found = store
        .find_session_by_id("ses-1")
        .expect("find session")
        .expect("session should exist");
    assert_eq!(found.origin.parent_session_id, Some("parent-123".to_owned()));
    assert_eq!(found.model, Some("claude-opus-4".to_owned()));
}

#[test]
fn find_session_by_id_not_found() {
    let store = test_store();
    let result = store.find_session_by_id("nonexistent-id").expect("query succeeds");
    assert!(result.is_none(), "find_session_by_id should return None for non-existent ID");
}

#[test]
fn list_sessions_returns_empty_for_nonexistent_nous() {
    let store = test_store();
    // Create a session for alice
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // List sessions for bob (who has no sessions)
    let sessions = store.list_sessions(Some("bob")).expect("list sessions");
    assert!(sessions.is_empty(), "should return empty vec for nous with no sessions");
}

#[test]
fn update_session_status_to_all_variants() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // Start as Active
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.status, SessionStatus::Active);

    // Update to Archived
    store
        .update_session_status("ses-1", SessionStatus::Archived)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.status, SessionStatus::Archived);

    // Update to Distilled
    store
        .update_session_status("ses-1", SessionStatus::Distilled)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.status, SessionStatus::Distilled);

    // Back to Active
    store
        .update_session_status("ses-1", SessionStatus::Active)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.status, SessionStatus::Active);
}

#[test]
fn session_timestamps_are_set() {
    let store = test_store();
    let session = store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // Verify timestamps are set and valid ISO 8601 format
    assert!(!session.created_at.is_empty(), "created_at should be set");
    assert!(!session.updated_at.is_empty(), "updated_at should be set");
    assert!(
        session.created_at.contains('T'),
        "created_at should be ISO 8601 format"
    );
    assert!(
        session.updated_at.contains('T'),
        "updated_at should be ISO 8601 format"
    );

    // updated_at should change after update
    let original_updated = session.updated_at.clone();
    std::thread::sleep(std::time::Duration::from_millis(10));
    store
        .update_display_name("ses-1", "New Name")
        .expect("update display name");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_ne!(
        session.updated_at, original_updated,
        "updated_at should change after update"
    );
}

#[test]
fn find_session_only_returns_active() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // Should find active session
    let found = store.find_session("alice", "main").expect("find session");
    assert!(found.is_some(), "should find active session");

    // Archive the session
    store
        .update_session_status("ses-1", SessionStatus::Archived)
        .expect("archive session");

    // Should not find archived session
    let found = store.find_session("alice", "main").expect("find session");
    assert!(found.is_none(), "should not find archived session with find_session");

    // But find_session_by_id should still find it
    let found = store.find_session_by_id("ses-1").expect("find session by id");
    assert!(found.is_some(), "find_session_by_id should find archived session");
}

#[test]
fn find_or_create_with_archived_session_reactivates() {
    let store = test_store();

    // Create and archive a session
    let session = store
        .create_session("ses-1", "bob", "main", None, None)
        .expect("create session");
    assert_eq!(session.status, SessionStatus::Active);

    store
        .update_session_status("ses-1", SessionStatus::Archived)
        .expect("archive session");

    // Find or create should reactivate the archived session
    let session = store
        .find_or_create_session("ses-new", "bob", "main", None, None)
        .expect("find or create session");
    assert_eq!(session.id, "ses-1", "should return same session");
    assert_eq!(session.status, SessionStatus::Active, "should reactivate archived session");
}

#[test]
fn multiple_sessions_same_nous_different_keys() {
    let store = test_store();

    // Create multiple sessions for same nous with different keys
    let s1 = store
        .create_session("ses-1", "acme.corp", "main", None, None)
        .expect("create session");
    let s2 = store
        .create_session("ses-2", "acme.corp", "secondary", None, None)
        .expect("create session");
    let s3 = store
        .create_session("ses-3", "acme.corp", "ephemeral:task1", None, None)
        .expect("create session");

    assert_eq!(s1.session_type, SessionType::Primary);
    assert_eq!(s2.session_type, SessionType::Primary);
    assert_eq!(s3.session_type, SessionType::Ephemeral);

    // List all sessions for acme.corp
    let sessions = store.list_sessions(Some("acme.corp")).expect("list sessions");
    assert_eq!(sessions.len(), 3, "should have 3 sessions");

    // Find each session by its key
    let found1 = store.find_session("acme.corp", "main").expect("find session");
    assert_eq!(found1.unwrap().id, "ses-1");

    let found2 = store.find_session("acme.corp", "secondary").expect("find session");
    assert_eq!(found2.unwrap().id, "ses-2");

    let found3 = store.find_session("acme.corp", "ephemeral:task1").expect("find session");
    assert_eq!(found3.unwrap().id, "ses-3");
}

#[test]
fn message_sequence_per_session_isolated() {
    let store = test_store();

    // Create two sessions
    store
        .create_session("ses-a", "alice", "main", None, None)
        .expect("create session");
    store
        .create_session("ses-b", "bob", "main", None, None)
        .expect("create session");

    // Add messages to both
    let seq_a1 = store
        .append_message("ses-a", Role::User, "alice msg 1", None, None, 10)
        .expect("append message");
    let seq_b1 = store
        .append_message("ses-b", Role::User, "bob msg 1", None, None, 10)
        .expect("append message");
    let seq_a2 = store
        .append_message("ses-a", Role::User, "alice msg 2", None, None, 10)
        .expect("append message");
    let seq_b2 = store
        .append_message("ses-b", Role::User, "bob msg 2", None, None, 10)
        .expect("append message");

    // Each session should have independent sequence numbers
    assert_eq!(seq_a1, 1);
    assert_eq!(seq_b1, 1);
    assert_eq!(seq_a2, 2);
    assert_eq!(seq_b2, 2);

    // Verify history is isolated
    let history_a = store.get_history("ses-a", None).expect("get history");
    assert_eq!(history_a.len(), 2);
    assert_eq!(history_a[0].content, "alice msg 1");
    assert_eq!(history_a[1].content, "alice msg 2");

    let history_b = store.get_history("ses-b", None).expect("get history");
    assert_eq!(history_b.len(), 2);
    assert_eq!(history_b[0].content, "bob msg 1");
    assert_eq!(history_b[1].content, "bob msg 2");
}

#[test]
fn session_notes_with_valid_categories() {
    let store = test_store();
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");

    // Add notes with all valid categories
    let note_task = store
        .add_note("ses-1", "alice", "task", "working on feature X")
        .expect("add task note");
    let note_decision = store
        .add_note("ses-1", "alice", "decision", "chose architecture Y")
        .expect("add decision note");
    let note_preference = store
        .add_note("ses-1", "alice", "preference", "prefer dark mode")
        .expect("add preference note");
    let note_correction = store
        .add_note("ses-1", "alice", "correction", "fixed bug in Z")
        .expect("add correction note");
    let note_context = store
        .add_note("ses-1", "alice", "context", "important context")
        .expect("add context note");

    let notes = store.get_notes("ses-1").expect("get notes");
    assert_eq!(notes.len(), 5, "should have 5 notes with valid categories");

    // Delete notes one by one
    store.delete_note(note_task).expect("delete task note");
    let notes = store.get_notes("ses-1").expect("get notes");
    assert_eq!(notes.len(), 4);

    store.delete_note(note_decision).expect("delete decision note");
    store.delete_note(note_preference).expect("delete preference note");
    store.delete_note(note_correction).expect("delete correction note");
    store.delete_note(note_context).expect("delete context note");

    let notes = store.get_notes("ses-1").expect("get notes");
    assert!(notes.is_empty(), "all notes should be deleted");
}

#[test]
fn list_sessions_ordering_by_updated_at() {
    let store = test_store();

    // Create sessions in order
    store
        .create_session("ses-1", "alice", "main", None, None)
        .expect("create session");
    std::thread::sleep(std::time::Duration::from_millis(10));
    store
        .create_session("ses-2", "alice", "secondary", None, None)
        .expect("create session");
    std::thread::sleep(std::time::Duration::from_millis(10));
    store
        .create_session("ses-3", "alice", "tertiary", None, None)
        .expect("create session");

    // Update ses-1 to make it most recent
    std::thread::sleep(std::time::Duration::from_millis(10));
    store
        .update_display_name("ses-1", "Updated")
        .expect("update display name");

    // List should be ordered by updated_at DESC (most recent first)
    let sessions = store.list_sessions(Some("alice")).expect("list sessions");
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[0].id, "ses-1", "most recently updated should be first");
    assert_eq!(sessions[1].id, "ses-3");
    assert_eq!(sessions[2].id, "ses-2");
}
