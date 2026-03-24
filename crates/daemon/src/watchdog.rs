//! Watchdog process monitor with heartbeat tracking and auto-recovery.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Default heartbeat timeout before a process is considered hung.
const DEFAULT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(60);

/// Default interval between watchdog health check sweeps.
const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Maximum number of restarts before the watchdog stops retrying.
const DEFAULT_MAX_RESTARTS: u32 = 5;

/// Base delay for exponential backoff (2 seconds).
const BACKOFF_BASE: Duration = Duration::from_secs(2);

/// Maximum backoff delay cap (5 minutes).
const BACKOFF_CAP: Duration = Duration::from_secs(300);

/// Watchdog configuration.
#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// Maximum time without a heartbeat before a process is declared hung.
    pub heartbeat_timeout: Duration,
    /// How often the watchdog sweeps for hung processes.
    pub check_interval: Duration,
    /// Maximum number of restart attempts before giving up.
    pub max_restarts: u32,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            heartbeat_timeout: DEFAULT_HEARTBEAT_TIMEOUT,
            check_interval: DEFAULT_CHECK_INTERVAL,
            max_restarts: DEFAULT_MAX_RESTARTS,
        }
    }
}

/// Lifecycle handle for a monitored process.
///
/// Implemented by the binary crate where concrete process types are available.
pub trait ProcessHandle: Send + Sync {
    /// Unique identifier for this process.
    fn id(&self) -> &str;

    /// Forcefully terminate the process.
    fn kill(&self) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + '_>>;

    /// Restart the process. Returns `Ok(())` when the new instance is running.
    fn restart(&self) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + '_>>;
}

/// State of a watched process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProcessState {
    /// Process is healthy and sending heartbeats.
    Healthy,
    /// Process missed heartbeat deadline and is considered hung.
    Hung,
    /// Process is being restarted.
    Restarting,
    /// Process exceeded max restarts and is abandoned.
    Abandoned,
}

/// Restart event recorded by the watchdog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartEvent {
    /// Process identifier.
    pub process_id: String,
    /// What triggered the restart.
    pub cause: RestartCause,
    /// Which restart attempt this is (1-indexed).
    pub attempt: u32,
    /// ISO 8601 timestamp of the event.
    pub timestamp: String,
}

/// Reason a process was restarted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RestartCause {
    /// No heartbeat received within the configured timeout.
    HeartbeatTimeout {
        /// Seconds since the last heartbeat.
        elapsed_secs: u64,
    },
    /// The process exited unexpectedly.
    ProcessExited {
        /// Exit reason or error message.
        reason: String,
    },
}

impl std::fmt::Display for RestartCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeartbeatTimeout { elapsed_secs } => {
                write!(f, "heartbeat timeout ({elapsed_secs}s without heartbeat)")
            }
            Self::ProcessExited { reason } => {
                write!(f, "process exited: {reason}")
            }
        }
    }
}

/// Status snapshot for a watched process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStatus {
    /// Process identifier.
    pub id: String,
    /// Current state.
    pub state: ProcessState,
    /// Seconds since last heartbeat.
    pub last_heartbeat_secs: u64,
    /// Total number of restarts performed.
    pub restart_count: u32,
}

/// Internal tracking state for a watched process.
struct WatchedProcess {
    handle: std::sync::Arc<dyn ProcessHandle>,
    last_heartbeat: Instant,
    restart_count: u32,
    backoff_until: Option<Instant>,
    state: ProcessState,
}

/// Watchdog process monitor.
///
/// Tracks registered processes via heartbeats, detects hangs, and performs
/// auto-restart with exponential backoff.
pub struct Watchdog {
    processes: HashMap<String, WatchedProcess>,
    config: WatchdogConfig,
    shutdown: CancellationToken,
    restart_log: Vec<RestartEvent>,
}

impl Watchdog {
    /// Create a new watchdog with the given configuration.
    pub fn new(config: WatchdogConfig, shutdown: CancellationToken) -> Self {
        Self {
            processes: HashMap::new(),
            config,
            shutdown,
            restart_log: Vec::new(),
        }
    }

    /// Register a process for monitoring.
    pub fn register(&mut self, handle: std::sync::Arc<dyn ProcessHandle>) {
        let id = handle.id().to_owned();
        tracing::info!(process_id = %id, "watchdog: registered process");
        self.processes.insert(
            id,
            WatchedProcess {
                handle,
                last_heartbeat: Instant::now(),
                restart_count: 0,
                backoff_until: None,
                state: ProcessState::Healthy,
            },
        );
    }

    /// Record a heartbeat from a process.
    pub fn heartbeat(&mut self, process_id: &str) {
        if let Some(proc) = self.processes.get_mut(process_id) {
            proc.last_heartbeat = Instant::now();
            if proc.state == ProcessState::Hung {
                tracing::info!(
                    process_id = %process_id,
                    "watchdog: process recovered — heartbeat received"
                );
                proc.state = ProcessState::Healthy;
            }
        }
    }

    /// Report a process exit so the watchdog can schedule a restart.
    pub fn report_exit(&mut self, process_id: &str, reason: &str) {
        if let Some(proc) = self.processes.get_mut(process_id) {
            proc.state = ProcessState::Hung;
            tracing::warn!(
                process_id = %process_id,
                reason = %reason,
                "watchdog: process exit reported"
            );
        }
    }

    /// Get status snapshots for all watched processes.
    pub fn status(&self) -> Vec<ProcessStatus> {
        self.processes
            .iter()
            .map(|(id, proc)| ProcessStatus {
                id: id.clone(),
                state: proc.state,
                last_heartbeat_secs: proc.last_heartbeat.elapsed().as_secs(),
                restart_count: proc.restart_count,
            })
            .collect()
    }

    /// Get the restart event log.
    pub fn restart_log(&self) -> &[RestartEvent] {
        &self.restart_log
    }

    /// Run the watchdog event loop. Returns when the shutdown token is cancelled.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe at the loop boundary. `interval.tick()` is cancel-safe,
    /// and `CancellationToken::cancelled()` is cancel-safe.
    pub async fn run(&mut self) {
        tracing::info!(
            processes = self.processes.len(),
            heartbeat_timeout_secs = self.config.heartbeat_timeout.as_secs(),
            "watchdog started"
        );

        let mut interval = tokio::time::interval(self.config.check_interval);

        loop {
            tokio::select! {
                biased;
                () = self.shutdown.cancelled() => {
                    tracing::info!("watchdog shutting down");
                    break;
                }
                _ = interval.tick() => {
                    self.check_processes().await;
                }
            }
        }
    }

    /// Sweep all processes: detect hangs and trigger restarts.
    async fn check_processes(&mut self) {
        let now = Instant::now();
        let timeout = self.config.heartbeat_timeout;
        let max_restarts = self.config.max_restarts;
        let mut hung_count: i64 = 0;

        let ids: Vec<String> = self.processes.keys().cloned().collect();

        for id in ids {
            let Some(proc) = self.processes.get(&id) else {
                continue;
            };

            if proc.state == ProcessState::Abandoned {
                continue;
            }

            if let Some(backoff_until) = proc.backoff_until
                && now < backoff_until
            {
                continue;
            }

            let elapsed = proc.last_heartbeat.elapsed();

            if elapsed <= timeout && proc.state == ProcessState::Healthy {
                continue;
            }

            if elapsed > timeout && proc.state == ProcessState::Healthy {
                tracing::warn!(
                    process_id = %id,
                    elapsed_secs = elapsed.as_secs(),
                    timeout_secs = timeout.as_secs(),
                    "watchdog: hung process detected — no heartbeat"
                );
                if let Some(proc) = self.processes.get_mut(&id) {
                    proc.state = ProcessState::Hung;
                }
                hung_count += 1;
            }

            let Some(proc) = self.processes.get(&id) else {
                continue;
            };

            if proc.state != ProcessState::Hung {
                continue;
            }

            if proc.restart_count >= max_restarts {
                tracing::error!(
                    process_id = %id,
                    restart_count = proc.restart_count,
                    max_restarts = max_restarts,
                    "watchdog: max restarts exceeded — abandoning process"
                );
                if let Some(proc) = self.processes.get_mut(&id) {
                    proc.state = ProcessState::Abandoned;
                }
                continue;
            }

            self.restart_process(&id, elapsed).await;
        }

        crate::metrics::set_hung_processes(hung_count);
    }

    /// Kill and restart a hung process.
    async fn restart_process(&mut self, id: &str, elapsed: Duration) {
        let Some(proc) = self.processes.get_mut(id) else {
            return;
        };

        proc.state = ProcessState::Restarting;
        proc.restart_count += 1;
        let attempt = proc.restart_count;
        let handle = std::sync::Arc::clone(&proc.handle);

        let cause = RestartCause::HeartbeatTimeout {
            elapsed_secs: elapsed.as_secs(),
        };

        tracing::warn!(
            process_id = %id,
            cause = %cause,
            attempt = attempt,
            "watchdog: restarting process"
        );

        if let Err(e) = handle.kill().await {
            tracing::warn!(
                process_id = %id,
                error = %e,
                "watchdog: kill failed (process may already be dead)"
            );
        }

        match handle.restart().await {
            Ok(()) => {
                tracing::info!(
                    process_id = %id,
                    attempt = attempt,
                    "watchdog: process restarted successfully"
                );
                if let Some(proc) = self.processes.get_mut(id) {
                    proc.last_heartbeat = Instant::now();
                    proc.state = ProcessState::Healthy;
                    proc.backoff_until = None;
                }
            }
            Err(e) => {
                tracing::error!(
                    process_id = %id,
                    attempt = attempt,
                    error = %e,
                    "watchdog: restart failed — applying backoff"
                );
                if let Some(proc) = self.processes.get_mut(id) {
                    proc.state = ProcessState::Hung;
                    proc.backoff_until = Some(Instant::now() + watchdog_backoff(attempt));
                }
            }
        }

        crate::metrics::record_watchdog_restart(id);

        self.restart_log.push(RestartEvent {
            process_id: id.to_owned(),
            cause,
            attempt,
            timestamp: jiff::Timestamp::now().to_string(),
        });
    }
}

/// Compute exponential backoff delay for watchdog restarts.
///
/// Formula: `min(base * 2^(attempt-1), cap)`
/// - attempt 1: 2s
/// - attempt 2: 4s
/// - attempt 3: 8s
/// - attempt 4: 16s
/// - attempt 5+: capped at 300s (5 min)
pub fn watchdog_backoff(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1);
    let multiplier = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
    let delay = BACKOFF_BASE.saturating_mul(u32::try_from(multiplier).unwrap_or(u32::MAX));
    std::cmp::min(delay, BACKOFF_CAP)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
#[expect(
    clippy::unchecked_time_subtraction,
    reason = "test: subtracting small millis from now cannot underflow"
)]
mod tests {
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
        assert_eq!(config.heartbeat_timeout, Duration::from_secs(60));
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
        };
        let mut wd = Watchdog::new(config, token);
        let proc = Arc::new(MockProcess::new("agent-1"));
        wd.register(proc.clone());

        // Set up: hung with backoff in the future.
        wd.processes.get_mut("agent-1").unwrap().state = ProcessState::Hung;
        wd.processes.get_mut("agent-1").unwrap().backoff_until =
            Some(Instant::now() + Duration::from_secs(300));

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

    #[tokio::test]
    async fn shutdown_exits_watchdog_loop() {
        let token = CancellationToken::new();
        let mut wd = Watchdog::new(WatchdogConfig::default(), token.clone());

        let handle = tokio::spawn(
            async move {
                wd.run().await;
            }
            .instrument(tracing::info_span!("test_watchdog")),
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
}
