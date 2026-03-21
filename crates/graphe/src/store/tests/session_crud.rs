//! Tests for session CRUD, messages, history, and usage recording.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
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
