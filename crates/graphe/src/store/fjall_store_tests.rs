#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on Vecs with asserted length"
)]

use super::{FinalizeMessage, FinalizeTurnRequest, test_persist_counter};
use crate::error::Error;
use crate::test_fixtures::test_store;
use crate::types::{BlackboardRow, Role, SessionStatus, UsageRecord};

fn write_raw(store: &super::SessionStore, partition_name: &str, key: &str, value: &[u8]) {
    let partition = store.partition(partition_name).expect("partition opens");
    let mut tx = store.db.write_tx();
    tx.insert(&partition, key, value);
    tx.commit().expect("raw value committed");
}

/// Run a closure with `TZ` temporarily set to `tz`, restoring the previous value
/// afterwards even if the closure panics.
///
/// WHY: jiff resolves the local timezone from `TZ` on first use, so tests that
/// exercise timezone-sensitive code must set the variable before opening the
/// store and must restore it to avoid polluting other tests.
#[expect(
    unsafe_code,
    reason = "WHY(#4742): single-threaded test helper; TZ env mutation is isolated by catch_unwind and unconditionally restored before return"
)]
fn with_tz<F, R>(tz: &str, f: F) -> R
where
    F: FnOnce() -> R,
{
    let original = std::env::var("TZ").ok();
    // SAFETY: tests are single-threaded; env mutation is acceptable in test
    // isolation.
    unsafe { std::env::set_var("TZ", tz) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    match original {
        Some(v) => unsafe { std::env::set_var("TZ", v) },
        None => unsafe { std::env::remove_var("TZ") },
    }
    result.expect("test did not panic")
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
fn blackboard_ttl_is_utc_under_non_utc_timezone() {
    // WHY(#4742): a TTL written on a non-UTC host must expire at the correct
    // absolute time. If the store used local wall time labeled as UTC, the
    // expiry would shift by the timezone offset.
    with_tz("Pacific/Auckland", || {
        let store = test_store();
        store
            .blackboard_write("ttl-goal", "value", "syn", 1)
            .expect("write");
        let row = store
            .blackboard_read("ttl-goal")
            .expect("read")
            .expect("row exists");

        let created = row
            .created_at
            .parse::<jiff::Timestamp>()
            .expect("created_at parses");
        let expires = row
            .expires_at
            .expect("expires_at set")
            .parse::<jiff::Timestamp>()
            .expect("expires_at parses");
        assert_eq!(
            expires.duration_since(created),
            jiff::SignedDuration::from_secs(1),
            "TTL must equal exactly 1 second in UTC"
        );

        let now = jiff::Timestamp::now();
        let diff = created.duration_since(now);
        assert!(
            diff <= jiff::SignedDuration::from_secs(5)
                && diff >= jiff::SignedDuration::from_secs(-5),
            "created_at must be near UTC now, not local wall time"
        );
    });
}

#[test]
fn list_sessions_orders_by_updated_at_under_non_utc_timezone() {
    // WHY(#4742): session ordering depends on lexicographic comparison of
    // updated_at strings. The index must remain correct even when the host uses
    // a non-UTC timezone.
    with_tz("Pacific/Auckland", || {
        let store = test_store();
        store
            .create_session("ses-1", "alice", "main", None, None)
            .expect("create first");
        std::thread::sleep(std::time::Duration::from_millis(10));
        store
            .create_session("ses-2", "alice", "secondary", None, None)
            .expect("create second");

        let listed = store.list_sessions(Some("alice")).expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(
            listed[0].id, "ses-2",
            "most recently updated session must be first"
        );
        assert_eq!(listed[1].id, "ses-1");
    });
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
fn finalize_turn_batches_user_assistant_and_usage_with_one_fsync() {
    test_persist_counter::reset();
    let store = test_store();
    let session_id = "ses-finalize-1";
    let usage = UsageRecord {
        session_id: session_id.to_owned(),
        turn_seq: 7,
        input_tokens: 10,
        output_tokens: 20,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        model: Some("test-model".to_owned()),
    };
    let messages = vec![
        FinalizeMessage {
            role: Role::User,
            content: "hello",
            tool_call_id: None,
            tool_name: None,
            token_estimate: 2,
        },
        FinalizeMessage {
            role: Role::Assistant,
            content: "hi there",
            tool_call_id: None,
            tool_name: None,
            token_estimate: 5,
        },
    ];
    let request = FinalizeTurnRequest {
        session_id,
        nous_id: "syn",
        session_key: "main",
        model: Some("test-model"),
        parent_session_id: None,
        messages: &messages,
        usage: &usage,
    };

    let result = store
        .finalize_turn(&request)
        .expect("finalize_turn should succeed");
    assert_eq!(result.messages_persisted, 2);
    assert!(result.usage_recorded);
    assert_eq!(
        test_persist_counter::count(),
        1,
        "finalize_turn must issue exactly one ensure_durable/fsync"
    );

    let history = store.get_history(session_id, None).expect("read history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[0].content, "hello");
    assert_eq!(history[1].role, Role::Assistant);
    assert_eq!(history[1].content, "hi there");

    let usage_rows = store.get_usage_for_session(session_id).expect("read usage");
    assert_eq!(usage_rows.len(), 1);
    assert_eq!(usage_rows[0].turn_seq, 7);
}

#[path = "fjall_store_tests_notes.rs"]
mod notes_and_cleanup;

#[path = "fjall_store_tests_prune.rs"]
mod prune_tests;

// ── Portability raw entry points (issue #4163) ─────────────────────────────

#[cfg(feature = "portability")]
#[path = "fjall_store_tests_portability.rs"]
mod portability;
