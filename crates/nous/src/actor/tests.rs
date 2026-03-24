#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#![expect(clippy::expect_used, reason = "test assertions")]
use tokio_util::sync::CancellationToken;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::{CompletionRequest, CompletionResponse};

use super::*;

use crate::handle::NousHandle;

fn test_config() -> NousConfig {
    NousConfig {
        id: "test-agent".to_owned(),
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
    static_assertions::assert_impl_all!(NousHandle: Send, Sync, Clone);
    static_assertions::assert_impl_all!(NousMessage: Send);
    static_assertions::assert_impl_all!(NousStatus: Send, Sync);
    static_assertions::assert_impl_all!(NousLifecycle: Send, Sync, Copy);
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
            dyn std::future::Future<Output = aletheia_hermeneus::error::Result<CompletionResponse>>
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
    store: Arc<tokio::sync::Mutex<aletheia_mneme::store::SessionStore>>,
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
    );
    (handle, join, dir)
}

/// Regression test for #758/#916/#923: session ID divergence.
///
/// Verifies that when pylon creates a DB session and passes its ID to the
/// actor, the finalize stage persists messages under the SAME session ID
/// (not a newly generated one).
#[tokio::test]
async fn session_id_adoption_prevents_fk_divergence() {
    let store = aletheia_mneme::store::SessionStore::open_in_memory().expect("in-memory store");
    let db_session_id = "db-ses-from-pylon";

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
