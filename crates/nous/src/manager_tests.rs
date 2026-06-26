#![expect(clippy::expect_used, reason = "test assertions")]
use hermeneus::test_utils::MockProvider;

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
    make_manager_with_behavior(oikos, taxis::config::NousBehaviorConfig::default())
}

fn make_manager_with_behavior(
    oikos: Arc<Oikos>,
    behavior: taxis::config::NousBehaviorConfig,
) -> NousManager {
    make_manager_with_behavior_and_router(oikos, behavior, None)
}

fn make_manager_with_router(
    oikos: Arc<Oikos>,
    router: Arc<crate::cross::CrossNousRouter>,
) -> NousManager {
    make_manager_with_behavior_and_router(
        oikos,
        taxis::config::NousBehaviorConfig::default(),
        Some(router),
    )
}

fn make_manager_with_behavior_and_router(
    oikos: Arc<Oikos>,
    behavior: taxis::config::NousBehaviorConfig,
    router: Option<Arc<crate::cross::CrossNousRouter>>,
) -> NousManager {
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
        router,
        None,
        behavior,
        taxis::config::ToolLimitsConfig::default(),
    )
}

fn syn_config() -> NousConfig {
    NousConfig {
        id: Arc::from("syn"),
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        ..NousConfig::default()
    }
}

fn demiurge_config() -> NousConfig {
    NousConfig {
        id: Arc::from("demiurge"),
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        ..NousConfig::default()
    }
}

fn private_demiurge_config() -> NousConfig {
    NousConfig {
        private: true,
        ..demiurge_config()
    }
}

#[tokio::test]
async fn spawn_returns_handle() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    assert_eq!(handle.id(), "syn", "spawned handle should have syn id");
    assert_eq!(mgr.count(), 1, "manager should have one actor after spawn");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn register_agent_spawns_with_default_pipeline() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle = mgr
        .register_agent(syn_config())
        .await
        .expect("register_agent should succeed");
    assert_eq!(handle.id(), "syn", "registered handle should have syn id");
    assert_eq!(
        mgr.count(),
        1,
        "manager should have one actor after register"
    );

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn get_finds_spawned_actor() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

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

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    let config = mgr.get_config("syn").expect("config");
    assert_eq!(config.id.as_ref(), "syn", "config id should match");
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

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    let configs = mgr.configs();
    assert_eq!(configs.len(), 2, "should have two configs");

    let ids: Vec<&str> = configs.iter().map(|c| c.id.as_ref()).collect();
    assert!(ids.contains(&"syn"), "configs should include syn");
    assert!(ids.contains(&"demiurge"), "configs should include demiurge");

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn list_hides_private_statuses() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    mgr.spawn(private_demiurge_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    let statuses = mgr.list().await;
    assert_eq!(statuses.len(), 1, "public list should hide private actors");

    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"syn"), "statuses should include syn");
    assert!(
        !ids.contains(&"demiurge"),
        "statuses should hide private demiurge"
    );

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn list_all_includes_private_statuses() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    mgr.spawn(private_demiurge_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    let statuses = mgr.list_all().await;
    assert_eq!(
        statuses.len(),
        2,
        "operator list should include private actors"
    );

    let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"syn"), "statuses should include syn");
    assert!(
        ids.contains(&"demiurge"),
        "statuses should include private demiurge"
    );

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn spawn_registers_private_address_mask_and_rejects_unsolicited_knowledge() {
    let (_dir, oikos) = make_oikos();
    let router = Arc::new(crate::cross::CrossNousRouter::default());
    let mut mgr = make_manager_with_router(oikos, Arc::clone(&router));

    mgr.spawn(private_demiurge_config(), PipelineConfig::default())
        .await
        .expect("spawn private demiurge");

    assert_eq!(
        router.address_mask("demiurge").await,
        crate::cross::AddressMask::OperatorOnly,
        "private nous should register an operator-only inbound mask"
    );

    let requester = koina::id::NousId::new("syn").expect("valid requester id");
    let proposal = crate::cross::knowledge::verify_message(
        "syn",
        "demiurge",
        "shared claim needs verification",
        requester,
        Duration::from_secs(1),
    );
    let err = router
        .send(proposal)
        .await
        .expect_err("private nous should reject unsolicited peer knowledge proposal");
    assert!(
        matches!(
            &err,
            crate::error::Error::AddressRejected { from, to, .. }
                if from == "syn" && to == "demiurge"
        ),
        "expected address rejection, got {err:?}"
    );

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn spawn_registers_public_shared_address_mask_and_allows_knowledge_delivery() {
    let (_dir, oikos) = make_oikos();
    let router = Arc::new(crate::cross::CrossNousRouter::default());
    let mut mgr = make_manager_with_router(oikos, Arc::clone(&router));

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn shared public syn");

    assert_eq!(
        router.address_mask("syn").await,
        crate::cross::AddressMask::Public,
        "non-private shared nous should register a public inbound mask"
    );

    let proposal =
        crate::cross::knowledge::published_message("demiurge", "syn", "shared-1", "summary");
    let state = router
        .send(proposal)
        .await
        .expect("public/shared nous should accept peer knowledge proposal");
    assert_eq!(state, crate::cross::DeliveryState::Delivered);

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn shutdown_all_stops_all_actors() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle1 = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    let handle2 = mgr
        .spawn(demiurge_config(), PipelineConfig::default())
        .await
        .expect("spawn");

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

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await
        .expect("spawn");

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

    let old_handle = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    let new_handle = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

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

    let handle = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    let result = handle.send_turn("main", "Hello").await.expect("turn");
    assert_eq!(result.content, "Hello!", "turn should return mock response");

    mgr.shutdown_all().await;
}

/// `drain()` cancels all actors via the root token and awaits their exit.
#[tokio::test]
async fn drain_stops_all_actors() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle1 = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    let handle2 = mgr
        .spawn(demiurge_config(), PipelineConfig::default())
        .await
        .expect("spawn");

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

    let handle = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

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

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");
    mgr.spawn(demiurge_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    // NOTE: 1-nanosecond timeout: drain will warn but must not panic.
    mgr.drain(Duration::from_nanos(1)).await;
}

#[tokio::test]
async fn check_health_reports_alive_actors() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    let health = mgr.check_health().await;
    assert_eq!(health.len(), 1, "health map should have one entry");
    let syn_health = health.get("syn").expect("syn health");
    assert!(syn_health.alive, "healthy actor should be alive");
    assert_eq!(
        syn_health.panic_count, 0,
        "healthy actor should have zero panics"
    );
    assert_eq!(
        syn_health.background_failure_total_count, 0,
        "healthy actor should have zero background failures"
    );
    assert_eq!(
        syn_health.background_failure_recent_count, 0,
        "healthy actor should have zero recent background failures"
    );
    assert!(
        !syn_health.background_health_degraded,
        "healthy actor should not have degraded background health"
    );

    mgr.shutdown_all().await;
}

#[tokio::test]
async fn check_health_detects_dead_actor() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    let handle = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

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

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    // NOTE: Simulate: actor is mid-turn (flag set) but its inbox is closed (ping fails).
    {
        let actors = &mgr.actors;
        let entry = actors.get("syn").expect("actor registered");
        entry
            .active_turn
            .read()
            .expect("active_turn lock")
            .store(true, std::sync::atomic::Ordering::Release);
        // WHY: set a recent turn_started_at_ms so stuck-turn detection doesn't trigger
        #[expect(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            reason = "u128→u64: test uptime in ms won't exceed u64::MAX"
        )]
        let now_ms = entry
            .last_start
            .lock()
            .expect("last_start lock")
            .elapsed()
            .as_millis() as u64; // kanon:ignore RUST/as-cast
        entry
            .turn_started_at_ms
            .read()
            .expect("turn_started_at_ms lock")
            .store(now_ms, std::sync::atomic::Ordering::Release);
    }

    let handle = mgr.get("syn").expect("entry");
    handle.shutdown().await.expect("shutdown sent");
    // kanon:ignore TESTING/sleep-in-test reason = "waiting for actor task to stop before checking busy state; real async shutdown cannot use pause+advance"
    tokio::time::sleep(Duration::from_millis(50)).await;

    let health = mgr.check_health().await;
    assert!(
        health.get("syn").expect("syn health").alive,
        "busy actor (active_turn=true) must report alive even when ping fails"
    );

    {
        let actors = &mgr.actors;
        let entry = actors.get("syn").expect("actor registered");
        entry
            .active_turn
            .read()
            .expect("active_turn lock")
            .store(false, std::sync::atomic::Ordering::Release);
    }

    let health = mgr.check_health().await;
    assert!(
        !health.get("syn").expect("syn health").alive,
        "dead actor with active_turn=false must report not alive"
    );
}

/// `shutdown_all` must honour `shutdown_timeout_secs`: an actor whose task
/// outlives the timeout is aborted via `JoinHandle::abort` and `shutdown_all`
/// returns within `timeout + slack`. (#3382)
#[tokio::test]
async fn shutdown_all_timeout_aborts_stuck_actor() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    // Spawn a real actor so the manager sees a normal ActorEntry.
    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    // Swap the actor's join handle for a task that sleeps far longer than
    // the shutdown budget. This simulates an actor blocked on a long-running
    // turn: its run loop cannot observe the Shutdown message or cancel token
    // until the sleep returns.
    let blocking_join: JoinHandle<()> = tokio::spawn(async {
        // WHY: 1 hour sleep stands in for a stuck turn; if the shutdown
        // timeout path fails to abort it the test hangs for an hour.
        tokio::time::sleep(Duration::from_hours(1)).await;
    });
    let blocking_abort = blocking_join.abort_handle();
    {
        let actors = &mgr.actors;
        let entry = actors.get("syn").expect("actor registered");
        let mut guard = entry
            .join
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Drop the real join handle; we don't need it for this test — the
        // real actor is still alive, which is fine: it stops when the manager
        // drops at end of test.
        if let Some(original) = guard.take() {
            original.abort();
        }
        *guard = Some(blocking_join);
    }

    let timeout = Duration::from_millis(250);
    let started = std::time::Instant::now();
    mgr.shutdown_all_with_timeout(timeout).await;
    let elapsed = started.elapsed();

    // Hard upper bound: must not take longer than timeout + 2s slack. Task
    // scheduling + teardown adds a small amount over the raw timeout, but a
    // failure to abort would leave the test hanging for 3600s.
    assert!(
        elapsed < timeout + Duration::from_secs(2),
        "shutdown_all took {elapsed:?}, expected < {:?}",
        timeout + Duration::from_secs(2)
    );
    // The stuck task must have been aborted.
    assert!(
        blocking_abort.is_finished(),
        "stuck actor task should have been aborted by shutdown timeout"
    );
    assert_eq!(
        mgr.count(),
        0,
        "manager should have zero actors after shutdown"
    );
}

/// `shutdown_all` with a normal actor (no stuck turn) completes well within
/// the configured budget and does not abort the task. Regression guard
/// against the timeout path accidentally aborting healthy shutdowns. (#3382)
#[tokio::test]
async fn shutdown_all_completes_cleanly_under_budget() {
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager(oikos);

    mgr.spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    let started = std::time::Instant::now();
    mgr.shutdown_all_with_timeout(Duration::from_secs(5)).await;
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "healthy shutdown took {elapsed:?}, expected well under 5s budget"
    );
    assert_eq!(mgr.count(), 0, "manager should be empty after shutdown");
}

#[test]
fn backoff_calculation() {
    let max_secs = taxis::config::NousBehaviorConfig::default().manager_max_restart_backoff_secs;
    assert_eq!(
        super::calculate_backoff(0, max_secs),
        Duration::from_secs(5),
        "attempt 0 should be 5s base"
    );
    assert_eq!(
        super::calculate_backoff(1, max_secs),
        Duration::from_secs(15),
        "attempt 1 should be 15s"
    );
    assert_eq!(
        super::calculate_backoff(2, max_secs),
        Duration::from_secs(45),
        "attempt 2 should be 45s"
    );
    assert_eq!(
        super::calculate_backoff(3, max_secs),
        Duration::from_secs(135),
        "attempt 3 should be 135s"
    );
    assert_eq!(
        super::calculate_backoff(4, max_secs),
        Duration::from_mins(5),
        "attempt 4 should clamp to 300s"
    );
    assert_eq!(
        super::calculate_backoff(10, max_secs),
        Duration::from_mins(5),
        "attempt 10 should clamp to 300s"
    );
}

/// Supervisor spawns the inner poller once and exits cleanly when cancelled.
#[tokio::test]
async fn health_poller_supervisor_runs_until_cancelled() {
    let cancel = CancellationToken::new();
    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let c = Arc::clone(&calls);

    let cancel_for_supervisor = cancel.clone();
    let supervisor = tokio::spawn(async move {
        super::supervise_health_poller(
            move || {
                let c = Arc::clone(&c);
                tokio::spawn(async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_hours(1)).await;
                })
            },
            cancel_for_supervisor.child_token(),
            Duration::from_millis(50),
            None,
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "poller should have been spawned exactly once"
    );

    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), supervisor).await;
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "poller should not have been restarted"
    );
}

/// When the inner poller panics, the supervisor catches the `JoinError`,
/// logs, and respawns after backoff. Without supervision a panicking
/// `health_cycle` would kill health checks permanently for all actors. (#3607)
#[tokio::test]
async fn health_poller_supervisor_restarts_on_panic() {
    let cancel = CancellationToken::new();
    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let c = Arc::clone(&calls);

    let cancel_for_supervisor = cancel.clone();
    let supervisor = tokio::spawn(async move {
        super::supervise_health_poller(
            move || {
                let c = Arc::clone(&c);
                tokio::spawn(async move {
                    assert!(
                        c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) != 0,
                        "simulated health poller panic"
                    );
                    tokio::time::sleep(Duration::from_hours(1)).await;
                })
            },
            cancel_for_supervisor.child_token(),
            Duration::from_millis(50),
            None,
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        calls.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "supervisor should have restarted poller at least once, got {}",
        calls.load(std::sync::atomic::Ordering::SeqCst)
    );

    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), supervisor).await;
}

/// `start_health_poller` on an `Arc<NousManager>` detects and restarts a dead
/// actor without the caller invoking `check_health` or `health_cycle` directly.
#[tokio::test]
async fn health_poller_restarts_dead_actor() {
    let behavior = taxis::config::NousBehaviorConfig {
        manager_ping_timeout_secs: 0,
        manager_dead_threshold: 1,
        manager_max_restart_backoff_secs: 0,
        manager_restart_drain_timeout_secs: 0,
        ..taxis::config::NousBehaviorConfig::default()
    };
    let (_dir, oikos) = make_oikos();
    let mut mgr = make_manager_with_behavior(oikos, behavior);

    let handle = mgr
        .spawn(syn_config(), PipelineConfig::default())
        .await
        .expect("spawn");

    let mgr = Arc::new(mgr);
    let cancel = CancellationToken::new();
    let _poller = NousManager::start_health_poller(
        Arc::clone(&mgr),
        Duration::from_millis(20),
        cancel.child_token(),
    );

    // Wait for the actor to be ready before killing it.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if handle.status().await.is_ok() {
                break;
            }
            // kanon:ignore TESTING/sleep-in-test reason = "polling loop waiting for actor startup; real async spawn cannot use pause+advance"
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actor should become ready");

    // Kill the actor. The poller should notice and restart it.
    handle.shutdown().await.expect("shutdown");

    let restarted = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(handle) = mgr.get("syn")
                && handle.status().await.is_ok()
            {
                break;
            }
            // kanon:ignore TESTING/sleep-in-test reason = "polling loop waiting for poller to restart dead actor; real async health cycle cannot use pause+advance"
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(restarted.is_ok(), "poller should have restarted dead actor");

    let snapshot = mgr.poller_snapshot();
    assert!(snapshot.running, "poller supervisor should be running");

    cancel.cancel();
}
