//! Tests for the session store.
#![expect(clippy::expect_used, reason = "test assertions")]

use super::SessionStore;
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
    assert_eq!(session.id, "ses-1");
    assert_eq!(session.nous_id, "syn");
    assert_eq!(session.session_key, "main");
    assert_eq!(session.status, SessionStatus::Active);
    assert_eq!(session.session_type, SessionType::Primary);

    let found = store.find_session("syn", "main").expect("find session");
    assert!(found.is_some());
    assert_eq!(found.expect("session must exist").id, "ses-1");
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
fn session_type_classification() {
    let store = test_store();

    let s1 = store
        .create_session("ses-bg", "syn", "prosoche-wake", None, None)
        .expect("create session");
    assert_eq!(s1.session_type, SessionType::Background);

    let s2 = store
        .create_session("ses-eph", "syn", "ask:demiurge", None, None)
        .expect("create session");
    assert_eq!(s2.session_type, SessionType::Ephemeral);

    let s3 = store
        .create_session("ses-pri", "syn", "main", None, None)
        .expect("create session");
    assert_eq!(s3.session_type, SessionType::Primary);
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
    assert_eq!(session.id, "ses-1"); // Reactivated, not created new
    assert_eq!(session.status, SessionStatus::Active);
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

    assert_eq!(seq1, 1);
    assert_eq!(seq2, 2);

    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "hello");
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[1].content, "hi there");
    assert_eq!(history[1].role, Role::Assistant);
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
    assert_eq!(session.metrics.message_count, 2);
    assert_eq!(session.metrics.token_count_estimate, 300);
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
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "msg 4");
    assert_eq!(history[1].content, "msg 5");
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
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "mid");
    assert_eq!(history[1].content, "new");
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

    // Distill the first two messages
    store
        .mark_messages_distilled("ses-1", &[1, 2])
        .expect("mark messages distilled");

    // History should only return undistilled
    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content, "keep this");

    // Session counts should be recalculated
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.metrics.message_count, 1);
    assert_eq!(session.metrics.token_count_estimate, 50);
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

    // Verify it was stored
    let count: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM usage WHERE session_id = ?1",
            ["ses-1"],
            |row| row.get(0),
        )
        .expect("query usage count");
    assert_eq!(count, 1);
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
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].content, "working on M0b");
    assert_eq!(notes[1].content, "use snafu for errors");

    store.delete_note(id1).expect("delete note");
    let notes = store.get_notes("ses-1").expect("get notes");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].id, id2);
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
    assert_eq!(all.len(), 2);

    let syn_only = store.list_sessions(Some("syn")).expect("list sessions");
    assert_eq!(syn_only.len(), 1);
    assert_eq!(syn_only[0].nous_id, "syn");
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
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].role, Role::ToolResult);
    assert_eq!(history[0].tool_call_id.as_deref(), Some("tool_123"));
    assert_eq!(history[0].tool_name.as_deref(), Some("exec"));
}

// --- Edge cases ---

#[test]
fn history_empty_session() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    let history = store.get_history("ses-1", None).expect("get history");
    assert!(history.is_empty());
}

#[test]
fn history_limit_one() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    for i in 1..=5 {
        store
            .append_message("ses-1", Role::User, &format!("msg {i}"), None, None, 10)
            .expect("append message");
    }
    let history = store.get_history("ses-1", Some(1)).expect("get history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content, "msg 5");
}

#[test]
fn history_limit_exceeds_count() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    store
        .append_message("ses-1", Role::User, "only", None, None, 10)
        .expect("append message");
    let history = store.get_history("ses-1", Some(100)).expect("get history");
    assert_eq!(history.len(), 1);
}

#[test]
fn large_message_content() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    let big = "x".repeat(1_000_000);
    store
        .append_message("ses-1", Role::User, &big, None, None, 250_000)
        .expect("append message");
    let history = store.get_history("ses-1", None).expect("get history");
    assert_eq!(history[0].content.len(), 1_000_000);
}

#[test]
fn distill_empty_seqs_is_noop() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    store
        .append_message("ses-1", Role::User, "keep", None, None, 10)
        .expect("append message");
    store
        .mark_messages_distilled("ses-1", &[])
        .expect("mark messages distilled");
    let history = store.get_history("ses-1", None).expect("append message");
    assert_eq!(history.len(), 1);
}

#[test]
fn delete_nonexistent_note_returns_false() {
    let store = test_store();
    let deleted = store.delete_note(9999).expect("delete note");
    assert!(!deleted);
}

#[test]
fn message_sequence_always_increases() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    let s1 = store
        .append_message("ses-1", Role::User, "a", None, None, 5)
        .expect("append message");
    let s2 = store
        .append_message("ses-1", Role::Assistant, "b", None, None, 5)
        .expect("append message");
    let s3 = store
        .append_message("ses-1", Role::User, "c", None, None, 5)
        .expect("append message");
    assert!(s1 < s2);
    assert!(s2 < s3);
}

#[test]
fn budget_always_includes_at_least_one() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    store
        .append_message("ses-1", Role::User, "big", None, None, 999_999)
        .expect("append message");
    let history = store
        .get_history_with_budget("ses-1", 1)
        .expect("get history with budget");
    assert_eq!(history.len(), 1);
}

#[test]
fn budget_loads_only_fitting_messages() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    // Insert 50 messages, each with 100 token estimate (total = 5000 tokens).
    for i in 1..=50 {
        store
            .append_message(
                "ses-1",
                Role::User,
                &format!("message {i}"),
                None,
                None,
                100,
            )
            .expect("append message");
    }

    // Budget of 500 fits exactly 5 messages.
    let history = store
        .get_history_with_budget("ses-1", 500)
        .expect("get history with budget");
    assert_eq!(
        history.len(),
        5,
        "budget of 500 should fit 5 messages at 100 tokens each"
    );
    assert_eq!(history[0].content, "message 46");
    assert_eq!(history[4].content, "message 50");

    // Budget that fits all messages returns everything.
    let all = store
        .get_history_with_budget("ses-1", 10_000)
        .expect("get history with budget");
    assert_eq!(
        all.len(),
        50,
        "budget exceeding total should return all messages"
    );
    assert_eq!(all[0].content, "message 1");
    assert_eq!(all[49].content, "message 50");
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
    assert_eq!(entry.key, "goal");
    assert_eq!(entry.value, "finish M0b");
    assert_eq!(entry.author_nous_id, "syn");

    let list = store.blackboard_list().expect("blackboard list");
    assert_eq!(list.len(), 1);

    let deleted = store
        .blackboard_delete("goal", "syn")
        .expect("blackboard delete");
    assert!(deleted);

    let gone = store.blackboard_read("goal").expect("blackboard delete");
    assert!(gone.is_none());
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
    assert_eq!(entry.value, "running");

    let list = store.blackboard_list().expect("blackboard list");
    assert_eq!(list.len(), 1);
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
    assert!(!deleted);

    let still_there = store.blackboard_read("secret").expect("blackboard delete");
    assert!(still_there.is_some());
}

#[test]
fn blackboard_read_missing_returns_none() {
    let store = test_store();
    let result = store
        .blackboard_read("nonexistent")
        .expect("blackboard read");
    assert!(result.is_none());
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
    assert!(result.is_none());

    let list = store.blackboard_list().expect("blackboard list");
    assert!(list.is_empty());
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
    assert_eq!(session.metrics.distillation_count, 0);
    assert!(session.metrics.last_distilled_at.is_none());

    store
        .record_distillation("ses-1", 20, 5, 50000, 2000, Some("sonnet"))
        .expect("record distillation");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.metrics.distillation_count, 1);
    assert!(session.metrics.last_distilled_at.is_some());

    store
        .record_distillation("ses-1", 15, 3, 30000, 1500, None)
        .expect("record distillation");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.metrics.distillation_count, 2);
}

#[test]
fn open_in_memory_creates_tables() {
    let store = test_store();
    let sessions = store.list_sessions(None).expect("list sessions");
    assert!(sessions.is_empty());
    let session = store
        .create_session("tbl-check", "syn", "main", None, None)
        .expect("create session");
    assert_eq!(session.id, "tbl-check");
    let found = store
        .find_session_by_id("tbl-check")
        .expect("create session");
    assert!(found.is_some());
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
    assert!(found.is_none());
}

#[test]
fn find_session_by_id_nonexistent() {
    let store = test_store();
    let found = store
        .find_session_by_id("non-existent-id-999")
        .expect("find session by id");
    assert!(found.is_none());
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

#[test]
fn get_history_empty_session() {
    let store = test_store();
    store
        .create_session("empty-ses", "syn", "main", None, None)
        .expect("create session");
    let history = store.get_history("empty-ses", None).expect("get history");
    assert!(history.is_empty());
    let history_limited = store
        .get_history("empty-ses", Some(10))
        .expect("get history");
    assert!(history_limited.is_empty());
}

#[test]
fn blackboard_write_read_delete_cycle() {
    let store = test_store();

    let read_before = store
        .blackboard_read("cycle-key")
        .expect("blackboard write");
    assert!(read_before.is_none());

    store
        .blackboard_write("cycle-key", "value-1", "agent-alice", 7200)
        .expect("blackboard write");
    let entry = store
        .blackboard_read("cycle-key")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(entry.value, "value-1");
    assert_eq!(entry.author_nous_id, "agent-alice");
    assert_eq!(entry.ttl_seconds, 7200);

    store
        .blackboard_write("cycle-key", "value-2", "agent-alice", 3600)
        .expect("blackboard write");
    let updated = store
        .blackboard_read("cycle-key")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(updated.value, "value-2");
    assert_eq!(updated.ttl_seconds, 3600);

    let deleted = store
        .blackboard_delete("cycle-key", "agent-alice")
        .expect("blackboard delete");
    assert!(deleted);

    let after_delete = store
        .blackboard_read("cycle-key")
        .expect("blackboard delete");
    assert!(after_delete.is_none());

    let list = store.blackboard_list().expect("blackboard list");
    assert!(list.is_empty());
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
    assert_eq!(session.status, SessionStatus::Active);

    store
        .update_session_status("ses-status", SessionStatus::Archived)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-status")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.status, SessionStatus::Archived);

    store
        .update_session_status("ses-status", SessionStatus::Distilled)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-status")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.status, SessionStatus::Distilled);

    store
        .update_session_status("ses-status", SessionStatus::Active)
        .expect("update session status");
    let session = store
        .find_session_by_id("ses-status")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.status, SessionStatus::Active);
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
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role, Role::System);
    assert!(history[0].content.contains("Distillation #1"));
    assert_eq!(history[1].content, "msg3");

    // Session counts should reflect new state
    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.metrics.message_count, 2);
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
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].role, Role::System);
    assert!(history[0].content.contains("Summary #1"));
    // Remaining messages preserve their original seq ordering.
    assert_eq!(history[1].content, "msg3");
    assert_eq!(history[2].content, "msg4");
    assert_eq!(history[3].content, "msg5");
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
    assert_eq!(history.len(), 3);
    let summary_seq = history[0].seq;
    let msg3_seq = history[1].seq;
    assert_eq!(history[0].role, Role::System);
    assert_eq!(history[1].content, "msg3");
    assert_eq!(history[2].content, "msg4");

    // Second distillation: condense the previous summary and msg3.
    store
        .mark_messages_distilled("ses-2d", &[summary_seq, msg3_seq])
        .expect("mark messages distilled");

    // This must not conflict with the old summary that is still at seq 0 in the DB.
    store
        .insert_distillation_summary("ses-2d", "Summary #2")
        .expect("second distillation must not conflict with old seq-0 summary");

    let history = store.get_history("ses-2d", None).expect("get history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role, Role::System);
    assert!(history[0].content.contains("Summary #2"));
    assert_eq!(history[1].content, "msg4");
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
    assert_eq!(count, 2);
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
    assert_eq!(history_3.len(), 3);
    assert_eq!(history_3[0].content, "message 8");
    assert_eq!(history_3[2].content, "message 10");

    let history_all = store.get_history("ses-lim", None).expect("get history");
    assert_eq!(history_all.len(), 10);
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
    assert_eq!(sessions.len(), 3);

    let keys: Vec<&str> = sessions.iter().map(|s| s.session_key.as_str()).collect();
    assert!(keys.contains(&"main"));
    assert!(keys.contains(&"secondary"));
    assert!(keys.contains(&"prosoche-wake"));
}

#[test]
fn blackboard_read_nonexistent_key() {
    let store = test_store();
    let result = store
        .blackboard_read("does-not-exist-key")
        .expect("blackboard read");
    assert!(result.is_none());
}

#[test]
fn list_notes_empty() {
    let store = test_store();
    store
        .create_session("ses-no-notes", "syn", "main", None, None)
        .expect("create session");

    let notes = store.get_notes("ses-no-notes").expect("create session");
    assert!(notes.is_empty());
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
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].content, "first");
    assert_eq!(history[1].content, "second");
    assert_eq!(history[2].content, "third");
    assert!(history[0].seq < history[1].seq);
    assert!(history[1].seq < history[2].seq);
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
    assert_eq!(history[0].content, "keep me");

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
    assert_eq!(distilled_count, 2);
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
    assert_eq!(notes.len(), 3);
    assert_eq!(notes[0].content, "note alpha");
    assert_eq!(notes[1].content, "note beta");
    assert_eq!(notes[2].content, "note gamma");
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
    assert!(session.origin.display_name.is_none());

    store
        .update_display_name("ses-1", "My Chat")
        .expect("find session by id");

    let session = store
        .find_session_by_id("ses-1")
        .expect("query succeeds")
        .expect("entry must exist");
    assert_eq!(session.origin.display_name.as_deref(), Some("My Chat"));
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
    assert_eq!(sessions.len(), 1);
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
    assert_eq!(session.origin.display_name.as_deref(), Some("Second"));
}
