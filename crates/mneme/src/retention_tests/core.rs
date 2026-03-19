//! Core retention delete/skip/archive tests.
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use super::*;

#[test]
fn retention_deletes_old_sessions_keeps_recent() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "old-1", "syn", "archived", 100);
    insert_message(&conn, "old-1", 1);
    insert_session(&conn, "recent-1", "syn", "archived", 10);
    insert_message(&conn, "recent-1", 1);

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 1);
    assert_eq!(count_sessions(&conn), 1);
}

#[test]
fn retention_skips_active_sessions() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "active-old", "syn", "active", 200);
    insert_session(&conn, "archived-old", "syn", "archived", 200);

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 1);
    // Active session preserved even though it's old
    assert_eq!(count_sessions(&conn), 1);
}

#[test]
fn archive_produces_valid_json() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let archive_dir = dir.path().join("archive");

    insert_session(&conn, "old-1", "syn", "archived", 100);
    insert_message(&conn, "old-1", 1);
    insert_message(&conn, "old-1", 2);

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: true,
        ..RetentionPolicy::default()
    };

    policy
        .apply(&conn, &archive_dir)
        .expect("retention apply should succeed");

    let archive_path = archive_dir.join("old-1.json");
    assert!(archive_path.exists(), "archive file should exist");

    let contents = std::fs::read_to_string(&archive_path).expect("archive file should be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&contents).expect("archive file should contain valid JSON");
    assert_eq!(parsed["session"]["id"], "old-1");
    assert_eq!(
        parsed["messages"]
            .as_array()
            .expect("messages field should be an array")
            .len(),
        2
    );
}

#[test]
fn orphan_messages_cleaned_up() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "ses-1", "syn", "active", 0);
    insert_message(&conn, "ses-1", 1);

    // Insert orphan message with old timestamp (session deleted after message insert)
    conn.execute_batch("PRAGMA foreign_keys = OFF")
        .expect("disabling foreign keys should succeed");
    let old_ts = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(60 * 24))
        .expect("test timestamp subtraction should not overflow");
    let ts_str = old_ts.strftime("%Y-%m-%dT%H:%M:%S.000Z").to_string();
    conn.execute(
        "INSERT INTO messages (session_id, seq, role, content, created_at) VALUES ('gone', 1, 'user', 'orphan', ?1)",
        [&ts_str],
    ).expect("inserting orphan message should succeed");
    conn.execute_batch("PRAGMA foreign_keys = ON")
        .expect("re-enabling foreign keys should succeed");

    let policy = RetentionPolicy {
        orphan_message_max_age_days: 30,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.messages_deleted, 1);
    // Non-orphan message still exists
    assert_eq!(count_messages(&conn), 1);
}

#[test]
fn max_sessions_per_nous_limit_works() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    for i in 0..5 {
        insert_session(
            &conn,
            &format!("ses-{i}"),
            "syn",
            "archived",
            i64::from(5 - i), // ses-0 is oldest
        );
    }

    let policy = RetentionPolicy {
        max_sessions_per_nous: 2,
        archive_before_delete: false,
        // Set high age so age-based retention doesn't fire
        session_max_age_days: 365,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 3);
    assert_eq!(count_sessions(&conn), 2);
}

#[test]
fn default_policy_retains_everything() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "ses-1", "syn", "archived", 30);
    insert_session(&conn, "ses-2", "syn", "distilled", 60);
    insert_session(&conn, "ses-3", "syn", "active", 10);

    let policy = RetentionPolicy::default();
    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");

    // Default is 90 days, all sessions are < 90 days old
    assert_eq!(result.sessions_deleted, 0);
    assert_eq!(count_sessions(&conn), 3);
}

#[test]
fn retention_archives_before_delete() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let archive_dir = dir.path().join("archive");

    insert_session(&conn, "expired-1", "alice", "archived", 100);
    insert_message(&conn, "expired-1", 1);

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: true,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, &archive_dir)
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 1);

    let archive_path = archive_dir.join("expired-1.json");
    assert!(archive_path.exists(), "archive file must be created");

    let contents = std::fs::read_to_string(&archive_path).expect("archive file should be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&contents).expect("archive file should contain valid JSON");
    assert_eq!(parsed["session"]["id"], "expired-1");
    assert_eq!(
        parsed["messages"]
            .as_array()
            .expect("messages field should be an array")
            .len(),
        1
    );
    assert!(parsed["archived_at"].is_string());

    assert_eq!(count_sessions(&conn), 0, "session deleted after archive");
}

#[test]
fn retention_policy_respects_age() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "young", "bob", "archived", 30);
    insert_session(&conn, "old", "bob", "archived", 100);

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 1);

    let remaining: String = conn
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .expect("querying remaining session should succeed");
    assert_eq!(remaining, "young", "30-day session survives 90-day policy");
}

#[test]
fn retention_idempotent() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "old-1", "alice", "archived", 100);
    insert_session(&conn, "old-2", "alice", "archived", 120);
    insert_session(&conn, "recent-1", "alice", "archived", 10);

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let first = policy
        .apply(&conn, dir.path())
        .expect("first retention apply should succeed");
    assert_eq!(first.sessions_deleted, 2);
    assert_eq!(count_sessions(&conn), 1);

    let second = policy
        .apply(&conn, dir.path())
        .expect("second retention apply should succeed");
    assert_eq!(second.sessions_deleted, 0, "second pass deletes nothing");
    assert_eq!(count_sessions(&conn), 1);
}

#[test]
fn retention_concurrent_access() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    for i in 0..10 {
        insert_session(
            &conn,
            &format!("ses-{i}"),
            "charlie",
            "archived",
            100 + i64::from(i),
        );
    }

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let r1 = policy
        .apply(&conn, dir.path())
        .expect("first retention apply should succeed");

    // Second run on same conn after first completed
    let r2 = policy
        .apply(&conn, dir.path())
        .expect("second retention apply should succeed");

    let total_deleted = r1.sessions_deleted + r2.sessions_deleted;
    assert_eq!(total_deleted, 10, "all expired sessions removed");
    assert_eq!(count_sessions(&conn), 0);
}
