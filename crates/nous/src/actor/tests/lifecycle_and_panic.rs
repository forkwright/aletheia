#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

#[tokio::test]
async fn turn_processes_and_returns_result() {
    let (handle, join, _dir) = spawn_test_actor();

    let result = handle.send_turn("main", "Hello").await.expect("turn");
    assert_eq!(result.content, "Hello from actor!");
    assert_eq!(result.usage.llm_calls, 1);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn status_returns_snapshot() {
    let (handle, join, _dir) = spawn_test_actor();

    let status = handle.status().await.expect("status");
    assert_eq!(status.id, "test-agent");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);
    assert_eq!(status.session_count, 0);
    assert!(status.active_session.is_none());

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn sleep_transitions_to_dormant() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.sleep().await.expect("sleep");
    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Dormant);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn wake_transitions_to_idle() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.sleep().await.expect("sleep");
    handle.wake().await.expect("wake");
    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn dormant_auto_wakes_on_turn() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.sleep().await.expect("sleep");
    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Dormant);

    let result = handle.send_turn("main", "Wake up").await.expect("turn");
    assert_eq!(result.content, "Hello from actor!");

    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn shutdown_exits_loop() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn actor_exits_when_all_handles_dropped() {
    let (handle, join, _dir) = spawn_test_actor();
    drop(handle);
    join.await.expect("join");
}

#[tokio::test]
async fn multiple_sequential_turns() {
    let (handle, join, _dir) = spawn_test_actor();

    for i in 0..5 {
        let result = handle
            .send_turn("main", format!("Turn {i}"))
            .await
            .expect("turn");
        assert_eq!(result.content, "Hello from actor!");
    }

    let status = handle.status().await.expect("status");
    assert_eq!(status.session_count, 1);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn turn_creates_session_for_unknown_key() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.send_turn("session-a", "Hello").await.expect("turn");
    handle.send_turn("session-b", "World").await.expect("turn");

    let status = handle.status().await.expect("status");
    assert_eq!(status.session_count, 2);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn status_after_turn_shows_idle_and_session() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.send_turn("main", "Hello").await.expect("turn");

    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);
    assert_eq!(status.session_count, 1);
    assert!(status.active_session.is_none());

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn sleep_then_sleep_is_idempotent() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.sleep().await.expect("sleep");
    handle.sleep().await.expect("sleep again");
    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Dormant);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn wake_when_idle_is_noop() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.wake().await.expect("wake");
    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn send_after_shutdown_returns_error() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");

    let err = handle.send_turn("main", "Hello").await;
    assert!(err.is_err());
    assert!(
        err.expect_err("send after shutdown should fail")
            .to_string()
            .contains("inbox closed")
    );
}

#[tokio::test]
async fn handle_clone_works() {
    let (handle, join, _dir) = spawn_test_actor();

    let handle2 = handle.clone();
    let status = handle2.status().await.expect("status");
    assert_eq!(status.id, "test-agent");

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[test]
fn send_sync_assertions() {
    const _: fn() = || {
        fn assert_send_sync_clone<T: Send + Sync + Clone>() {}
        fn assert_send<T: Send>() {}
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_send_sync_copy<T: Send + Sync + Copy>() {}
        assert_send_sync_clone::<NousHandle>();
        assert_send::<NousMessage>();
        assert_send_sync::<NousStatus>();
        assert_send_sync_copy::<NousLifecycle>();
    };
}

#[test]
fn default_inbox_capacity_is_32() {
    assert_eq!(DEFAULT_INBOX_CAPACITY, 32);
}

#[tokio::test]
async fn validate_workspace_creates_missing_dir() {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    std::fs::write(root.join("shared/SOUL.md"), "# Test Soul").expect("write");

    let oikos = Oikos::from_root(root);
    super::spawn::validate_workspace(&oikos, "test-agent")
        .await
        .expect("validate_workspace should create missing agent dir");
    assert!(root.join("nous/test-agent").exists());
}

#[tokio::test]
async fn validate_workspace_fails_without_soul() {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/test-agent")).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");

    let oikos = Oikos::from_root(root);
    let result = super::spawn::validate_workspace(&oikos, "test-agent").await;
    assert!(result.is_err());
    let msg = result
        .expect_err("missing SOUL.md should fail validation")
        .to_string();
    assert!(
        msg.contains("SOUL.md"),
        "error should mention SOUL.md: {msg}"
    );
}

#[tokio::test]
async fn actor_survives_pipeline_panic() {
    let (handle, join, _dir) = spawn_panicking_actor();

    let result = handle.send_turn("main", "Hello").await;
    assert!(result.is_err(), "panicking turn should return error");
    let msg = result
        .expect_err("panicking turn should return error")
        .to_string();
    assert!(
        msg.contains("panic") || msg.contains("pipeline"),
        "error should mention panic: {msg}"
    );

    let status = handle.status().await.expect("actor should still be alive");
    assert_eq!(status.panic_count, 1);
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    let result2 = handle.send_turn("main", "Hello again").await;
    assert!(result2.is_err());

    let status2 = handle
        .status()
        .await
        .expect("actor still alive after 2 panics");
    assert_eq!(status2.panic_count, 2);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn ping_pong_liveness() {
    let (handle, join, _dir) = spawn_test_actor();

    let result = handle.ping(std::time::Duration::from_secs(5)).await;
    assert!(result.is_ok(), "ping should succeed on healthy actor");

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn ping_fails_on_dead_actor() {
    let (handle, join, _dir) = spawn_test_actor();

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");

    let result = handle.ping(std::time::Duration::from_millis(100)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn send_timeout_fires_when_inbox_full() {
    let (tx, _rx) = mpsc::channel(1);
    let handle = NousHandle::new("test-agent".to_owned(), tx.clone());

    // WHY: don't drop _rx -- dropping closes the channel and the send below would fail.
    let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();
    tx.send(NousMessage::Turn {
        session_key: "main".to_owned(),
        session_id: None,
        request_id: None,
        content: "filler".to_owned(),
        span: tracing::Span::current(),
        turn_cancel: tokio_util::sync::CancellationToken::new(),
        reply: reply_tx,
    })
    .await
    .expect("fill inbox");

    let result = handle
        .send_turn_with_timeout("main", "Hello", std::time::Duration::from_millis(50))
        .await;
    assert!(result.is_err());
    let msg = result
        .expect_err("full inbox should reject send")
        .to_string();
    assert!(
        msg.contains("inbox full"),
        "should report inbox full: {msg}"
    );
}

#[tokio::test]
async fn degraded_state_after_repeated_panics() {
    let (handle, join, _dir) = spawn_panicking_actor();

    for i in 0..5 {
        let result = handle.send_turn("main", &format!("panic {i}")).await;
        assert!(result.is_err());
    }

    let status = handle.status().await.expect("status");
    assert_eq!(
        status.lifecycle,
        NousLifecycle::Degraded,
        "should be degraded after 5 panics"
    );
    assert_eq!(status.panic_count, 5);

    let result = handle.send_turn("main", "more work").await;
    assert!(result.is_err());
    let msg = result
        .expect_err("degraded actor should reject work")
        .to_string();
    assert!(msg.contains("degraded"), "should report degraded: {msg}");

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn background_task_reaping() {
    let (handle, join, _dir) = spawn_test_actor();

    let result = handle.send_turn("main", "Hello").await.expect("turn");
    assert_eq!(result.content, "Hello from actor!");

    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn status_includes_uptime() {
    let (handle, join, _dir) = spawn_test_actor();

    // kanon:ignore TESTING/sleep-in-test reason = "uptime is measured by std::time::Instant; tokio::time::advance does not affect it"
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let status = handle.status().await.expect("status");
    assert!(
        !status.uptime.is_zero(),
        "uptime should be non-zero, got {:?}",
        status.uptime
    );

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

// ── empirical router: after-action recording ────────────────────────────────

use std::time::Duration;

use aletheia_routing::types::{ProviderId, TaskCategory};
use aletheia_routing::{AfterActionStore, RecordingRouter};

#[tokio::test]
async fn turn_records_after_action_outcome_in_empirical_store() {
    let store = Arc::new(AfterActionStore::in_memory());
    let router: Arc<dyn aletheia_routing::Router> =
        Arc::new(RecordingRouter::new(Arc::clone(&store), "test-model"));

    let (handle, join, _dir) = spawn_test_actor_with_router(Some(router));

    let result = handle
        .send_turn("main", "Build a feature")
        .await
        .expect("turn");
    assert_eq!(result.content, "Hello from actor!");

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");

    let provider = ProviderId::new("test-model");
    for _ in 0..20 {
        if let Some(stats) = store
            .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(168))
            .await
            .expect("rolling stats query")
        {
            assert_eq!(stats.successes, 1);
            assert_eq!(stats.failures, 0);
            assert_eq!(stats.total, 1);
            return;
        }
        tokio::task::yield_now().await;
    }

    panic!("completed interactive turn did not record after-action outcome");
}
