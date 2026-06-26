//! Watchdog process monitor with heartbeat tracking and auto-recovery.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Default heartbeat timeout before a process is considered hung.
const DEFAULT_HEARTBEAT_TIMEOUT: Duration = Duration::from_mins(1);

/// Default interval between watchdog health check sweeps.
const DEFAULT_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Maximum number of restarts before the watchdog stops retrying.
const DEFAULT_MAX_RESTARTS: u32 = 5;

/// Base delay for exponential backoff (2 seconds).
/// Fallback default; runtime reads `DaemonBehaviorConfig::watchdog_backoff_base_secs`.
pub const BACKOFF_BASE: Duration = Duration::from_secs(2);

/// Maximum backoff delay cap (5 minutes).
/// Fallback default; runtime reads `DaemonBehaviorConfig::watchdog_backoff_cap_secs`.
pub const BACKOFF_CAP: Duration = Duration::from_mins(5);

/// Maximum number of restart events retained in memory.
///
/// WHY: a fixed cap prevents unbounded growth over daemon lifetime while
/// preserving enough diagnostic history for the current restart window.
const RESTART_LOG_CAP: usize = 100;

/// Watchdog configuration.
#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// Maximum time without a heartbeat before a process is declared hung.
    pub heartbeat_timeout: Duration,
    /// How often the watchdog sweeps for hung processes.
    pub check_interval: Duration,
    /// Maximum number of restart attempts before giving up.
    pub max_restarts: u32,
    /// Base duration for restart backoff.
    pub backoff_base: Duration,
    /// Maximum restart backoff duration.
    pub backoff_cap: Duration,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            heartbeat_timeout: DEFAULT_HEARTBEAT_TIMEOUT,
            check_interval: DEFAULT_CHECK_INTERVAL,
            max_restarts: DEFAULT_MAX_RESTARTS,
            backoff_base: BACKOFF_BASE,
            backoff_cap: BACKOFF_CAP,
        }
    }
}

impl WatchdogConfig {
    /// Build watchdog config from deployment maintenance settings.
    #[must_use]
    pub fn from_settings(settings: &taxis::config::WatchdogSettings) -> Self {
        Self {
            heartbeat_timeout: Duration::from_secs(settings.heartbeat_timeout_secs),
            check_interval: Duration::from_secs(settings.check_interval_secs.max(1)),
            max_restarts: settings.max_restarts,
            ..Self::default()
        }
    }

    /// Apply deployment-tunable backoff settings from taxis config.
    #[must_use]
    pub fn with_daemon_behavior(mut self, behavior: &taxis::config::DaemonBehaviorConfig) -> Self {
        self.backoff_base = Duration::from_secs(behavior.watchdog_backoff_base_secs);
        self.backoff_cap = Duration::from_secs(behavior.watchdog_backoff_cap_secs);
        self
    }
}

/// Lifecycle handle for a monitored process.
///
/// Implemented by the binary crate where concrete process types are available.
pub(crate) trait ProcessHandle: Send + Sync {
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
    // kanon:ignore RUST/primitive-for-domain-id — RestartEvent::process_id is a runtime process handle identifier string, not a typed domain entity ID
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
    // kanon:ignore RUST/primitive-for-domain-id — ProcessStatus::id is a runtime process handle identifier string, not a typed domain entity ID
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
pub(crate) struct Watchdog {
    processes: HashMap<String, WatchedProcess>,
    config: WatchdogConfig,
    shutdown: CancellationToken,
    restart_log: VecDeque<RestartEvent>,
}

impl Watchdog {
    /// Create a new watchdog with the given configuration.
    pub(crate) fn new(config: WatchdogConfig, shutdown: CancellationToken) -> Self {
        Self {
            processes: HashMap::new(),
            config,
            shutdown,
            restart_log: VecDeque::new(),
        }
    }

    /// Return the configured health-check sweep interval.
    pub(crate) fn check_interval(&self) -> Duration {
        self.config.check_interval
    }

    /// Register a process for monitoring.
    pub(crate) fn register(&mut self, handle: std::sync::Arc<dyn ProcessHandle>) {
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

    /// Remove a process from monitoring.
    pub(crate) fn unregister(&mut self, process_id: &str) {
        if self.processes.remove(process_id).is_some() {
            tracing::info!(process_id = %process_id, "watchdog: unregistered process");
        }
    }

    /// Record a heartbeat from a process.
    pub(crate) fn heartbeat(&mut self, process_id: &str) {
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

    /// Return a snapshot of all watched process statuses.
    pub(crate) fn status(&self) -> Vec<ProcessStatus> {
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

    /// Return the restart event log.
    pub(crate) fn restart_log(&self) -> &VecDeque<RestartEvent> {
        &self.restart_log
    }

    /// Report that a process exited unexpectedly.
    pub(crate) fn report_exit(&mut self, process_id: &str, reason: &str) {
        if let Some(proc) = self.processes.get_mut(process_id) {
            tracing::warn!(
                process_id = %process_id,
                reason = %reason,
                "watchdog: process reported exit"
            );
            proc.state = ProcessState::Hung;
        }
    }

    /// Run the watchdog event loop. Returns when the shutdown token is cancelled.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe at the loop boundary. `interval.tick()` is cancel-safe,
    /// and `CancellationToken::cancelled()` is cancel-safe.
    #[cfg(test)]
    #[tracing::instrument(skip_all)]
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

    /// Run one watchdog sweep.
    pub(crate) async fn sweep(&mut self) {
        if self.shutdown.is_cancelled() {
            return;
        }
        self.check_processes().await;
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
                    proc.backoff_until =
                        Some(Instant::now() + watchdog_backoff_with_config(attempt, &self.config));
                }
            }
        }

        crate::metrics::record_watchdog_restart(id);

        self.restart_log.push_back(RestartEvent {
            process_id: id.to_owned(),
            cause,
            attempt,
            timestamp: jiff::Timestamp::now().to_string(),
        });
        // WHY: evict oldest entries once the cap is exceeded.
        if self.restart_log.len() > RESTART_LOG_CAP {
            self.restart_log.pop_front();
        }
    }
}

/// Compute exponential backoff delay for watchdog restarts.
///
/// Delegates to [`koina::retry::BackoffStrategy::Exponential`] with
/// base=2s, factor=2, cap=300s.
///
/// - attempt 1: 2s
/// - attempt 2: 4s
/// - attempt 3: 8s
/// - attempt 4: 16s
/// - attempt 5+: capped at 300s (5 min)
#[cfg(test)]
pub(crate) fn watchdog_backoff(attempt: u32) -> Duration {
    watchdog_backoff_with_config(attempt, &WatchdogConfig::default())
}

fn watchdog_backoff_with_config(attempt: u32, config: &WatchdogConfig) -> Duration {
    use koina::retry::BackoffStrategy;
    let strategy = BackoffStrategy::Exponential {
        base: config.backoff_base,
        factor: 2,
        max_delay: config.backoff_cap,
    };
    // WHY: call site passes 1-indexed attempt; delay_for_attempt is 0-indexed
    strategy.delay_for_attempt(attempt.saturating_sub(1))
}

#[cfg(test)]
#[path = "watchdog_tests.rs"]
mod watchdog_tests;
