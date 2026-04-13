#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#![expect(clippy::expect_used, reason = "test assertions")]
use tokio_util::sync::CancellationToken;

use hermeneus::provider::LlmProvider;
use hermeneus::test_utils::MockProvider;
use hermeneus::types::{CompletionRequest, CompletionResponse};

use super::*;

use crate::handle::NousHandle;

fn test_config() -> NousConfig {
    NousConfig {
        id: Arc::from("test-agent"),
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        ..NousConfig::default()
    }
}

fn test_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/test-agent")).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    std::fs::write(root.join("nous/test-agent/SOUL.md"), "Test agent.").expect("write");
    let oikos = Arc::new(Oikos::from_root(root));
    (dir, oikos)
}

fn test_providers() -> Arc<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::new("Hello from actor!").models(&["test-model"]),
    ));
    Arc::new(providers)
}

fn spawn_test_actor() -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (handle, join, _active_turn) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
    );
    (handle, join, dir)
}

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

/// Mock provider that panics on every call.
struct PanickingProvider;

impl LlmProvider for PanickingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { panic!("deliberate test panic in pipeline") })
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "panicking-mock"
    }
}

fn panicking_providers() -> Arc<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(PanickingProvider));
    Arc::new(providers)
}

fn spawn_panicking_actor() -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = panicking_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (handle, join, _active_turn) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
    );
    (handle, join, dir)
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
        content: "filler".to_owned(),
        span: tracing::Span::current(),
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

fn spawn_test_actor_with_store(
    store: Arc<tokio::sync::Mutex<mneme::store::SessionStore>>,
) -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (handle, join, _active_turn) = spawn(
        config,
        pipeline_config,
        providers,
        tools,
        oikos,
        None,
        None,
        Some(store),
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        None,
        CancellationToken::new(),
        taxis::config::NousBehaviorConfig::default(),
    );
    (handle, join, dir)
}

// ── Direct actor construction helper ─────────────────────────────────────────

/// Build a bare `NousActor` for unit tests that exercise internal state
/// without running the actor loop. Returns the actor and the inbox sender
/// (kept alive so the receiver does not close).
fn make_test_actor(
    pipeline_config: PipelineConfig,
) -> (
    NousActor,
    mpsc::Sender<NousMessage>,
    tempfile::TempDir, // kept alive: drops would delete tempdir
) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let (tx, rx) = mpsc::channel(DEFAULT_INBOX_CAPACITY);
    let active_turn = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let actor = NousActor::new(
        "test-agent".to_owned(),
        config,
        pipeline_config,
        rx,
        None,
        CancellationToken::new(),
        providers,
        tools,
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        None,
        Vec::new(),
        active_turn,
        taxis::config::NousBehaviorConfig::default(),
    );
    (actor, tx, dir)
}

// ── turn.rs: mark_turn_active ─────────────────────────────────────────────────

#[test]
fn mark_turn_active_sets_active_lifecycle_and_session() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.channel.status, NousLifecycle::Idle);
    assert!(actor.active_session.is_none());
    assert!(!actor.runtime.active_turn.load(std::sync::atomic::Ordering::Acquire));

    actor.mark_turn_active("my-session");

    assert_eq!(actor.channel.status, NousLifecycle::Active);
    assert_eq!(actor.active_session.as_deref(), Some("my-session"));
    assert!(actor.runtime.active_turn.load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn mark_turn_active_auto_wakes_from_dormant() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    actor.channel.status = NousLifecycle::Dormant;

    actor.mark_turn_active("s");

    // After auto-wake + active set, lifecycle must be Active.
    assert_eq!(actor.channel.status, NousLifecycle::Active);
}

// ── turn.rs: record_pipeline_panic ───────────────────────────────────────────

#[test]
fn record_pipeline_panic_increments_count_and_records_timestamp() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.runtime.pipeline_panic_count, 0);
    assert!(actor.runtime.last_panic_at.is_none());
    assert!(actor.runtime.pipeline_panic_timestamps.is_empty());

    actor.record_pipeline_panic();

    assert_eq!(actor.runtime.pipeline_panic_count, 1);
    assert!(actor.runtime.last_panic_at.is_some());
    assert_eq!(actor.runtime.pipeline_panic_timestamps.len(), 1);
}

#[test]
fn record_pipeline_panic_multiple_increments_count() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    for _ in 0..3 {
        actor.record_pipeline_panic();
    }

    assert_eq!(actor.runtime.pipeline_panic_count, 3);
    assert_eq!(actor.runtime.pipeline_panic_timestamps.len(), 3);
}

#[test]
fn record_pipeline_panic_enters_degraded_at_threshold() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.channel.status, NousLifecycle::Idle);

    // DEGRADED_PANIC_THRESHOLD is 5 — trigger exactly 5 panics.
    for _ in 0..5 {
        actor.record_pipeline_panic();
    }

    assert_eq!(
        actor.channel.status,
        NousLifecycle::Degraded,
        "should enter degraded after 5 panics in window"
    );
}

#[test]
fn record_pipeline_panic_below_threshold_stays_idle() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    for _ in 0..4 {
        actor.record_pipeline_panic();
    }

    assert_eq!(actor.channel.status, NousLifecycle::Idle);
}

// ── turn.rs: record_drift_metrics ────────────────────────────────────────────

fn make_turn_result(
    output_tokens: u64,
    tool_calls: Vec<crate::pipeline::ToolCall>,
) -> crate::pipeline::TurnResult {
    crate::pipeline::TurnResult {
        content: "ok".to_owned(),
        tool_calls,
        usage: crate::pipeline::TurnUsage {
            input_tokens: 10,
            output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            llm_calls: 1,
        },
        signals: vec![],
        stop_reason: "end_turn".to_owned(),
        degraded: None,
    }
}

fn make_tool_call(name: &str, is_error: bool) -> crate::pipeline::ToolCall {
    crate::pipeline::ToolCall {
        id: "tc-1".to_owned(),
        name: name.to_owned(),
        input: serde_json::Value::Null,
        result: None,
        is_error,
        duration_ms: 10,
    }
}

#[test]
fn record_drift_metrics_creates_detector_for_new_session() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert!(actor.drift_detectors.is_empty());

    let result = make_turn_result(100, vec![]);
    actor.record_drift_metrics("my-session", &result);

    assert!(
        actor.drift_detectors.contains_key("my-session"),
        "detector should be created for new session"
    );
}

#[test]
fn record_drift_metrics_zero_tool_calls_produces_zero_error_rate() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    let result = make_turn_result(50, vec![]);
    // Should not panic; zero-division guard must hold.
    actor.record_drift_metrics("s", &result);
}

#[test]
fn record_drift_metrics_all_errored_calls_produces_rate_one() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    let calls = vec![
        make_tool_call("bash", true),
        make_tool_call("read", true),
    ];
    let result = make_turn_result(80, calls);
    // Should not panic; rate = 1.0.
    actor.record_drift_metrics("s", &result);
    // Detector created.
    assert!(actor.drift_detectors.contains_key("s"));
}

#[test]
fn record_drift_metrics_mixed_calls_partial_error_rate() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    let calls = vec![
        make_tool_call("bash", false),
        make_tool_call("read", true),  // 1 of 2 errors → rate = 0.5
    ];
    let result = make_turn_result(60, calls);
    actor.record_drift_metrics("s", &result);
    assert!(actor.drift_detectors.contains_key("s"));
}

#[test]
fn record_drift_metrics_accumulates_across_turns() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    for _ in 0..3 {
        let result = make_turn_result(50, vec![]);
        actor.record_drift_metrics("s", &result);
    }
    // Still only one detector for the session.
    assert_eq!(actor.drift_detectors.len(), 1);
}

// ── background.rs: record_background_panic ───────────────────────────────────

#[test]
fn record_background_panic_increments_count() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());
    assert_eq!(actor.runtime.background_panic_count, 0);

    actor.record_background_panic();

    assert_eq!(actor.runtime.background_panic_count, 1);
    assert_eq!(actor.runtime.background_panic_timestamps.len(), 1);
}

#[test]
fn record_background_panic_does_not_enter_degraded() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    for _ in 0..10 {
        actor.record_background_panic();
    }

    // Background panics never trigger degraded mode — only pipeline panics do.
    assert_eq!(
        actor.channel.status,
        NousLifecycle::Idle,
        "background panics must not enter degraded mode"
    );
    assert_eq!(actor.runtime.background_panic_count, 10);
}

// ── background.rs: reap_background_tasks ─────────────────────────────────────

#[tokio::test]
async fn reap_background_tasks_joins_completed_tasks() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    // Spawn a task that finishes immediately.
    actor.runtime.background_tasks.spawn(async { /* no-op */ });

    // Let it finish.
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

    // Yield until the spawned task panics and is collected.
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }

    actor.reap_background_tasks();

    assert_eq!(
        actor.runtime.background_panic_count,
        1,
        "panic in background task should increment background_panic_count"
    );
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

    actor.maybe_spawn_extraction("hello world", "response text");

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

    actor.maybe_spawn_extraction("hello world", "response");

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

    actor.maybe_spawn_extraction("short", "response");

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

    // Fill up the task set to the limit with tasks that block indefinitely.
    for _ in 0..MAX_SPAWNED_TASKS {
        actor
            .runtime
            .background_tasks
            .spawn(std::future::pending::<()>());
    }
    assert_eq!(actor.runtime.background_tasks.len(), MAX_SPAWNED_TASKS);

    actor.maybe_spawn_extraction("long enough content here", "response text here");

    // Limit was already reached; no additional task spawned.
    assert_eq!(
        actor.runtime.background_tasks.len(),
        MAX_SPAWNED_TASKS,
        "task should not be spawned when limit is reached"
    );
}

// ── background.rs: maybe_spawn_skill_analysis ────────────────────────────────

#[test]
fn maybe_spawn_skill_analysis_noop_on_empty_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    actor.maybe_spawn_skill_analysis(&[], "my-session");

    // No tasks spawned, no panics.
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

#[test]
fn maybe_spawn_skill_analysis_processes_successful_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let calls = vec![make_tool_call("bash", false)];
    // Should not panic; candidate tracker absorbs the call.
    actor.maybe_spawn_skill_analysis(&calls, "my-session");
    // No task spawned unless the candidate is promoted (first occurrence never promotes).
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

#[test]
fn maybe_spawn_skill_analysis_processes_errored_tool_calls() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    let calls = vec![make_tool_call("bash", true)];
    // Error calls are recorded as ToolCallRecord::errored; must not panic.
    actor.maybe_spawn_skill_analysis(&calls, "s");
    assert_eq!(actor.runtime.background_tasks.len(), 0);
}

// ── background.rs: maybe_spawn_distillation ──────────────────────────────────

#[tokio::test]
async fn maybe_spawn_distillation_skips_when_flag_already_set() {
    let (mut actor, _tx, _dir) = make_test_actor(PipelineConfig::default());

    // Pre-set the flag so the guard fires immediately.
    actor
        .runtime
        .distillation_in_progress
        .store(true, std::sync::atomic::Ordering::Release);

    actor.maybe_spawn_distillation("s").await;

    // Flag should still be true (we didn't touch it) and no task spawned.
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
    // No session store configured: try_spawn_distillation returns false.

    actor.maybe_spawn_distillation("s").await;

    // Flag must be cleared (not stuck) when no task was spawned.
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

    // Attach a session store but no matching session.
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
/// Before the fix, `find_or_create_session` would return the existing DB
/// session silently (ON CONFLICT DO NOTHING), but `finalize` would call
/// `append_message` with the actor's newly generated ID (no DB row) →
/// FOREIGN KEY constraint failure and silent data loss.
///
/// After the fix, the actor adopts the DB session ID returned by
/// `find_or_create_session`, so finalize uses the correct ID.
#[tokio::test]
async fn prosoche_daemon_adopts_existing_db_session_id() {
    let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    // WHY: SessionId requires UUID v4 format
    let existing_db_id = "660e8400-e29b-41d4-a716-446655440001";

    // Simulate an existing DB session for the "daemon:prosoche" key
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
