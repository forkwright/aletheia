//! Edge case tests for history, budget, distillation, and notes.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use super::super::SessionStore;
use crate::types::Role;

fn test_store() -> SessionStore {
    SessionStore::open_in_memory().expect("open in-memory store")
}

#[test]
fn history_empty_session() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");
    let history = store.get_history("ses-1", None).expect("get history");
    assert!(
        history.is_empty(),
        "newly created session should have no messages"
    );
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
    assert_eq!(
        history.len(),
        1,
        "limit of 1 should return exactly one message"
    );
    assert_eq!(
        history[0].content, "msg 5",
        "with limit 1, the most recent message should be returned"
    );
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
    assert_eq!(
        history.len(),
        1,
        "limit exceeding message count should return all available messages"
    );
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
    assert_eq!(
        history[0].content.len(),
        1_000_000,
        "large message content should be stored and retrieved without truncation"
    );
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
    assert_eq!(
        history.len(),
        1,
        "distilling empty slice should not remove any messages"
    );
}

#[test]
fn delete_nonexistent_note_returns_false() {
    let store = test_store();
    let deleted = store.delete_note(9999).expect("delete note");
    assert!(
        !deleted,
        "deleting a nonexistent note id should return false"
    );
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
    assert!(
        s1 < s2,
        "sequence numbers must increase monotonically: s1 < s2"
    );
    assert!(
        s2 < s3,
        "sequence numbers must increase monotonically: s2 < s3"
    );
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
    assert_eq!(
        history.len(),
        1,
        "budget that is smaller than any single message should still return at least one message"
    );
}

#[test]
fn budget_loads_only_fitting_messages() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

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

    let history = store
        .get_history_with_budget("ses-1", 500)
        .expect("get history with budget");
    assert_eq!(
        history.len(),
        5,
        "budget of 500 should fit 5 messages at 100 tokens each"
    );
    assert_eq!(
        history[0].content, "message 46",
        "first message in budget window should be message 46"
    );
    assert_eq!(
        history[4].content, "message 50",
        "last message in budget window should be message 50"
    );

    let all = store
        .get_history_with_budget("ses-1", 10_000)
        .expect("get history with budget");
    assert_eq!(
        all.len(),
        50,
        "budget exceeding total should return all messages"
    );
    assert_eq!(
        all[0].content, "message 1",
        "first message when budget covers all should be message 1"
    );
    assert_eq!(
        all[49].content, "message 50",
        "last message when budget covers all should be message 50"
    );
}
