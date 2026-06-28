#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

// ── background.rs: reap_background_tasks ─────────────────────────────────────

#[tokio::test]
async fn reap_background_tasks_joins_completed_tasks() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor.runtime.background_tasks.spawn(async { /* no-op */ });

    tokio::task::yield_now().await;

    actor.reap_background_tasks();

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "completed tasks should be reaped"
    );
}

#[tokio::test]
async fn reap_background_tasks_records_background_panic() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor
        .runtime
        .background_tasks
        .spawn(async { panic!("background test panic") });

    // WHY: multiple yields let the spawned task panic and be collected.
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }

    actor.reap_background_tasks();

    assert_eq!(
        actor.runtime.background_panic_count, 1,
        "panic in background task should increment background_panic_count"
    );
    assert_eq!(
        actor.runtime.background_failure.total_count, 1,
        "panic in background task should increment background_failure_total_count"
    );
    assert_eq!(
        actor.runtime.background_failure.timestamps.len(),
        1,
        "panic in background task should record a failure timestamp"
    );
    assert_eq!(
        actor.runtime.background_failure.latest_kind.as_deref(),
        Some("panic"),
        "latest background failure kind should be panic"
    );
    assert!(
        actor
            .runtime
            .background_failure
            .latest_message
            .as_deref()
            .is_some_and(|m: &str| m.contains("panicked")),
        "panics captured by JoinSet should carry a panic message"
    );
    assert_eq!(
        actor.channel.status,
        NousLifecycle::Idle,
        "background panic must not enter pipeline degraded mode"
    );

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    actor.handle_status(tx);
    let status = rx.try_recv().expect("status should be ready");
    assert_eq!(status.background_failure_total_count, 1);
    assert_eq!(status.background_failure_recent_count, 1);
    assert!(!status.background_health_degraded);
}

#[tokio::test]
async fn reap_background_tasks_records_cancelled_task_as_error() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let abort_handle = actor
        .runtime
        .background_tasks
        .spawn(std::future::pending::<()>());
    abort_handle.abort();

    tokio::task::yield_now().await;

    actor.reap_background_tasks();

    assert_eq!(
        actor.runtime.background_panic_count, 0,
        "cancelled task should not increment background_panic_count"
    );
    assert_eq!(
        actor.runtime.background_failure.total_count, 1,
        "cancelled task should increment background_failure_total_count"
    );
    assert_eq!(
        actor.runtime.background_failure.latest_kind.as_deref(),
        Some("error"),
        "latest background failure kind should be error for cancelled task"
    );
    assert!(
        actor
            .runtime
            .background_failure
            .latest_message
            .as_deref()
            .is_some_and(|m: &str| m.contains("cancelled")),
        "cancelled task should carry a cancellation message"
    );

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    actor.handle_status(tx);
    let status = rx.try_recv().expect("status should be ready");
    assert_eq!(status.background_failure_total_count, 1);
    assert_eq!(status.background_failure_recent_count, 1);
    assert!(!status.background_health_degraded);
}

#[tokio::test]
async fn reap_background_tasks_noop_when_empty() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.runtime.background_tasks.len(), 0);

    actor.reap_background_tasks(); // must not panic

    assert_eq!(actor.runtime.background_panic_count, 0);
}

// ── background.rs: maybe_spawn_extraction ────────────────────────────────────

#[test]
fn maybe_spawn_extraction_skips_when_no_config() {
    let config = PipelineConfig {
        extraction: None,
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction("hello world", "response text", &[], "");

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "no task should be spawned when extraction config is absent"
    );
}

#[test]
fn maybe_spawn_extraction_skips_when_disabled() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: false,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction("hello world", "response", &[], "");

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "no task should be spawned when extraction is disabled"
    );
}

#[test]
fn maybe_spawn_extraction_skips_when_content_too_short() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1000, // very high threshold
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction("short", "response", &[], "");

    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "no task should be spawned when content is below min_message_length"
    );
}

#[tokio::test]
async fn maybe_spawn_extraction_skips_when_task_limit_reached() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1, // accept any content
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    for _ in 0..MAX_SPAWNED_TASKS {
        actor
            .runtime
            .background_tasks
            .spawn(std::future::pending::<()>());
    }
    assert_eq!(actor.runtime.background_tasks.len(), MAX_SPAWNED_TASKS);

    actor.maybe_spawn_extraction("long enough content here", "response text here", &[], "");

    assert_eq!(
        actor.runtime.background_tasks.len(),
        MAX_SPAWNED_TASKS,
        "task should not be spawned when limit is reached"
    );
}

#[tokio::test]
async fn maybe_spawn_extraction_spawns_with_tool_calls_and_reasoning() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    let tool_calls = vec![crate::pipeline::ToolCall {
        id: "tc-1".to_owned(),
        name: "read_file".to_owned(),
        input: serde_json::json!({"path": "/tmp/test.txt"}),
        result: Some("file contents".to_owned()),
        is_error: false,
        duration_ms: 42,
        approval: None,
        receipt: None,
    }];

    actor.maybe_spawn_extraction(
        "user message here",
        "assistant response here",
        &tool_calls,
        "I need to check the file first.",
    );

    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "extraction task should be spawned with tool calls and reasoning"
    );
}

#[tokio::test]
async fn maybe_spawn_extraction_spawns_without_tool_calls_or_reasoning() {
    let config = PipelineConfig {
        extraction: Some(mneme::extract::ExtractionConfig {
            enabled: true,
            min_message_length: 1,
            ..mneme::extract::ExtractionConfig::default()
        }),
        ..PipelineConfig::default()
    };
    let (mut actor, _tx, _dir) = make_test_actor(config);

    actor.maybe_spawn_extraction("user message here", "assistant response here", &[], "");

    assert_eq!(
        actor.runtime.background_tasks.len(),
        1,
        "extraction task should be spawned even without tool calls or reasoning"
    );
}

// ── background.rs: maybe_spawn_skill_analysis ────────────────────────────────

#[test]
fn maybe_spawn_skill_analysis_noop_on_empty_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor.maybe_spawn_skill_analysis(&[], "my-session");

    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

#[test]
fn maybe_spawn_skill_analysis_processes_successful_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let calls = vec![make_tool_call("bash", false)];
    actor.maybe_spawn_skill_analysis(&calls, "my-session");
    // WHY: no task is spawned unless the candidate is promoted (first occurrence never promotes).
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

#[test]
fn maybe_spawn_skill_analysis_processes_errored_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let calls = vec![make_tool_call("bash", true)];
    // NOTE: error calls are recorded as ToolCallRecord::errored.
    actor.maybe_spawn_skill_analysis(&calls, "s");
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

// ── background.rs: maybe_spawn_distillation ──────────────────────────────────

#[tokio::test]
async fn maybe_spawn_distillation_skips_when_flag_already_set() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor
        .runtime
        .distillation_in_progress
        .store(true, std::sync::atomic::Ordering::Release);

    actor.maybe_spawn_distillation("s").await;

    assert!(
        actor
            .runtime
            .distillation_in_progress
            .load(std::sync::atomic::Ordering::Acquire),
        "flag should remain set"
    );
    assert_eq!(
        actor.runtime.background_tasks.len(),
        0,
        "no task should be spawned when distillation is already in progress"
    );
}

#[tokio::test]
async fn maybe_spawn_distillation_clears_flag_when_no_session_store() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor.maybe_spawn_distillation("s").await;

    assert!(
        !actor
            .runtime
            .distillation_in_progress
            .load(std::sync::atomic::Ordering::Acquire),
        "flag should be cleared when no task was spawned"
    );
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

#[tokio::test]
async fn maybe_spawn_distillation_clears_flag_when_session_not_found() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    actor.stores.session_store = Some(Arc::new(tokio::sync::Mutex::new(store)));

    actor.maybe_spawn_distillation("nonexistent-session").await;

    assert!(
        !actor
            .runtime
            .distillation_in_progress
            .load(std::sync::atomic::Ordering::Acquire),
        "flag must be cleared when session is not found"
    );
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

/// Regression test for #758/#916/#923: session ID divergence.
///
/// Verifies that when pylon creates a DB session and passes its ID to the
/// actor, the finalize stage persists messages under the SAME session ID
/// (not a newly generated one).
#[tokio::test]
async fn session_id_adoption_prevents_fk_divergence() {
    let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    // WHY: SessionId requires UUID v4 format after security hardening (#1754)
    let db_session_id = "550e8400-e29b-41d4-a716-446655440000";

    // NOTE: Simulate pylon creating the session in the store
    store
        .create_session(
            db_session_id,
            "test-agent",
            "main",
            None,
            Some("test-model"),
        )
        .expect("create session");

    let store = Arc::new(tokio::sync::Mutex::new(store));
    let (handle, join, _dir) = spawn_test_actor_with_store(Arc::clone(&store));

    let result = handle
        .send_turn_with_session_id(
            "main",
            Some(db_session_id.to_owned()),
            "Hello",
            crate::handle::DEFAULT_SEND_TIMEOUT,
        )
        .await
        .expect("turn should succeed");
    assert_eq!(result.content, "Hello from actor!");

    let store_guard = store.lock().await;
    let history = store_guard
        .get_history(db_session_id, None)
        .expect("history");

    assert!(
        history.len() >= 2,
        "expected at least 2 messages under DB session ID, got {}",
        history.len()
    );

    // WHY: Verify no messages exist under a different session ID
    // (if divergence occurred, messages would be under a random ULID)
    let all_sessions = store_guard
        .list_sessions(Some("test-agent"))
        .expect("list sessions");
    assert_eq!(
        all_sessions.len(),
        1,
        "should have exactly 1 session, got {}",
        all_sessions.len()
    );
    assert_eq!(all_sessions[0].id, db_session_id);

    drop(store_guard);
    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

/// Regression test for #3103: prosoche daemon FK constraint failure.
///
/// Simulates the scenario where the daemon's "daemon:prosoche" session already
/// exists in the DB (from a previous cycle), but the actor has no in-memory
/// session for that key (e.g., after restart or LRU eviction). The daemon
/// bridge calls `send_turn` with `session_id: None`, so the actor generates a
/// new ULID — which diverges from the DB's canonical ID.
///
/// Without session-ID adoption, `find_or_create_session` returns the existing
/// DB session silently (ON CONFLICT DO NOTHING) while `finalize` calls
/// `append_message` with the actor's newly generated ID (no DB row) →
/// FOREIGN KEY constraint failure and silent data loss. The actor must adopt
/// the DB session ID returned by `find_or_create_session`.
#[tokio::test]
async fn prosoche_daemon_adopts_existing_db_session_id() {
    let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    // WHY: SessionId requires UUID v4 format
    let existing_db_id = "660e8400-e29b-41d4-a716-446655440001";

    // NOTE: simulate an existing DB session for the "daemon:prosoche" key
    // (as would exist from a previous prosoche cycle).
    store
        .create_session(
            existing_db_id,
            "test-agent",
            "daemon:prosoche",
            None,
            Some("test-model"),
        )
        .expect("create pre-existing prosoche session");

    let store = Arc::new(tokio::sync::Mutex::new(store));
    // WHY: Actor has no in-memory session for "daemon:prosoche" — simulates
    // restart or eviction. The daemon bridge sends with session_id: None.
    let (handle, join, _dir) = spawn_test_actor_with_store(Arc::clone(&store));

    // NOTE: Daemon bridge calls send_turn (not send_turn_with_session_id),
    // so session_id is None — the actor must discover and adopt the DB ID.
    let result = handle
        .send_turn("daemon:prosoche", "Run your prosoche heartbeat check.")
        .await
        .expect("turn should succeed without FK constraint failure");
    assert_eq!(result.content, "Hello from actor!");

    let store_guard = store.lock().await;

    // WHY: Messages must be under the existing DB session ID, not a new ULID.
    let history = store_guard
        .get_history(existing_db_id, None)
        .expect("history under existing DB session ID");
    assert!(
        history.len() >= 2,
        "expected at least 2 messages under existing DB session ID, got {}",
        history.len()
    );

    // WHY: No orphan session should be created — only one session for
    // "daemon:prosoche" should exist.
    let all_sessions = store_guard
        .list_sessions(Some("test-agent"))
        .expect("list sessions");
    assert_eq!(
        all_sessions.len(),
        1,
        "should have exactly 1 session (no orphan), got {}",
        all_sessions.len()
    );
    assert_eq!(
        all_sessions[0].id, existing_db_id,
        "surviving session must be the original DB session ID"
    );

    drop(store_guard);
    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

// ── cross-nous panic boundary tests (#3606) ──────────────────────────────────
