use mneme::types::{SessionMetrics, SessionOrigin, SessionType};
use oikonomos::maintenance::RetentionExecutor as _;

use super::*;

fn retention_error(reason: impl Into<String>) -> oikonomos::error::Error {
    oikonomos::error::TaskFailedSnafu {
        task_id: "retention-execution",
        reason: reason.into(),
    }
    .build()
}

fn session_fixture(
    id: &str,
    nous_id: &str,
    session_key: &str,
    status: SessionStatus,
    updated_at: &str,
) -> Session {
    Session {
        id: id.to_owned(),
        nous_id: nous_id.to_owned(),
        session_key: session_key.to_owned(),
        status,
        model: None,
        session_type: SessionType::Primary,
        created_at: updated_at.to_owned(),
        updated_at: updated_at.to_owned(),
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
            owner: None,
            task_id: None,
            client_turn_id: None,
        },
        artefact_meta: None,
    }
}

fn import_fixture(
    store: &SessionStore,
    id: &str,
    nous_id: &str,
    status: SessionStatus,
    updated_at: &str,
) -> oikonomos::error::Result<()> {
    store
        .import_session(
            &session_fixture(id, nous_id, &format!("key-{id}"), status, updated_at),
            false,
        )
        .map_err(|e| retention_error(format!("import {id}: {e}")))?;
    Ok(())
}

fn session_exists(store: &SessionStore, id: &str) -> oikonomos::error::Result<bool> {
    store
        .find_session_by_id(id)
        .map(|session| session.is_some())
        .map_err(|e| retention_error(format!("find {id}: {e}")))
}

#[tokio::test]
async fn retention_adapter_executes_blackboard_cleanup() -> oikonomos::error::Result<()> {
    let store =
        Arc::new(Mutex::new(SessionStore::open_in_memory().map_err(|e| {
            retention_error(format!("session store open failed: {e}"))
        })?));

    let adapter = SessionRetentionAdapter::new_with_settings(
        Arc::clone(&store),
        RetentionSettings::default(),
    );
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("retention task join failed: {e}")))??;

    assert_eq!(summary.blackboard_entries_cleaned, 0);
    assert_eq!(summary.sessions_cleaned, 0);
    let entries = store
        .lock()
        .await
        .blackboard_list()
        .map_err(|e| retention_error(format!("blackboard list failed: {e}")))?;
    assert!(entries.is_empty());
    Ok(())
}

#[tokio::test]
async fn retention_disabled_skips_session_cleanup() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        locked
            .create_session("ses-old", "syn", "main", None, None)
            .map_err(|e| retention_error(format!("create: {e}")))?;
    }

    let settings = RetentionSettings {
        enabled: false,
        closed_session_ttl_days: Some(0),
        archive_before_delete: true,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(
        summary.sessions_cleaned, 0,
        "disabled retention must not clean sessions"
    );
    Ok(())
}

#[tokio::test]
async fn retention_no_ttl_skips_session_cleanup() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        locked
            .create_session("ses-old", "syn", "main", None, None)
            .map_err(|e| retention_error(format!("create: {e}")))?;
    }

    let settings = RetentionSettings {
        enabled: true,
        closed_session_ttl_days: None,
        archive_before_delete: true,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(
        summary.sessions_cleaned, 0,
        "no ttl means no session cleanup"
    );
    Ok(())
}

#[tokio::test]
async fn retention_skips_active_sessions() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        locked
            .create_session("ses-active", "syn", "key-a", None, None)
            .map_err(|e| retention_error(format!("create: {e}")))?;
    }

    let settings = RetentionSettings {
        enabled: true,
        closed_session_ttl_days: Some(0),
        archive_before_delete: true,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(
        summary.sessions_cleaned, 0,
        "active session must not be deleted by closed-session retention"
    );
    let session = store
        .lock()
        .await
        .find_session_by_id("ses-active")
        .map_err(|e| retention_error(format!("find: {e}")))?;
    assert_eq!(
        session.map(|s| s.status),
        Some(SessionStatus::Active),
        "active session must remain active after retention"
    );
    Ok(())
}

#[tokio::test]
async fn retention_exports_archived_session_before_delete() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        locked
            .create_session("ses-arc", "syn", "key-b", None, None)
            .map_err(|e| retention_error(format!("create: {e}")))?;
        locked
            .append_message(
                "ses-arc",
                mneme::types::Role::User,
                "archive me",
                None,
                None,
                2,
            )
            .map_err(|e| retention_error(format!("append: {e}")))?;
        locked
            .update_session_status("ses-arc", SessionStatus::Archived)
            .map_err(|e| retention_error(format!("archive: {e}")))?;
    }

    let settings = RetentionSettings {
        enabled: true,
        closed_session_ttl_days: Some(0),
        archive_before_delete: true,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(summary.sessions_cleaned, 1);
    assert_eq!(summary.messages_cleaned, 1);
    assert!(
        summary.bytes_freed > 0,
        "archive byte count should be reported"
    );
    let locked = store.lock().await;
    let archive_path = archive_dir_for_store(&locked)?.join("ses-arc.json");
    let archive = std::fs::read_to_string(&archive_path)
        .map_err(|e| retention_error(format!("read archive: {e}")))?;
    let archive_json: serde_json::Value = serde_json::from_str(&archive)
        .map_err(|e| retention_error(format!("parse archive: {e}")))?;
    assert_eq!(archive_json["session"]["id"], "ses-arc");
    assert_eq!(archive_json["messages"][0]["content"], "archive me");
    let session = locked
        .find_session_by_id("ses-arc")
        .map_err(|e| retention_error(format!("find: {e}")))?;
    assert!(
        session.is_none(),
        "archived session must be deleted after archive write"
    );
    Ok(())
}

#[tokio::test]
async fn retention_cap_zero_is_unlimited() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        import_fixture(
            &locked,
            "ses-a",
            "syn",
            SessionStatus::Archived,
            "2024-01-01T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "ses-b",
            "syn",
            SessionStatus::Distilled,
            "2024-01-02T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "ses-c",
            "syn",
            SessionStatus::Archived,
            "2024-01-03T00:00:00Z",
        )?;
    }

    let settings = RetentionSettings {
        enabled: true,
        closed_session_ttl_days: None,
        max_sessions_per_nous: 0,
        archive_before_delete: true,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(summary.sessions_cleaned, 0);
    assert_eq!(summary.cap_sessions_cleaned, 0);
    let locked = store.lock().await;
    for id in ["ses-a", "ses-b", "ses-c"] {
        assert!(session_exists(&locked, id)?);
    }
    Ok(())
}

#[tokio::test]
async fn retention_cap_enforces_per_nous_over_closed_sessions_and_preserves_active_sessions()
-> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        import_fixture(
            &locked,
            "syn-active-new",
            "syn",
            SessionStatus::Active,
            "2024-01-05T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "syn-arch-keep",
            "syn",
            SessionStatus::Archived,
            "2024-01-04T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "syn-dist-drop",
            "syn",
            SessionStatus::Distilled,
            "2024-01-03T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "syn-arch-drop",
            "syn",
            SessionStatus::Archived,
            "2024-01-02T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "syn-active-old",
            "syn",
            SessionStatus::Active,
            "2024-01-01T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "bob-arch-keep",
            "bob",
            SessionStatus::Archived,
            "2024-01-03T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "bob-dist-keep",
            "bob",
            SessionStatus::Distilled,
            "2024-01-02T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "bob-arch-drop",
            "bob",
            SessionStatus::Archived,
            "2024-01-01T00:00:00Z",
        )?;
    }

    let settings = RetentionSettings {
        enabled: true,
        closed_session_ttl_days: None,
        max_sessions_per_nous: 2,
        archive_before_delete: false,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(summary.sessions_cleaned, 2);
    assert_eq!(summary.cap_sessions_cleaned, 2);
    let locked = store.lock().await;
    for id in [
        "syn-active-new",
        "syn-arch-keep",
        "syn-dist-drop",
        "syn-active-old",
        "bob-arch-keep",
        "bob-dist-keep",
    ] {
        assert!(session_exists(&locked, id)?, "{id} should remain");
    }
    for id in ["syn-arch-drop", "bob-arch-drop"] {
        assert!(!session_exists(&locked, id)?, "{id} should be deleted");
    }
    Ok(())
}

#[tokio::test]
async fn retention_cap_uses_session_id_for_stable_ties() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        import_fixture(
            &locked,
            "b",
            "syn",
            SessionStatus::Archived,
            "2024-01-01T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "a",
            "syn",
            SessionStatus::Archived,
            "2024-01-01T00:00:00Z",
        )?;
    }

    let settings = RetentionSettings {
        enabled: true,
        closed_session_ttl_days: None,
        max_sessions_per_nous: 1,
        archive_before_delete: false,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(summary.sessions_cleaned, 1);
    assert_eq!(summary.cap_sessions_cleaned, 1);
    let locked = store.lock().await;
    assert!(session_exists(&locked, "a")?);
    assert!(!session_exists(&locked, "b")?);
    Ok(())
}

#[tokio::test]
async fn retention_cap_archives_before_delete() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        locked
            .create_session("cap-del", "syn", "key-cap-del", None, None)
            .map_err(|e| retention_error(format!("create: {e}")))?;
        locked
            .append_message(
                "cap-del",
                mneme::types::Role::User,
                "cap archive me",
                None,
                None,
                2,
            )
            .map_err(|e| retention_error(format!("append: {e}")))?;
        locked
            .import_session(
                &session_fixture(
                    "cap-del",
                    "syn",
                    "key-cap-del",
                    SessionStatus::Archived,
                    "2024-01-01T00:00:00Z",
                ),
                true,
            )
            .map_err(|e| retention_error(format!("import cap-del: {e}")))?;
        import_fixture(
            &locked,
            "keep",
            "syn",
            SessionStatus::Archived,
            "2024-01-02T00:00:00Z",
        )?;
    }

    let settings = RetentionSettings {
        enabled: true,
        closed_session_ttl_days: None,
        max_sessions_per_nous: 1,
        archive_before_delete: true,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(summary.sessions_cleaned, 1);
    assert_eq!(summary.cap_sessions_cleaned, 1);
    assert_eq!(summary.messages_cleaned, 1);
    assert!(summary.bytes_freed > 0);
    let locked = store.lock().await;
    let archive_path = archive_dir_for_store(&locked)?.join("cap-del.json");
    let archive = std::fs::read_to_string(&archive_path)
        .map_err(|e| retention_error(format!("read archive: {e}")))?;
    let archive_json: serde_json::Value = serde_json::from_str(&archive)
        .map_err(|e| retention_error(format!("parse archive: {e}")))?;
    assert_eq!(archive_json["session"]["id"], "cap-del");
    assert_eq!(archive_json["messages"][0]["content"], "cap archive me");
    assert!(!session_exists(&locked, "cap-del")?);
    assert!(session_exists(&locked, "keep")?);
    Ok(())
}

#[tokio::test]
async fn retention_applies_age_policy_before_closed_session_cap() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));
    {
        let locked = store.lock().await;
        import_fixture(
            &locked,
            "ttl-drop",
            "syn",
            SessionStatus::Archived,
            "2024-01-01T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "cap-drop",
            "syn",
            SessionStatus::Archived,
            "2099-01-01T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "keep-distilled",
            "syn",
            SessionStatus::Distilled,
            "2099-01-03T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "keep-archived",
            "syn",
            SessionStatus::Archived,
            "2099-01-02T00:00:00Z",
        )?;
        import_fixture(
            &locked,
            "active-old",
            "syn",
            SessionStatus::Active,
            "2024-01-02T00:00:00Z",
        )?;
    }

    let settings = RetentionSettings {
        enabled: true,
        closed_session_ttl_days: Some(30),
        max_sessions_per_nous: 2,
        archive_before_delete: false,
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert_eq!(
        summary.sessions_cleaned, 2,
        "one closed session should be removed by age and one by cap"
    );
    assert_eq!(
        summary.cap_sessions_cleaned, 1,
        "age deletion must not also be reported as a cap deletion"
    );
    let locked = store.lock().await;
    for id in ["keep-distilled", "keep-archived", "active-old"] {
        assert!(session_exists(&locked, id)?, "{id} should remain");
    }
    for id in ["ttl-drop", "cap-drop"] {
        assert!(!session_exists(&locked, id)?, "{id} should be deleted");
    }
    Ok(())
}

#[tokio::test]
async fn retention_prunes_session_archives_older_than_ttl() -> oikonomos::error::Result<()> {
    let store = Arc::new(Mutex::new(
        SessionStore::open_in_memory().map_err(|e| retention_error(format!("store open: {e}")))?,
    ));

    let archive_dir = {
        let locked = store.lock().await;
        let dir = archive_dir_for_store(&locked)?;
        fs::create_dir_all(&dir)
            .map_err(|e| retention_error(format!("create archive dir: {e}")))?;
        dir
    };

    let stale_path = archive_dir.join("stale.json");
    let recent_path = archive_dir.join("recent.json");
    write_archive_file(&stale_path, b"old")
        .map_err(|e| retention_error(format!("write stale archive: {e}")))?;
    write_archive_file(&recent_path, b"recent")
        .map_err(|e| retention_error(format!("write recent archive: {e}")))?;

    let stale_time = SystemTime::now()
        .checked_sub(Duration::from_hours(120))
        .ok_or_else(|| retention_error("stale archive time overflow"))?;
    let file = OpenOptions::new()
        .write(true)
        .open(&stale_path)
        .map_err(|e| retention_error(format!("open stale archive: {e}")))?;
    file.set_times(std::fs::FileTimes::new().set_modified(stale_time))
        .map_err(|e| retention_error(format!("set stale archive mtime: {e}")))?;

    let settings = RetentionSettings {
        enabled: false,
        archive_ttl_days: Some(2),
        ..RetentionSettings::default()
    };
    let adapter = SessionRetentionAdapter::new_with_settings(Arc::clone(&store), settings);
    let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
        .await
        .map_err(|e| retention_error(format!("join: {e}")))??;

    assert!(
        !stale_path.exists(),
        "archive file older than TTL should be pruned"
    );
    assert!(
        recent_path.exists(),
        "archive file newer than TTL should remain"
    );
    assert!(
        summary.bytes_freed >= 3,
        "pruned archive bytes should be counted in summary"
    );
    Ok(())
}
