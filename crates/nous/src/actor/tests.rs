use std::sync::Mutex;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{
    CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
};
use tokio_util::sync::CancellationToken;

use super::*;

use crate::handle::NousHandle;

// --- Test infrastructure ---

struct MockProvider {
    // std::sync::Mutex is intentional — test mock, never crosses .await
    response: Mutex<CompletionResponse>,
}

impl LlmProvider for MockProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = aletheia_hermeneus::error::Result<CompletionResponse>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            Ok(self.response.lock().expect("lock poisoned").clone())
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["test-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn test_config() -> NousConfig {
    NousConfig {
        id: "test-agent".to_owned(),
        model: "test-model".to_owned(),
        ..NousConfig::default()
    }
}

fn test_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/test-agent")).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir");
    std::fs::write(root.join("nous/test-agent/SOUL.md"), "Test agent.").expect("write");
    let oikos = Arc::new(Oikos::from_root(root));
    (dir, oikos)
}

fn test_providers() -> Arc<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(MockProvider {
        response: Mutex::new(CompletionResponse {
            id: "resp-1".to_owned(),
            model: "test-model".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "Hello from actor!".to_owned(),
                citations: None,
            }],
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
                ..Usage::default()
            },
        }),
    }));
    Arc::new(providers)
}

fn spawn_test_actor() -> (NousHandle, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    let (dir, oikos) = test_oikos();
    let providers = test_providers();
    let tools = Arc::new(ToolRegistry::new());
    let config = test_config();
    let pipeline_config = PipelineConfig::default();

    let (handle, join) = spawn(
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

// --- Tests ---

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
    assert!(err.unwrap_err().to_string().contains("inbox closed"));
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
    // Only create shared/ with SOUL.md for cascade fallback
    std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
    std::fs::write(root.join("shared/SOUL.md"), "# Test Soul").expect("write");

    let oikos = Oikos::from_root(root);
    super::spawn::validate_workspace(&oikos, "test-agent")
        .await
        .unwrap();
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
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("SOUL.md"),
        "error should mention SOUL.md: {msg}"
    );
}

// --- Resilience tests ---

/// Mock provider that panics on every call.
struct PanickingProvider;

impl LlmProvider for PanickingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = aletheia_hermeneus::error::Result<CompletionResponse>,
                > + Send
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
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

    let (handle, join) = spawn(
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

    // First turn panics — should return an error, not kill the actor
    let result = handle.send_turn("main", "Hello").await;
    assert!(result.is_err(), "panicking turn should return error");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("panic") || msg.contains("pipeline"),
        "error should mention panic: {msg}"
    );

    // Actor is still alive — can respond to status
    let status = handle.status().await.expect("actor should still be alive");
    assert_eq!(status.panic_count, 1);
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    // Second panicking turn also returns error, actor still alive
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

    // Shut down the actor first
    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");

    // Ping should fail
    let result = handle.ping(std::time::Duration::from_millis(100)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn send_timeout_fires_when_inbox_full() {
    // Create a channel with capacity 1
    let (tx, _rx) = mpsc::channel(1);
    let handle = NousHandle::new("test-agent".to_owned(), tx.clone());

    // Fill the inbox — don't drop _rx so the channel stays open
    // Send one message to fill the single slot
    let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();
    tx.send(NousMessage::Turn {
        session_key: "main".to_owned(),
        content: "filler".to_owned(),
        reply: reply_tx,
    })
    .await
    .expect("fill inbox");

    // Now the inbox is full — send_turn_with_timeout should fail
    let result = handle
        .send_turn_with_timeout("main", "Hello", std::time::Duration::from_millis(50))
        .await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("inbox full"),
        "should report inbox full: {msg}"
    );
}

#[tokio::test]
async fn degraded_state_after_repeated_panics() {
    let (handle, join, _dir) = spawn_panicking_actor();

    // Trigger enough panics to enter degraded mode (5 in window)
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

    // Subsequent turn should get ServiceDegraded error
    let result = handle.send_turn("main", "more work").await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("degraded"), "should report degraded: {msg}");

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn background_task_reaping() {
    let (handle, join, _dir) = spawn_test_actor();

    // Run a turn — this may spawn background tasks (extraction etc.)
    // Even if no tasks spawn, the reaping code runs each loop iteration.
    let result = handle.send_turn("main", "Hello").await.expect("turn");
    assert_eq!(result.content, "Hello from actor!");

    // The actor is still responsive — reaping didn't break anything.
    let status = handle.status().await.expect("status");
    assert_eq!(status.lifecycle, NousLifecycle::Idle);

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}

#[tokio::test]
async fn status_includes_uptime() {
    let (handle, join, _dir) = spawn_test_actor();

    // Give the actor a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let status = handle.status().await.expect("status");
    // Uptime should be non-zero — the actor has been alive for at least a few ms
    assert!(
        !status.uptime.is_zero(),
        "uptime should be non-zero, got {:?}",
        status.uptime
    );

    handle.shutdown().await.expect("shutdown");
    join.await.expect("join");
}
