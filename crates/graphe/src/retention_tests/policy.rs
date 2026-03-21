//! Retention policy apply tests.
use super::*;

#[test]
fn retention_empty_store() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    assert_eq!(count_sessions(&conn), 0);

    let policy = RetentionPolicy::default();
    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");

    assert_eq!(result.sessions_deleted, 0);
    assert_eq!(result.messages_deleted, 0);
}

#[test]
fn retention_preserves_active_facts() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    for i in 0..7 {
        insert_session(
            &conn,
            &format!("active-{i}"),
            "alice",
            "active",
            100 + i64::from(i),
        );
    }
    for i in 0..3 {
        insert_session(
            &conn,
            &format!("expired-{i}"),
            "alice",
            "archived",
            100 + i64::from(i),
        );
    }

    let policy = RetentionPolicy {
        session_max_age_days: 90,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 3, "only archived sessions deleted");
    assert_eq!(count_sessions(&conn), 7, "active sessions untouched");
}

#[test]
fn apply_empty_policy_is_noop() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "s1", "alice", "active", 10);
    insert_session(&conn, "s2", "alice", "archived", 30);
    insert_session(&conn, "s3", "bob", "active", 50);
    insert_message(&conn, "s1", 1);
    insert_message(&conn, "s2", 1);

    let policy = RetentionPolicy::default();
    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");

    assert_eq!(
        result.sessions_deleted, 0,
        "default policy keeps everything under 90 days"
    );
    assert_eq!(result.messages_deleted, 0);
    assert_eq!(count_sessions(&conn), 3);
    assert_eq!(count_messages(&conn), 2);
}

#[test]
fn apply_preserves_recent_sessions() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "recent-a", "alice", "archived", 5);
    insert_session(&conn, "recent-b", "alice", "archived", 15);
    insert_session(&conn, "recent-c", "bob", "archived", 25);

    let policy = RetentionPolicy {
        session_max_age_days: 30,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 0, "all sessions are recent enough");
    assert_eq!(count_sessions(&conn), 3);
}

#[test]
fn apply_removes_old_sessions() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "keep", "alice", "archived", 10);
    insert_session(&conn, "remove-1", "alice", "archived", 60);
    insert_session(&conn, "remove-2", "bob", "archived", 80);
    insert_message(&conn, "remove-1", 1);
    insert_message(&conn, "remove-2", 1);

    let policy = RetentionPolicy {
        session_max_age_days: 30,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 2);
    assert_eq!(count_sessions(&conn), 1);

    let remaining: String = conn
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .expect("querying remaining session should succeed");
    assert_eq!(remaining, "keep");
}

#[test]
fn apply_twice_is_idempotent() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    for i in 0..8 {
        let age = 10 + i64::from(i) * 15;
        insert_session(&conn, &format!("idem-{i}"), "alice", "archived", age);
        insert_message(&conn, &format!("idem-{i}"), 1);
    }

    let policy = RetentionPolicy {
        session_max_age_days: 60,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let first = policy
        .apply(&conn, dir.path())
        .expect("first retention apply should succeed");
    let count_after_first = count_sessions(&conn);
    let msg_after_first = count_messages(&conn);

    let second = policy
        .apply(&conn, dir.path())
        .expect("second retention apply should succeed");
    let count_after_second = count_sessions(&conn);
    let msg_after_second = count_messages(&conn);

    assert_eq!(
        count_after_first, count_after_second,
        "applying same policy twice yields same session count"
    );
    assert_eq!(
        msg_after_first, msg_after_second,
        "applying same policy twice yields same message count"
    );
    assert_eq!(second.sessions_deleted, 0, "second pass should be a no-op");
    assert!(
        first.sessions_deleted > 0,
        "first pass should delete something"
    );
}

#[test]
fn apply_skips_active_sessions() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "active-ancient", "alice", "active", 500);
    insert_session(&conn, "active-old", "bob", "active", 200);
    insert_session(&conn, "archived-old", "alice", "archived", 200);

    let policy = RetentionPolicy {
        session_max_age_days: 1,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 1, "only archived session removed");
    assert_eq!(count_sessions(&conn), 2, "both active sessions survive");

    let ids: Vec<String> = conn
        .prepare("SELECT id FROM sessions ORDER BY id")
        .expect("preparing session id query should succeed")
        .query_map([], |row| row.get(0))
        .expect("querying session ids should succeed")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("collecting session ids should succeed");
    assert!(ids.contains(&"active-ancient".to_owned()));
    assert!(ids.contains(&"active-old".to_owned()));
}

#[test]
fn policy_max_sessions_respected() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    for i in 0..10 {
        insert_session(
            &conn,
            &format!("max-{i}"),
            "alice",
            "archived",
            i64::from(i),
        );
    }

    let policy = RetentionPolicy {
        max_sessions_per_nous: 3,
        session_max_age_days: 365,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(result.sessions_deleted, 7);
    assert_eq!(count_sessions(&conn), 3, "only 3 most recent kept");
}

#[test]
fn retention_with_zero_max_age() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    insert_session(&conn, "closed-1", "alice", "archived", 1);
    insert_session(&conn, "closed-2", "alice", "distilled", 2);
    insert_session(&conn, "active-1", "alice", "active", 1);

    let policy = RetentionPolicy {
        session_max_age_days: 0,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert_eq!(
        result.sessions_deleted, 2,
        "max_age=0 should delete all non-active sessions"
    );
    assert_eq!(count_sessions(&conn), 1, "active session survives");

    let remaining: String = conn
        .query_row("SELECT id FROM sessions", [], |row| row.get(0))
        .expect("querying remaining session should succeed");
    assert_eq!(remaining, "active-1");
}

#[test]
fn retention_respects_keep_minimum() {
    let conn = test_conn();
    let dir = tempfile::tempdir().expect("temp dir should be created");

    for i in 0..6 {
        insert_session(
            &conn,
            &format!("keep-{i}"),
            "bob",
            "archived",
            i64::from(i) + 1,
        );
    }

    let policy = RetentionPolicy {
        session_max_age_days: 0,
        max_sessions_per_nous: 3,
        archive_before_delete: false,
        ..RetentionPolicy::default()
    };

    let result = policy
        .apply(&conn, dir.path())
        .expect("retention apply should succeed");
    assert!(
        result.sessions_deleted >= 3,
        "at least the excess sessions should be deleted"
    );
    let remaining = count_sessions(&conn);
    assert!(
        remaining <= 3,
        "per-nous limit of 3 should be respected, got {remaining}"
    );
}
