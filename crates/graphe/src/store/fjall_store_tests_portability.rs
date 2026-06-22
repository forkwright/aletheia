#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on Vecs with asserted length"
)]

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
    let mut s = import_session_record("ses-imp3", SessionStatus::Active, "2024-04-01T00:00:00Z");
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
