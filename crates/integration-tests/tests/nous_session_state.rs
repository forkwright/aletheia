//! Cross-crate tests for nous `SessionState` with mneme store.

#![expect(clippy::expect_used, reason = "test assertions")]
#![cfg(feature = "sqlite-tests")]

use aletheia_mneme::store::SessionStore;
use aletheia_mneme::types::Role;
use aletheia_nous::config::NousConfig;
use aletheia_nous::session::{SessionManager, SessionState};

fn test_config() -> NousConfig {
    NousConfig {
        id: "syn".to_owned(),
        ..NousConfig::default()
    }
}

#[test]
fn session_state_tracks_tokens_with_store() {
    let store = SessionStore::open_in_memory().expect("open in-memory session store");
    let config = test_config();
    let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);

    store
        .create_session("ses-1", "syn", "main", None, Some(&config.generation.model))
        .expect("create session");

    store
        .append_message("ses-1", Role::User, "hello", None, None, 100)
        .expect("append user message");
    state.token_estimate += 100;

    store
        .append_message("ses-1", Role::Assistant, "hi", None, None, 200)
        .expect("append assistant message");
    state.token_estimate += 200;

    let session = store
        .find_session_by_id("ses-1")
        .expect("find session")
        .expect("session must exist");
    assert_eq!(session.metrics.token_count_estimate, state.token_estimate);
}

#[test]
fn distillation_threshold_aligned() {
    let store = SessionStore::open_in_memory().expect("open in-memory session store");
    let config = test_config();
    let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);

    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create session");

    // Append enough tokens to cross the 90% threshold of 200k context window
    store
        .append_message("ses-1", Role::User, "big-msg", None, None, 180_001)
        .expect("append large message");
    state.token_estimate = 180_001;

    assert!(state.needs_distillation(0.9, 200_000));

    let session = store
        .find_session_by_id("ses-1")
        .expect("find session")
        .expect("session must exist");
    assert_eq!(session.metrics.token_count_estimate, state.token_estimate);
}

#[test]
fn session_manager_creates_compatible_state() {
    let config = test_config();
    let store = SessionStore::open_in_memory().expect("open in-memory session store");
    let mgr = SessionManager::new(config.clone());

    let state = mgr.create_session("ses-1", "main");
    let db_session = store
        .create_session(
            "ses-1",
            &state.nous_id,
            &state.session_key,
            None,
            Some(&state.model),
        )
        .expect("create session in store");

    assert_eq!(state.id, db_session.id);
    assert_eq!(state.nous_id, db_session.nous_id);
    assert_eq!(state.session_key, db_session.session_key);
}

#[test]
fn turn_counter_advances() {
    let config = test_config();
    let mut state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);

    assert_eq!(state.next_turn(), 1);
    assert_eq!(state.next_turn(), 2);
    assert_eq!(state.next_turn(), 3);
}
