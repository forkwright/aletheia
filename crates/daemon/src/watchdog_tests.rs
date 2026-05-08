#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
#![expect(
    clippy::unchecked_time_subtraction,
    reason = "test: subtracting small millis from now cannot underflow"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use tracing::Instrument;

use super::*;

struct MockProcess {
    id: String,
    kill_called: AtomicBool,
    restart_called: AtomicBool,
    restart_fail: AtomicBool,
    restart_count: AtomicU32,
}

impl MockProcess {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            kill_called: AtomicBool::new(false),
            restart_called: AtomicBool::new(false),
            restart_fail: AtomicBool::new(false),
            restart_count: AtomicU32::new(0),
        }
    }

    fn set_restart_fail(&self, fail: bool) {
        self.restart_fail.store(fail, Ordering::Relaxed);
    }
}

impl ProcessHandle for MockProcess {
    fn id(&self) -> &str {
        &self.id
    }

    fn kill(&self) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + '_>> {
        self.kill_called.store(true, Ordering::Relaxed);
        Box::pin(async { Ok(()) })
    }

    fn restart(&self) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + '_>> {
        self.restart_called.store(true, Ordering::Relaxed);
        self.restart_count.fetch_add(1, Ordering::Relaxed);
        let should_fail = self.restart_fail.load(Ordering::Relaxed);
        Box::pin(async move {
            if should_fail {
                crate::error::TaskFailedSnafu {
                    task_id: "mock-restart".to_owned(),
                    reason: "simulated restart failure".to_owned(),
                }
                .fail()
            } else {
                Ok(())
            }
        })
    }
}

#[test]
fn watchdog_config_default_values() {
    let config = WatchdogConfig::default();
    assert_eq!(config.heartbeat_timeout, Duration::from_mins(1));
    assert_eq!(config.check_interval, Duration::from_secs(10));
    assert_eq!(config.max_restarts, 5);
}

#[test]
fn register_adds_process() {
    let token = CancellationToken::new();
    let mut wd = Watchdog::new(WatchdogConfig::default(), token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    wd.register(proc);

    let statuses = wd.status();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].id, "agent-1");
    assert_eq!(statuses[0].state, ProcessState::Healthy);
}

#[test]
fn heartbeat_updates_timestamp() {
    let token = CancellationToken::new();
    let mut wd = Watchdog::new(WatchdogConfig::default(), token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    wd.register(proc);

    std::thread::sleep(Duration::from_millis(10));
    wd.heartbeat("agent-1");

    let statuses = wd.status();
    assert!(
        statuses[0].last_heartbeat_secs < 1,
        "heartbeat should reset the timer"
    );
}

#[tokio::test]
async fn detects_hung_process() {
    let token = CancellationToken::new();
    let config = WatchdogConfig {
        heartbeat_timeout: Duration::from_millis(50),
        check_interval: Duration::from_millis(10),
        max_restarts: 5,
        ..WatchdogConfig::default()
    };
    let mut wd = Watchdog::new(config, token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    wd.register(proc.clone());

    // WHY: artificially age the heartbeat to simulate a hang.
    wd.processes.get_mut("agent-1").unwrap().last_heartbeat =
        Instant::now() - Duration::from_millis(100);

    wd.check_processes().await;

    assert!(
        proc.kill_called.load(Ordering::Relaxed),
        "hung process should be killed"
    );
    assert!(
        proc.restart_called.load(Ordering::Relaxed),
        "hung process should be restarted"
    );

    let statuses = wd.status();
    assert_eq!(
        statuses[0].state,
        ProcessState::Healthy,
        "process should be healthy after successful restart"
    );
    assert_eq!(statuses[0].restart_count, 1);
}

#[tokio::test]
async fn restart_failure_applies_backoff() {
    let token = CancellationToken::new();
    let config = WatchdogConfig {
        heartbeat_timeout: Duration::from_millis(50),
        check_interval: Duration::from_millis(10),
        max_restarts: 5,
        ..WatchdogConfig::default()
    };
    let mut wd = Watchdog::new(config, token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    proc.set_restart_fail(true);
    wd.register(proc);

    wd.processes.get_mut("agent-1").unwrap().last_heartbeat =
        Instant::now() - Duration::from_millis(100);

    wd.check_processes().await;

    let internal = wd.processes.get("agent-1").unwrap();
    assert_eq!(internal.state, ProcessState::Hung);
    assert!(
        internal.backoff_until.is_some(),
        "failed restart should set backoff"
    );
}

#[tokio::test]
async fn max_restarts_abandons_process() {
    let token = CancellationToken::new();
    let config = WatchdogConfig {
        heartbeat_timeout: Duration::from_millis(50),
        check_interval: Duration::from_millis(10),
        max_restarts: 2,
        ..WatchdogConfig::default()
    };
    let mut wd = Watchdog::new(config, token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    wd.register(proc);

    // Simulate exceeding max restarts.
    wd.processes.get_mut("agent-1").unwrap().restart_count = 2;
    wd.processes.get_mut("agent-1").unwrap().state = ProcessState::Hung;

    wd.check_processes().await;

    let statuses = wd.status();
    assert_eq!(
        statuses[0].state,
        ProcessState::Abandoned,
        "should be abandoned after exceeding max restarts"
    );
}

#[tokio::test]
async fn backoff_prevents_immediate_retry() {
    let token = CancellationToken::new();
    let config = WatchdogConfig {
        heartbeat_timeout: Duration::from_millis(50),
        check_interval: Duration::from_millis(10),
        max_restarts: 5,
        ..WatchdogConfig::default()
    };
    let mut wd = Watchdog::new(config, token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    wd.register(proc.clone());

    // Set up: hung with backoff in the future.
    wd.processes.get_mut("agent-1").unwrap().state = ProcessState::Hung;
    wd.processes.get_mut("agent-1").unwrap().backoff_until =
        Some(Instant::now() + Duration::from_mins(5));

    wd.check_processes().await;

    assert_eq!(
        proc.restart_count.load(Ordering::Relaxed),
        0,
        "process in backoff should not be restarted"
    );
}

#[test]
fn watchdog_backoff_exponential_growth() {
    assert_eq!(watchdog_backoff(1), Duration::from_secs(2));
    assert_eq!(watchdog_backoff(2), Duration::from_secs(4));
    assert_eq!(watchdog_backoff(3), Duration::from_secs(8));
    assert_eq!(watchdog_backoff(4), Duration::from_secs(16));
    assert_eq!(watchdog_backoff(5), Duration::from_secs(32));
}

#[test]
fn watchdog_backoff_capped_at_max() {
    let delay = watchdog_backoff(20);
    assert_eq!(delay, BACKOFF_CAP, "backoff should be capped at 5 minutes");
}

#[test]
fn watchdog_backoff_zero_attempt() {
    assert_eq!(
        watchdog_backoff(0),
        BACKOFF_BASE,
        "attempt 0 should return base delay"
    );
}

#[test]
fn watchdog_backoff_honors_daemon_behavior() {
    let behavior = taxis::config::DaemonBehaviorConfig {
        watchdog_backoff_base_secs: 3,
        watchdog_backoff_cap_secs: 9,
        ..taxis::config::DaemonBehaviorConfig::default()
    };
    let config = WatchdogConfig::default().with_daemon_behavior(&behavior);

    assert_eq!(
        watchdog_backoff_with_config(1, &config),
        Duration::from_secs(3)
    );
    assert_eq!(
        watchdog_backoff_with_config(2, &config),
        Duration::from_secs(6)
    );
    assert_eq!(
        watchdog_backoff_with_config(3, &config),
        Duration::from_secs(9)
    );
}

#[tokio::test]
async fn shutdown_exits_watchdog_loop() {
    let token = CancellationToken::new();
    let mut wd = Watchdog::new(WatchdogConfig::default(), token.clone());

    let handle = tokio::spawn(
        async move {
            wd.run().await;
        }
        .instrument(tracing::info_span!("test_watchdog_shutdown")),
    );

    token.cancel();

    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
    assert!(result.is_ok(), "watchdog should exit on shutdown signal");
}

#[tokio::test]
async fn restart_log_records_events() {
    let token = CancellationToken::new();
    let config = WatchdogConfig {
        heartbeat_timeout: Duration::from_millis(50),
        check_interval: Duration::from_millis(10),
        max_restarts: 5,
        ..WatchdogConfig::default()
    };
    let mut wd = Watchdog::new(config, token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    wd.register(proc);

    wd.processes.get_mut("agent-1").unwrap().last_heartbeat =
        Instant::now() - Duration::from_millis(100);

    wd.check_processes().await;

    let log = wd.restart_log();
    assert_eq!(log.len(), 1, "should have one restart event");
    assert_eq!(log[0].process_id, "agent-1");
    assert_eq!(log[0].attempt, 1);
}

#[test]
fn report_exit_sets_hung_state() {
    let token = CancellationToken::new();
    let mut wd = Watchdog::new(WatchdogConfig::default(), token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    wd.register(proc);

    wd.report_exit("agent-1", "segfault");

    let statuses = wd.status();
    assert_eq!(statuses[0].state, ProcessState::Hung);
}

#[test]
fn heartbeat_recovers_from_hung() {
    let token = CancellationToken::new();
    let mut wd = Watchdog::new(WatchdogConfig::default(), token);
    let proc = Arc::new(MockProcess::new("agent-1"));
    wd.register(proc);

    wd.processes.get_mut("agent-1").unwrap().state = ProcessState::Hung;
    wd.heartbeat("agent-1");

    let statuses = wd.status();
    assert_eq!(
        statuses[0].state,
        ProcessState::Healthy,
        "heartbeat should recover from hung state"
    );
}

#[test]
fn restart_cause_display() {
    let timeout = RestartCause::HeartbeatTimeout { elapsed_secs: 75 };
    assert_eq!(
        timeout.to_string(),
        "heartbeat timeout (75s without heartbeat)"
    );

    let exited = RestartCause::ProcessExited {
        reason: "OOM".to_owned(),
    };
    assert_eq!(exited.to_string(), "process exited: OOM");
}

#[test]
fn process_state_serialization_roundtrip() {
    let state = ProcessState::Healthy;
    let json = serde_json::to_string(&state).expect("serialize");
    let back: ProcessState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, ProcessState::Healthy);
}

#[test]
fn restart_event_serialization_roundtrip() {
    let event = RestartEvent {
        process_id: "agent-1".to_owned(),
        cause: RestartCause::HeartbeatTimeout { elapsed_secs: 65 },
        attempt: 2,
        timestamp: "2026-03-22T10:00:00Z".to_owned(),
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let back: RestartEvent = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.process_id, "agent-1");
    assert_eq!(back.attempt, 2);
}
