#![expect(clippy::expect_used, reason = "test assertions")]
use aletheia_hermeneus::test_utils::MockProvider;

use super::*;
use crate::message::NousLifecycle;

fn make_oikos() -> (tempfile::TempDir, Arc<Oikos>) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/syn")).expect("mkdir");
    std::fs::create_dir_all(root.join("nous/demiurge")).expect("mkdir");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    std::fs::write(root.join("nous/syn/SOUL.md"), "I am Syn.").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    std::fs::write(root.join("nous/demiurge/SOUL.md"), "I am Demiurge.").expect("write");
    let oikos = Arc::new(Oikos::from_root(root));
    (dir, oikos)
}

fn make_providers() -> Arc<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::new("Hello!").models(&["test-model"]),
    ));
    Arc::new(providers)
}

fn make_manager(oikos: Arc<Oikos>) -> NousManager {
    NousManager::new(
        make_providers(),
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
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        ..NousConfig::default()
    }
}

fn demiurge_config() -> NousConfig {
    NousConfig {
        id: "demiurge".to_owned(),
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        ..NousConfig::default()
    }
}

#[tokio::test]
async fn spawn_returns_handle() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    assert_eq!(handle.id(), "syn", "spawned handle should have syn id");
    assert_eq!(mgr.count(), 1, "manager should have one actor after spawn");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn get_finds_spawned_actor() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;

    let handle = mgr.get("syn").expect("found");
    assert_eq!(handle.id(), "syn", "retrieved handle should have syn id");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn get_returns_none_for_unknown() {
    let (_dir, oikos) = make_oikos();
    let mgr = make_manager(oikos);
    assert!(
        mgr.get("unknown").is_none(),
        "unknown id should return None"
    );
}

#[tokio::test]
async fn get_config_returns_stored_config() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;

    let config = mgr.get_config("syn").expect("config");
    assert_eq!(config.id, "syn", "config id should match");
    assert_eq!(
        config.generation.model, "test-model",
        "config model should match"
    );

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn get_config_returns_none_for_unknown() {
    let (_dir, oikos) = make_oikos();
    let mgr = make_manager(oikos);
    assert!(
        mgr.get_config("unknown").is_none(),
        "unknown id should return None"
    );
}

#[tokio::test]
async fn configs_returns_all() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await;

    let configs = mgr.configs();
    assert_eq!(configs.len(), 2, "should have two configs");

    let ids: Vec<&str> = configs.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&"syn"), "configs should include syn");
    assert!(ids.contains(&"demiurge"), "configs should include demiurge");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn list_returns_all_statuses() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await;

    let statuses = mgr.list().await;
    assert_eq!(statuses.len(), 2, "should list two actors");

    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"syn"), "statuses should include syn");
    assert!(
        ids.contains(&"demiurge"),
        "statuses should include demiurge"
    );

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn shutdown_all_stops_all_actors() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle1 = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    let handle2 = mgr
        .spawn(demiurge_config(), PipelineConfig::default())
        .await;

    mgr.shutdown_all().await;

    assert_eq!(
        mgr.count(),
        0,
        "manager should have zero actors after shutdown"
    );
    assert!(
        handle1.status().await.is_err(),
        "handle1 should be stopped after shutdown"
    );
    assert!(
        handle2.status().await.is_err(),
        "handle2 should be stopped after shutdown"
    );
}

#[tokio::test]
async fn spawn_multiple_actors() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await;

    assert_eq!(mgr.count(), 2, "manager should have two actors");

    let syn = mgr.get("syn").expect("syn");
    let dem = mgr.get("demiurge").expect("demiurge");

    let s1 = syn.status().await.expect("status");
    let s2 = dem.status().await.expect("status");
    assert_eq!(s1.lifecycle, NousLifecycle::Idle, "syn should be idle");
    assert_eq!(s2.lifecycle, NousLifecycle::Idle, "demiurge should be idle");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn spawn_replaces_existing_actor() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let old_handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    let new_handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;

    assert_eq!(mgr.count(), 1, "re-spawn should replace, not add");

    assert!(
        old_handle.status().await.is_err(),
        "old handle should be dead after replacement"
    );

    let status = new_handle.status().await.expect("status");
    assert_eq!(status.id, "syn", "new handle should have syn id");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn manager_turn_through_handle() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    let result = handle.send_turn("main", "Hello").await.expect("turn");
    assert_eq!(result.content, "Hello!", "turn should return mock response");

    mgr.shutdown_all().await;
}

/// `drain()` cancels all actors via the root token and awaits their exit.
#[tokio::test]
async fn drain_stops_all_actors() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle1 = mgr.spawn(syn_config(), PipelineConfig::default()).await;
    let handle2 = mgr
        .spawn(demiurge_config(), PipelineConfig::default())
        .await;

    // WHY: drain() takes &self: no mutable access needed.
    mgr.drain(Duration::from_secs(5)).await;

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
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;

    // WHY: Cancel via manager's root token directly (as drain() would do internally).
    mgr.cancel.cancel();

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if handle.status().await.is_err() {
                break;
            }
            // kanon:ignore TESTING/sleep-in-test reason = "polling loop waiting for real actor shutdown; pause+advance would freeze the actor's own timers"
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actor should stop when cancel token fires");
}

/// `drain()` with a very short timeout should warn and return, not panic.
#[tokio::test]
async fn drain_timeout_does_not_panic() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await;

    // NOTE: 1-nanosecond timeout: drain will warn but must not panic.
    mgr.drain(Duration::from_nanos(1)).await;
}

#[tokio::test]
async fn check_health_reports_alive_actors() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;

    let health = mgr.check_health().await;
    assert_eq!(health.len(), 1, "health map should have one entry");
    let syn_health = health.get("syn").expect("syn health");
    assert!(syn_health.alive, "healthy actor should be alive");
    assert_eq!(
        syn_health.panic_count, 0,
        "healthy actor should have zero panics"
    );

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn check_health_detects_dead_actor() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle = mgr.spawn(syn_config(), PipelineConfig::default()).await;

    handle.shutdown().await.expect("shutdown");
    // kanon:ignore TESTING/sleep-in-test reason = "waiting for actor task to fully stop before health check; real async shutdown cannot use pause+advance"
    tokio::time::sleep(Duration::from_millis(50)).await;

    let health = mgr.check_health().await;
    let syn_health = health.get("syn").expect("syn health");
    assert!(!syn_health.alive, "dead actor should not be alive");
}

/// An actor processing a long turn cannot respond to pings: the inbox is
/// occupied. `check_health` must report it alive as long as `active_turn`
/// is set, distinguishing "busy" from "dead".
#[tokio::test]
async fn check_health_busy_actor_reports_alive() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default()).await;

    // NOTE: Simulate: actor is mid-turn (flag set) but its inbox is closed (ping fails).
    mgr.actors
        .get("syn")
        .expect("actor registered")
        .active_turn
        .store(true, std::sync::atomic::Ordering::Release);

    let handle = mgr.actors.get("syn").expect("entry").handle.clone();
    handle.shutdown().await.expect("shutdown sent");
    // kanon:ignore TESTING/sleep-in-test reason = "waiting for actor task to stop before checking busy state; real async shutdown cannot use pause+advance"
    tokio::time::sleep(Duration::from_millis(50)).await;

    let health = mgr.check_health().await;
    assert!(
        health.get("syn").expect("syn health").alive,
        "busy actor (active_turn=true) must report alive even when ping fails"
    );

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
    assert_eq!(
        super::calculate_backoff(0),
        Duration::from_secs(5),
        "attempt 0 should be 5s base"
    );
    assert_eq!(
        super::calculate_backoff(1),
        Duration::from_secs(15),
        "attempt 1 should be 15s"
    );
    assert_eq!(
        super::calculate_backoff(2),
        Duration::from_secs(45),
        "attempt 2 should be 45s"
    );
    assert_eq!(
        super::calculate_backoff(3),
        Duration::from_secs(135),
        "attempt 3 should be 135s"
    );
    assert_eq!(
        super::calculate_backoff(4),
        Duration::from_secs(300),
        "attempt 4 should clamp to 300s"
    );
    assert_eq!(
        super::calculate_backoff(10),
        Duration::from_secs(300),
        "attempt 10 should clamp to 300s"
    );
}
