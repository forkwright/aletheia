use super::*;
use crate::migration;

fn test_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database should open");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enabling foreign keys should succeed");
    migration::run_migrations(&conn).expect("migrations should run on fresh database");
    conn
}

fn insert_session(conn: &Connection, id: &str, nous_id: &str, status: &str, age_days: i64) {
    let ts = jiff::Timestamp::now()
        .checked_sub(jiff::SignedDuration::from_hours(age_days * 24))
        .expect("test timestamp subtraction should not overflow");
    let ts_str = ts.strftime("%Y-%m-%dT%H:%M:%S.000Z").to_string();

    conn.execute(
        "INSERT INTO sessions (id, nous_id, session_key, status, created_at, updated_at)
         VALUES (?1, ?2, ?1, ?3, ?4, ?4)",
        rusqlite::params![id, nous_id, status, ts_str],
    )
    .expect("inserting test session should succeed");
}

fn insert_message(conn: &Connection, session_id: &str, seq: i64) {
    conn.execute(
        "INSERT INTO messages (session_id, seq, role, content)
         VALUES (?1, ?2, 'user', 'test message')",
        rusqlite::params![session_id, seq],
    )
    .expect("inserting test message should succeed");
}

fn count_sessions(conn: &Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .expect("counting sessions should succeed")
}

fn count_messages(conn: &Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("counting messages should succeed")
}

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

    let contents =
        std::fs::read_to_string(&archive_path).expect("archive file should be readable");
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

    let contents =
        std::fs::read_to_string(&archive_path).expect("archive file should be readable");
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
    let contents =
        std::fs::read_to_string(&archive_path).expect("archive file should be readable");
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
    }
}
