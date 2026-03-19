//! Archive, boundary, and edge case retention tests.
use super::*;

#[test]
fn policy_preserves_notes_in_archive() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let archive_dir = dir.path().join("archive");

    insert_session(&conn, "noted", "alice", "archived", 100);
    insert_message(&conn, "noted", 1);
    conn.execute(
        "INSERT INTO agent_notes (session_id, nous_id, category, content) VALUES ('noted', 'alice', 'context', 'important context')",
        [],
    )
    .expect("inserting test agent note should succeed");

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: true,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, &archive_dir)
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 1);

    let archive_path = archive_dir.join("noted.json");
    assert!(archive_path.exists());
    let contents = std::fs::read_to_string(&archive_path).expect("archive file should be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&contents).expect("archive file should contain valid JSON");
    assert_eq!(
        parsed["notes"]
            .as_array()
            .expect("notes field should be an array")
            .len(),
        1
    );
    assert_eq!(parsed["notes"][0]["content"], "important context");
    assert_eq!(parsed["notes"][0]["category"], "context");
}

#[test]
fn default_policy_has_correct_field_values() {
    let policy = RetentionPolicy::default();
    assert_eq!(
        policy.session_max_age_days, 90,
        "default session max age is 90 days"
    );
    assert_eq!(
        policy.orphan_message_max_age_days, 30,
        "default orphan message max age is 30 days"
    );
    assert_eq!(
        policy.max_sessions_per_nous, 0,
        "default max sessions per nous is 0 (unlimited)"
    );
    assert!(
        policy.archive_before_delete,
        "default archive_before_delete is true"
    );
}

#[test]
fn custom_policy_overrides_all_defaults() {
    let policy = RetentionPolicy {
        session_max_age_days: 14,
        orphan_message_max_age_days: 7,
        max_sessions_per_nous: 10,
        archive_before_delete: false,
    };
    assert_eq!(policy.session_max_age_days, 14);
    assert_eq!(policy.orphan_message_max_age_days, 7);
    assert_eq!(policy.max_sessions_per_nous, 10);
    assert!(!policy.archive_before_delete);
}

#[test]
fn retention_boundary_age_keeps_within_threshold() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    // Policy: 30-day max age.
    // 29-day session is within threshold: must be kept.
    // 31-day session is past threshold: must be deleted.
    insert_session(&conn, "boundary-young", "alice", "archived", 29);
    insert_session(&conn, "boundary-old", "alice", "archived", 31);

    let policy = RetentionPolicy {
        session_max_age_days: 30,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(
        result.sessions_deleted, 1,
        "only the 31-day session should be deleted"
    );

    let remaining: String = conn
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .expect("querying remaining session should succeed");
    assert_eq!(
        remaining, "boundary-young",
        "29-day session survives a 30-day policy"
    );
}

#[test]
fn archive_preserves_message_seq_order() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let archive_dir = dir.path().join("ordered-archive");

    insert_session(&conn, "ordered-ses", "alice", "archived", 100);
    // Insert messages out of seq order to verify the archive sorts them.
    insert_message(&conn, "ordered-ses", 3);
    insert_message(&conn, "ordered-ses", 1);
    insert_message(&conn, "ordered-ses", 2);

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: true,
        ..RetentionPolicy::default()
    };

    policy
        .apply(&conn, &archive_dir)
        .expect("retention apply should succeed");

    let archive_path = archive_dir.join("ordered-ses.json");
    let contents = std::fs::read_to_string(&archive_path).expect("archive file should be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&contents).expect("archive should be valid JSON");

    let messages = parsed["messages"]
        .as_array()
        .expect("messages should be an array");
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[0]["seq"], 1i64,
        "first message in archive has seq=1"
    );
    assert_eq!(
        messages[1]["seq"], 2i64,
        "second message in archive has seq=2"
    );
    assert_eq!(
        messages[2]["seq"], 3i64,
        "third message in archive has seq=3"
    );
}

#[test]
fn archive_handles_session_with_no_messages() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let archive_dir = dir.path().join("empty-msg-archive");

    insert_session(&conn, "no-messages", "alice", "archived", 100);

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: true,
        ..RetentionPolicy::default()
    };

    policy
        .apply(&conn, &archive_dir)
        .expect("retention apply should succeed for session with no messages");

    let archive_path = archive_dir.join("no-messages.json");
    assert!(
        archive_path.exists(),
        "archive file should exist even when session has no messages"
    );

    let contents = std::fs::read_to_string(&archive_path).expect("archive file should be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&contents).expect("archive should be valid JSON");

    assert_eq!(parsed["session"]["id"], "no-messages");
    assert_eq!(
        parsed["messages"]
            .as_array()
            .expect("messages should be an array")
            .len(),
        0,
        "messages array is empty when session has no messages"
    );
}

#[test]
fn archive_handles_large_message_count() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let archive_dir = dir.path().join("large-msg-archive");

    insert_session(&conn, "large-ses", "alice", "archived", 100);
    for i in 1i64..=105 {
        insert_message(&conn, "large-ses", i);
    }

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: true,
        ..RetentionPolicy::default()
    };

    policy
        .apply(&conn, &archive_dir)
        .expect("retention apply should succeed for session with 105 messages");

    let archive_path = archive_dir.join("large-ses.json");
    let contents = std::fs::read_to_string(&archive_path).expect("archive file should be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&contents).expect("archive should be valid JSON");

    assert_eq!(
        parsed["messages"]
            .as_array()
            .expect("messages should be an array")
            .len(),
        105,
        "all 105 messages are preserved in the archive"
    );
}

#[test]
fn recent_orphan_messages_not_deleted() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    // Insert an orphan with the current timestamp (SQLite DEFAULT = now).
    // With a 30-day threshold, this message must not be cleaned up.
    conn.execute_batch("PRAGMA foreign_keys = OFF")
        .expect("disabling foreign keys should succeed");
    conn.execute(
        "INSERT INTO messages (session_id, seq, role, content) VALUES ('no-such-session', 1, 'user', 'recent orphan')",
        [],
    )
    .expect("inserting recent orphan should succeed");
    conn.execute_batch("PRAGMA foreign_keys = ON")
        .expect("re-enabling foreign keys should succeed");

    let policy = RetentionPolicy {
        orphan_message_max_age_days: 30,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(
        result.messages_deleted, 0,
        "recent orphan should not be deleted"
    );
    assert_eq!(count_messages(&conn), 1, "recent orphan is retained");
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn retention_idempotency(
            fact_count in 1_usize..20,
            policy_days in 1_u32..365,
        ) {
            let conn = test_conn();
            let dir = tempfile::tempdir().expect("temp dir should be created");

            for i in 0..fact_count {
                let age = i64::try_from(i).expect("test index fits i64") * 10 + 5;
                insert_session(
                    &conn,
                    &format!("prop-ses-{i}"),
                    "alice",
                    "archived",
                    age,
                );
            }

            let policy = RetentionPolicy {
                session_max_age_days: policy_days,
                archive_before_delete: false,
                ..RetentionPolicy::default()
            };

            policy
                .apply(&conn, dir.path())
                .expect("first retention apply should succeed");
            let after_first = count_sessions(&conn);

            policy
                .apply(&conn, dir.path())
                .expect("second retention apply should succeed");
            let after_second = count_sessions(&conn);

            prop_assert_eq!(
                after_first, after_second,
                "second retention pass must not change session count"
            );
        }

        #[test]
        fn deletion_count_never_exceeds_total_session_count(
            session_count in 1_usize..=15,
            policy_days in 0_u32..=180,
        ) {
            let conn = test_conn();
            let dir = tempfile::tempdir().expect("temp dir should be created");

            for i in 0..session_count {
                let age = i64::try_from(i).expect("test index fits i64") * 10 + 5;
                insert_session(
                    &conn,
                    &format!("bound-{i}"),
                    "alice",
                    "archived",
                    age,
                );
            }

            let initial_count = count_sessions(&conn);
            let policy = RetentionPolicy {
                session_max_age_days: policy_days,
                archive_before_delete: false,
                ..RetentionPolicy::default()
            };

            let result = policy
                .apply(&conn, dir.path())
                .expect("retention apply should succeed");

            prop_assert!(
                result.sessions_deleted <= initial_count,
                "sessions_deleted ({}) exceeded initial count ({})",
                result.sessions_deleted,
                initial_count,
            );
        }
    }
}
