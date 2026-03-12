//! Cross-crate integration tests for mneme `SessionStore`.

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![cfg(feature = "sqlite-tests")]

use aletheia_mneme::store::SessionStore;
use aletheia_mneme::types::Role;

fn test_store() -> SessionStore {
    SessionStore::open_in_memory().unwrap()
}

#[test]
fn create_session_and_append_messages() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .unwrap();

    store
        .append_message("ses-1", Role::User, "hello", None, None, 50)
        .unwrap();
    store
        .append_message("ses-1", Role::Assistant, "hi there", None, None, 60)
        .unwrap();

    let history = store.get_history("ses-1", None).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[0].content, "hello");
    assert_eq!(history[1].role, Role::Assistant);
    assert_eq!(history[1].content, "hi there");
}

#[test]
fn session_token_estimate_accumulates() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .unwrap();

    store
        .append_message("ses-1", Role::User, "a", None, None, 100)
        .unwrap();
    store
        .append_message("ses-1", Role::Assistant, "b", None, None, 200)
        .unwrap();
    store
        .append_message("ses-1", Role::User, "c", None, None, 150)
        .unwrap();

    let session = store.find_session_by_id("ses-1").unwrap().unwrap();
    assert_eq!(session.token_count_estimate, 450);
}

#[test]
fn history_excludes_distilled_messages() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .unwrap();

    store
        .append_message("ses-1", Role::User, "old-1", None, None, 100)
        .unwrap();
    store
        .append_message("ses-1", Role::Assistant, "old-2", None, None, 100)
        .unwrap();
    store
        .append_message("ses-1", Role::User, "keep-me", None, None, 100)
        .unwrap();

    store.mark_messages_distilled("ses-1", &[1, 2]).unwrap();

    let history = store.get_history("ses-1", None).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content, "keep-me");
}

#[test]
fn history_budget_returns_most_recent() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .unwrap();

    for i in 1..=5 {
        store
            .append_message("ses-1", Role::User, &format!("msg-{i}"), None, None, 100)
            .unwrap();
    }

    let history = store.get_history_with_budget("ses-1", 250).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "msg-4");
    assert_eq!(history[1].content, "msg-5");
}
