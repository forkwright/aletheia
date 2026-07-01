//! Integration tests for the `SessionStore` public API.
//!
//! WHY: graphe had zero `crates/graphe/tests/` integration tests prior to
//! this. These tests run against the published API surface only — what an
//! external crate (e.g. mneme, nous) can actually use.
//!
//! Each test creates an isolated tempdir-backed fjall store so they can run
//! in parallel without sharing state.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "history Vecs have known length from preceding append assertions"
)]

use graphe::store::SessionStore;
use graphe::types::{Role, SessionStatus, SessionType};
#[cfg(feature = "portability")]
use graphe::types::{Session, SessionMetrics, SessionOrigin};
use koina::ulid::Ulid;
use tempfile::TempDir;

/// Open a fresh `SessionStore` in a tempdir. The `TempDir` must be kept alive
/// for the duration of the test or its files will be cleaned up.
fn fresh_store() -> (SessionStore, TempDir) {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("session");
    let store = SessionStore::open(&path).expect("session store opens");
    (store, dir)
}

#[test]
fn open_creates_store_directory() {
    let (_store, dir) = fresh_store();
    let db_path = dir.path().join("session");
    assert!(
        db_path.exists(),
        "open() must create the store directory at the requested path"
    );
}

#[test]
fn create_session_returns_populated_record() {
    let (store, _dir) = fresh_store();
    let id = Ulid::new().to_string();
    let session = store
        .create_session(
            &id,
            "nous-int-test",
            "primary",
            None,
            Some("claude-sonnet-4-6"),
        )
        .expect("create_session succeeds");

    assert_eq!(session.id, id, "id should round-trip");
    assert_eq!(session.nous_id, "nous-int-test");
    assert_eq!(session.session_key, "primary");
    assert_eq!(session.model.as_deref(), Some("claude-sonnet-4-6"));
    assert_eq!(session.status, SessionStatus::Active);
    assert_eq!(
        session.metrics.message_count, 0,
        "fresh session has zero messages"
    );
}

#[test]
fn create_session_unique_per_nous_session_key() {
    // WHY: the (nous_id, session_key) pair has a UNIQUE constraint —
    // attempting a second create with the same pair must fail rather
    // than silently shadowing the existing row.
    let (store, _dir) = fresh_store();
    let id1 = Ulid::new().to_string();
    let id2 = Ulid::new().to_string();
    store
        .create_session(&id1, "nous-int-test", "primary", None, None)
        .expect("first create succeeds");
    let result = store.create_session(&id2, "nous-int-test", "primary", None, None);
    assert!(
        result.is_err(),
        "duplicate (nous_id, session_key) must fail, got: {result:?}"
    );
}

#[test]
fn append_message_assigns_sequential_seq_numbers() {
    let (store, _dir) = fresh_store();
    let session_id = Ulid::new().to_string();
    store
        .create_session(&session_id, "nous-int-test", "primary", None, None)
        .expect("session create");

    let s1 = store
        .append_message(&session_id, Role::User, "first", None, None, 5)
        .expect("first append");
    let s2 = store
        .append_message(&session_id, Role::Assistant, "second", None, None, 10)
        .expect("second append");
    let s3 = store
        .append_message(&session_id, Role::User, "third", None, None, 3)
        .expect("third append");

    assert_eq!(s1, 1, "first message gets seq=1");
    assert_eq!(s2, 2, "second message gets seq=2");
    assert_eq!(s3, 3, "third message gets seq=3");
}

#[test]
fn append_message_updates_session_token_count() {
    // WHY: append_message must atomically update sessions.token_count_estimate
    // (in the same transaction). Verify the running total reflects every append.
    let (store, _dir) = fresh_store();
    let session_id = Ulid::new().to_string();
    store
        .create_session(&session_id, "nous-int-test", "primary", None, None)
        .expect("session create");

    store
        .append_message(&session_id, Role::User, "hi", None, None, 7)
        .expect("append");
    store
        .append_message(&session_id, Role::Assistant, "hello", None, None, 13)
        .expect("append");

    let session = store
        .find_session_by_id(&session_id)
        .expect("query")
        .expect("session exists");
    assert_eq!(
        session.metrics.token_count_estimate, 20,
        "running total should be 7 + 13 = 20"
    );
    assert_eq!(session.metrics.message_count, 2);
}

#[test]
fn get_history_returns_messages_in_seq_order() {
    let (store, _dir) = fresh_store();
    let session_id = Ulid::new().to_string();
    store
        .create_session(&session_id, "nous-int-test", "primary", None, None)
        .expect("session create");

    store
        .append_message(&session_id, Role::User, "one", None, None, 1)
        .expect("append");
    store
        .append_message(&session_id, Role::Assistant, "two", None, None, 1)
        .expect("append");
    store
        .append_message(&session_id, Role::User, "three", None, None, 1)
        .expect("append");

    let history = store
        .get_history(&session_id, None)
        .expect("get_history succeeds");
    assert_eq!(history.len(), 3, "all 3 messages should return");
    assert_eq!(history[0].content, "one");
    assert_eq!(history[1].content, "two");
    assert_eq!(history[2].content, "three");
}

#[test]
fn get_history_respects_limit() {
    let (store, _dir) = fresh_store();
    let session_id = Ulid::new().to_string();
    store
        .create_session(&session_id, "nous-int-test", "primary", None, None)
        .expect("session create");
    for i in 0..10 {
        let content = format!("message {i}");
        store
            .append_message(&session_id, Role::User, &content, None, None, 1)
            .expect("append");
    }

    let history = store
        .get_history(&session_id, Some(3))
        .expect("get_history succeeds");
    assert_eq!(history.len(), 3, "limit=3 should return 3 messages");
}

#[test]
fn find_or_create_session_creates_when_absent() {
    let (store, _dir) = fresh_store();
    let id = Ulid::new().to_string();
    let session = store
        .find_or_create_session(&id, "nous-int-test", "primary", None, None)
        .expect("find_or_create succeeds");
    assert_eq!(session.id, id);
    assert_eq!(session.metrics.message_count, 0);
}

#[test]
fn find_or_create_session_returns_existing_when_present() {
    // WHY: find_or_create must idempotently return the same session when
    // (nous_id, session_key) already exists, ignoring the proposed id.
    let (store, _dir) = fresh_store();
    let id_first = Ulid::new().to_string();
    let id_second = Ulid::new().to_string();

    let first = store
        .find_or_create_session(&id_first, "nous-int-test", "primary", None, None)
        .expect("first call");
    let second = store
        .find_or_create_session(&id_second, "nous-int-test", "primary", None, None)
        .expect("second call");

    assert_eq!(
        first.id, second.id,
        "second find_or_create should return the existing session id, not the proposed one"
    );
    assert_eq!(first.id, id_first);
}

#[test]
fn list_sessions_filters_by_nous_id() {
    let (store, _dir) = fresh_store();
    let id_a = Ulid::new().to_string();
    let id_b = Ulid::new().to_string();
    store
        .create_session(&id_a, "nous-a", "primary", None, None)
        .expect("create a");
    store
        .create_session(&id_b, "nous-b", "primary", None, None)
        .expect("create b");

    let nous_a_sessions = store.list_sessions(Some("nous-a")).expect("list nous-a");
    assert_eq!(nous_a_sessions.len(), 1);
    assert_eq!(nous_a_sessions[0].id, id_a);

    let all = store.list_sessions(None).expect("list all");
    assert!(all.len() >= 2, "list_sessions(None) should return both");
}

#[test]
fn delete_session_removes_empty_session() {
    // WHY: baseline — deleting a childless session must succeed.
    let (store, _dir) = fresh_store();
    let session_id = Ulid::new().to_string();
    store
        .create_session(&session_id, "nous-int-test", "primary", None, None)
        .expect("session create");

    let deleted = store.delete_session(&session_id).expect("delete");
    assert!(deleted, "delete should report success");
    let after = store.find_session_by_id(&session_id).expect("query");
    assert!(
        after.is_none(),
        "session should not be findable after delete"
    );
}

#[test]
fn delete_session_removes_session_and_messages() {
    // WHY(#2959): regression — delete_session must remove child records
    // (messages, usage, distillations, notes) in the same transaction;
    // nothing in the store cascades deletes automatically.
    let (store, _dir) = fresh_store();
    let session_id = Ulid::new().to_string();
    store
        .create_session(&session_id, "nous-int-test", "primary", None, None)
        .expect("session create");
    store
        .append_message(&session_id, Role::User, "hi", None, None, 1)
        .expect("append");

    let deleted = store.delete_session(&session_id).expect("delete");
    assert!(deleted, "delete should report success");
    let after = store.find_session_by_id(&session_id).expect("query");
    assert!(
        after.is_none(),
        "session should not be findable after delete"
    );
    // WHY: also verify no orphan messages remain.
    let history = store.get_history(&session_id, None).expect("history query");
    assert!(
        history.is_empty(),
        "messages should be deleted along with the session"
    );
}

#[test]
fn ping_returns_ok_on_healthy_store() {
    let (store, _dir) = fresh_store();
    store.ping().expect("ping should succeed on healthy store");
}

#[test]
fn open_in_memory_creates_isolated_stores() {
    // WHY: each open_in_memory call must yield an independent database —
    // tests use this to avoid disk I/O while keeping isolation.
    let store_a = SessionStore::open_in_memory().expect("first store");
    let store_b = SessionStore::open_in_memory().expect("second store");

    let id = Ulid::new().to_string();
    store_a
        .create_session(&id, "nous-mem", "primary", None, None)
        .expect("create in store_a");

    let in_b = store_b.find_session_by_id(&id).expect("query store_b");
    assert!(
        in_b.is_none(),
        "store_b should not see sessions created in store_a"
    );
}

#[test]
fn create_session_key_does_not_control_session_type() {
    // WHY: session keys are identifiers; lifecycle retention is caller-owned
    // metadata and must not change because a user-visible key matches an
    // internal-looking prefix.
    let (store, _dir) = fresh_store();

    for (idx, key) in [
        "prosoche-wake",
        "daemon:prosoche",
        "ask:demiurge",
        "spawn:coder",
        "dispatch:task",
        "ephemeral:one-off",
    ]
    .iter()
    .enumerate()
    {
        let id = format!("ses-{idx}");
        let session = store
            .create_session(&id, "nous-int-test", key, None, None)
            .expect("create_session succeeds");
        assert_eq!(
            session.session_type,
            SessionType::Primary,
            "key '{key}' must not change the default primary lifecycle"
        );
    }
}

#[test]
fn create_session_with_type_uses_explicit_session_type() {
    let (store, _dir) = fresh_store();

    let background = store
        .create_session_with_type(
            "ses-background",
            "nous-int-test",
            "main",
            SessionType::Background,
            None,
            None,
        )
        .expect("create background session succeeds");
    assert_eq!(background.session_type, SessionType::Background);

    let ephemeral = store
        .find_or_create_session_with_type(
            "ses-ephemeral",
            "nous-int-test",
            "regular-session",
            SessionType::Ephemeral,
            None,
            None,
        )
        .expect("find_or_create ephemeral session succeeds");
    assert_eq!(ephemeral.session_type, SessionType::Ephemeral);
}

#[cfg(feature = "portability")]
#[test]
fn import_session_preserves_session_type() {
    // WHY: exported/restored sessions must keep their original lifecycle
    // class — a daemon session imported on a new node must not become a
    // primary conversation just because the key was recreated locally.
    let (store, _dir) = fresh_store();

    for (idx, stype) in [
        SessionType::Primary,
        SessionType::Background,
        SessionType::Ephemeral,
    ]
    .iter()
    .enumerate()
    {
        let id = format!("ses-imp-{idx}");
        let key = format!("key-{id}");
        let session = Session {
            id: id.clone(),
            nous_id: "nous-int-test".to_owned(),
            session_key: key.clone(),
            status: SessionStatus::Active,
            model: None,
            session_type: *stype,
            created_at: "2024-01-01T00:00:00Z".to_owned(),
            updated_at: "2024-01-01T00:00:00Z".to_owned(),
            metrics: SessionMetrics {
                token_count_estimate: 0,
                message_count: 0,
                last_input_tokens: 0,
                bootstrap_hash: None,
                distillation_count: 0,
                last_distilled_at: None,
                computed_context_tokens: 0,
            },
            origin: SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: None,
                display_name: None,
            },
            artefact_meta: None,
        };

        let imported = store
            .import_session(&session, false)
            .expect("import succeeds");
        assert_eq!(
            imported.session_type, *stype,
            "imported session {id} must retain {stype:?}"
        );

        let roundtrip = store
            .find_session_by_id(&id)
            .expect("query succeeds")
            .expect("session exists after import");
        assert_eq!(
            roundtrip.session_type, *stype,
            "re-read session {id} must retain {stype:?}"
        );
    }
}
