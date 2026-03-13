#![expect(clippy::expect_used, reason = "test assertions")]
use std::sync::Mutex;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::{
    CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
};

use super::*;
use crate::message::NousLifecycle;

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
            dyn std::future::Future<Output = aletheia_hermeneus::error::Result<CompletionResponse>>
                + Send
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

fn test_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/syn")).expect("mkdir");
    std::fs::create_dir_all(root.join("nous/demiurge")).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir");
    std::fs::write(root.join("nous/syn/SOUL.md"), "I am Syn.").expect("write");
    std::fs::write(root.join("nous/demiurge/SOUL.md"), "I am Demiurge.").expect("write");
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
                text: "Hello!".to_owned(),
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

fn test_manager(oikos: Arc<Oikos>) -> NousManager {
    NousManager::new(
        test_providers(),
        Arc::new(ToolRegistry::new()),
        oikos,
        None,
        None,
        None,
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(Vec::new()),
        None,
        None,
    )
}

fn syn_config() -> NousConfig {
    NousConfig {
        id: "syn".to_owned(),
        model: "test-model".to_owned(),
        ..NousConfig::default()
    }
}

fn demiurge_config() -> NousConfig {
    NousConfig {
        id: "demiurge".to_owned(),
        model: "test-model".to_owned(),
        ..NousConfig::default()
    }
}

#[tokio::test]
async fn spawn_returns_handle() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    assert_eq!(handle.id(), "syn");
    assert_eq!(mgr.count(), 1);

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn get_finds_spawned_actor() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;

    let handle = mgr.get("syn").expect("found");
    assert_eq!(handle.id(), "syn");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn get_returns_none_for_unknown() {
    let (_dir, oikos) = test_oikos();
    let mgr = test_manager(oikos);
    assert!(mgr.get("unknown").is_none());
}

#[tokio::test]
async fn get_config_returns_stored_config() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;

    let config = mgr.get_config("syn").expect("config");
    assert_eq!(config.id, "syn");
    assert_eq!(config.model, "test-model");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn get_config_returns_none_for_unknown() {
    let (_dir, oikos) = test_oikos();
    let mgr = test_manager(oikos);
    assert!(mgr.get_config("unknown").is_none());
}

#[tokio::test]
async fn configs_returns_all() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await;

    let configs = mgr.configs();
    assert_eq!(configs.len(), 2);

    let ids: Vec<&str> = configs.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&"syn"));
    assert!(ids.contains(&"demiurge"));

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn list_returns_all_statuses() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await;

    let statuses = mgr.list().await;
    assert_eq!(statuses.len(), 2);

    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"syn"));
    assert!(ids.contains(&"demiurge"));

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn shutdown_all_stops_all_actors() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    let handle1 = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    let handle2 = mgr
        .spawn(demiurge_config(), PipelineConfig::default())
        .await;

    mgr.shutdown_all().await;

    assert_eq!(mgr.count(), 0);
    assert!(handle1.status().await.is_err());
    assert!(handle2.status().await.is_err());
}

#[tokio::test]
async fn spawn_multiple_actors() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await;

    assert_eq!(mgr.count(), 2);

    let syn = mgr.get("syn").expect("syn");
    let dem = mgr.get("demiurge").expect("demiurge");

    let s1 = syn.status().await.expect("status");
    let s2 = dem.status().await.expect("status");
    assert_eq!(s1.lifecycle, NousLifecycle::Idle);
    assert_eq!(s2.lifecycle, NousLifecycle::Idle);

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn spawn_replaces_existing_actor() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    let old_handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    let new_handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;

    assert_eq!(mgr.count(), 1);

    // Old handle should be disconnected
    assert!(old_handle.status().await.is_err());

    // New handle should work
    let status = new_handle.status().await.expect("status");
    assert_eq!(status.id, "syn");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn manager_turn_through_handle() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    let result = handle.send_turn("main", "Hello").await.expect("turn");
    assert_eq!(result.content, "Hello!");

    mgr.shutdown_all().await;
}

/// `drain()` cancels all actors via the root token and awaits their exit.
#[tokio::test]
async fn drain_stops_all_actors() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    let handle1 = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    let handle2 = mgr
        .spawn(demiurge_config(), PipelineConfig::default())
        .await;

    // drain() takes &self — no mutable access needed.
    mgr.drain(Duration::from_secs(5)).await;

    // After drain join handles are taken and tasks have stopped.
    assert!(
        handle1.status().await.is_err(),
        "syn actor should have exited"
    );
    assert!(
        handle2.status().await.is_err(),
        "demiurge actor should have exited"
    );
}

/// Cancelling the manager's root token reaches all actor child tokens.
#[tokio::test]
async fn cancel_token_propagates_to_actors() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;

    // Cancel via manager's root token directly (as drain() would do internally).
    mgr.cancel.cancel();

    // Wait for actor to observe cancellation and exit.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if handle.status().await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actor should stop when cancel token fires");
}

/// `drain()` with a very short timeout should warn and return, not panic.
#[tokio::test]
async fn drain_timeout_does_not_panic() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await;

    // 1-nanosecond timeout: drain will warn but must not panic.
    mgr.drain(Duration::from_nanos(1)).await;
}

// --- Resilience tests ---

#[tokio::test]
async fn check_health_reports_alive_actors() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;

    let health = mgr.check_health().await;
    assert_eq!(health.len(), 1);
    let syn_health = health.get("syn").expect("syn health");
    assert!(syn_health.alive, "healthy actor should be alive");
    assert_eq!(syn_health.panic_count, 0);

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn check_health_detects_dead_actor() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;

    // Kill the actor by sending shutdown directly
    handle.shutdown().await.expect("shutdown");
    // Wait for actor to stop
    tokio::time::sleep(Duration::from_millis(50)).await;

    let health = mgr.check_health().await;
    let syn_health = health.get("syn").expect("syn health");
    assert!(!syn_health.alive, "dead actor should not be alive");
}

/// An actor processing a long turn cannot respond to pings — the inbox is
/// occupied. `check_health` must report it alive as long as `active_turn`
/// is set, distinguishing "busy" from "dead".
#[tokio::test]
async fn check_health_busy_actor_reports_alive() {
    let (_dir, oikos) = test_oikos();
    let mut mgr = test_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;

    // Simulate: actor is mid-turn (flag set) but its inbox is closed (ping fails).
    mgr.actors
        .get("syn")
        .expect("actor registered")
        .active_turn
        .store(true, std::sync::atomic::Ordering::Release);

    // Kill the actor so the ping fails.
    let handle = mgr.actors.get("syn").expect("entry").handle.clone();
    handle.shutdown().await.expect("shutdown sent");
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Ping fails but active_turn is set — must report healthy-busy, not dead.
    let health = mgr.check_health().await;
    assert!(
        health.get("syn").expect("syn health").alive,
        "busy actor (active_turn=true) must report alive even when ping fails"
    );

    // Clear the flag: actor is now both dead and idle — must report unhealthy.
    mgr.actors
        .get("syn")
        .expect("actor registered")
        .active_turn
        .store(false, std::sync::atomic::Ordering::Release);

    let health = mgr.check_health().await;
    assert!(
        !health.get("syn").expect("syn health").alive,
        "dead actor with active_turn=false must report not alive"
    );
}

#[test]
fn backoff_calculation() {
    assert_eq!(super::calculate_backoff(0), Duration::from_secs(5));
    assert_eq!(super::calculate_backoff(1), Duration::from_secs(15));
    assert_eq!(super::calculate_backoff(2), Duration::from_secs(45));
    assert_eq!(super::calculate_backoff(3), Duration::from_secs(135));
    // After 4+ restarts, caps at 5 minutes
    assert_eq!(super::calculate_backoff(4), Duration::from_secs(300));
    assert_eq!(super::calculate_backoff(10), Duration::from_secs(300));
}
