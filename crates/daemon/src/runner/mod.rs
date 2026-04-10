//! Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::bridge::DaemonBridge;
use crate::error::Result;
use crate::execution::execute_action;
use crate::maintenance::{KnowledgeMaintenanceExecutor, MaintenanceConfig, RetentionExecutor};
use crate::schedule::{Schedule, TaskDef};
// WHY: tests use `use super::*` and reference BuiltinTask/TaskAction directly.
#[cfg(test)]
use crate::schedule::{BuiltinTask, TaskAction};

/// Output mode for daemon logging.
///
/// WHY: daemon logs should be scannable; full model responses and tool results
/// flood the log when running in production.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DaemonOutputMode {
    /// Full output  -  all tool results and model responses logged verbatim.
    #[default]
    Full,
    /// Brief output  -  tool results truncated to first/last N lines, model
    /// responses logged at info level with truncation.
    Brief,
}

mod inflight;
mod output;
mod persistence;
mod registration;
mod systemd;
mod tracking;
pub(crate) use output::truncate_output;

/// Per-nous background task runner.
pub struct TaskRunner {
    nous_id: String,
    tasks: Vec<RegisteredTask>,
    shutdown: CancellationToken,
    bridge: Option<Arc<dyn DaemonBridge>>,
    maintenance: Option<MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
    /// In-flight tasks: `task_id` → [`InFlightTask`].
    in_flight: HashMap<String, InFlightTask>,
    /// Optional SQLite-backed state store for cross-restart persistence.
    state_store: Option<crate::state::TaskStateStore>,
    /// Output mode: full or brief (truncated).
    output_mode: DaemonOutputMode,
    /// Self-prompt rate limiter (tracks per-agent dispatch counts).
    self_prompt_limiter: crate::self_prompt::SelfPromptLimiter,
    /// Self-prompt configuration (enabled, rate limits).
    self_prompt_config: crate::self_prompt::SelfPromptConfig,
}

/// Tracks a task that is currently executing.
struct InFlightTask {
    handle: tokio::task::JoinHandle<Result<ExecutionResult>>,
    started_at: Instant,
    timeout: Duration,
    warned: bool,
}

struct RegisteredTask {
    def: TaskDef,
    next_run: Option<jiff::Timestamp>,
    last_run: Option<jiff::Timestamp>,
    run_count: u64,
    consecutive_failures: u32,
    /// If SET, the task is in backoff and should not run before this instant.
    backoff_until: Option<Instant>,
    /// Most recent error message, if the last execution failed. (#2212)
    last_error: Option<String>,
}

/// Outcome of executing a single task action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Whether the task completed without error.
    pub success: bool,
    /// Task output or diagnostic message.
    pub output: Option<String>,
}

impl TaskRunner {
    /// Create a runner for the given nous, listening for shutdown on the cancellation token.
    pub fn new(nous_id: impl Into<String>, shutdown: CancellationToken) -> Self {
        Self {
            nous_id: nous_id.into(),
            tasks: Vec::new(),
            shutdown,
            bridge: None,
            maintenance: None,
            retention_executor: None,
            knowledge_executor: None,
            in_flight: HashMap::new(),
            state_store: None,
            output_mode: DaemonOutputMode::Full,
            self_prompt_limiter: crate::self_prompt::SelfPromptLimiter::new(1),
            self_prompt_config: crate::self_prompt::SelfPromptConfig::default(),
        }
    }

    /// Create a runner with a bridge for nous communication.
    pub fn with_bridge(
        nous_id: impl Into<String>,
        shutdown: CancellationToken,
        bridge: Arc<dyn DaemonBridge>,
    ) -> Self {
        Self {
            nous_id: nous_id.into(),
            tasks: Vec::new(),
            shutdown,
            bridge: Some(bridge),
            maintenance: None,
            retention_executor: None,
            knowledge_executor: None,
            in_flight: HashMap::new(),
            state_store: None,
            output_mode: DaemonOutputMode::Full,
            self_prompt_limiter: crate::self_prompt::SelfPromptLimiter::new(1),
            self_prompt_config: crate::self_prompt::SelfPromptConfig::default(),
        }
    }

    /// Attach maintenance configuration.
    #[must_use]
    pub fn with_maintenance(mut self, config: MaintenanceConfig) -> Self {
        self.maintenance = Some(config);
        self
    }

    /// Attach a retention executor for data lifecycle management.
    #[must_use]
    pub fn with_retention(
        mut self,
        executor: Arc<dyn crate::maintenance::RetentionExecutor>,
    ) -> Self {
        self.retention_executor = Some(executor);
        self
    }

    /// Attach a knowledge maintenance executor for graph operations.
    #[must_use]
    pub fn with_knowledge_maintenance(
        mut self,
        executor: Arc<dyn KnowledgeMaintenanceExecutor>,
    ) -> Self {
        self.knowledge_executor = Some(executor);
        self
    }

    /// Attach a `SQLite` state store for task execution persistence.
    ///
    /// State is loaded on the first call to [`Self::run`] (before catch-up),
    /// and saved after every task completion or failure.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "called by runner_tests::with_state_store_persists_across_restarts; production wiring lives in the binary crate"
        )
    )]
    pub(crate) fn with_state_store(mut self, store: crate::state::TaskStateStore) -> Self {
        self.state_store = Some(store);
        self
    }

    /// Set the output mode (full or brief).
    #[must_use]
    pub fn with_output_mode(mut self, mode: DaemonOutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    /// Configure self-prompting behavior (rate-limited daemon-initiated follow-ups).
    ///
    /// WHY: self-prompting enables proactive work when prosoche checks identify
    /// items needing attention. Must be explicitly enabled with rate limits.
    #[must_use]
    pub fn with_self_prompt(mut self, config: crate::self_prompt::SelfPromptConfig) -> Self {
        self.self_prompt_limiter = crate::self_prompt::SelfPromptLimiter::new(config.max_per_hour);
        self.self_prompt_config = config;
        self
    }

    /// Run the event loop. Checks for due tasks every second, executes them.
    /// Returns when the shutdown token is cancelled.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe at the loop boundary. Each `select!` branch is cancel-safe:
    /// `interval.tick()` is cancel-safe (a dropped tick simply delays the next
    /// poll), and `CancellationToken::cancelled()` is cancel-safe. If this
    /// future is dropped between iterations, in-flight tasks continue running
    /// on the Tokio executor; their `JoinHandle`s are held in `self.in_flight`
    /// and will be abandoned (not awaited) on DROP.
    #[tracing::instrument(skip_all)]
    pub async fn run(&mut self) {
        tracing::info!(nous_id = %self.nous_id, tasks = self.tasks.len(), "daemon started");

        self.restore_state();

        self.check_missed_cron_catchup();

        // WHY: send READY=1 to systemd after initialization is complete.
        sd_notify_ready();

        let mut interval = tokio::time::interval(Duration::from_secs(1));

        // WHY: watchdog interval for systemd WatchdogSec integration.
        let watchdog_interval = sd_watchdog_interval();
        let mut watchdog_tick =
            tokio::time::interval(watchdog_interval.unwrap_or(Duration::from_secs(30)));

        loop {
            tokio::select! {
                // SAFETY: cancel-safe. `interval.tick()` is cancel-safe; dropping it
                // before it fires simply delays the next tick without losing state.
                // `check_in_flight` polls already-spawned handles and does not
                // mutate scheduler state if cancelled mid-loop.
                _ = interval.tick() => {
                    self.check_in_flight().await;
                    self.tick();
                }
                // WHY: send WATCHDOG=1 on the systemd watchdog interval so
                // WatchdogSec integration enables automatic restart on hang.
                _ = watchdog_tick.tick(), if watchdog_interval.is_some() => {
                    sd_notify_watchdog();
                }
                // SAFETY: cancel-safe. `CancellationToken::cancelled()` is cancel-safe;
                // dropping the future before it fires has no side effects.
                () = self.shutdown.cancelled() => {
                    tracing::info!(nous_id = %self.nous_id, "daemon shutting down");
                    break;
                }
            }
        }

        // WHY: send STOPPING=1 to systemd before cleanup.
        sd_notify_stopping();

        // WHY: abort all in-flight tasks on shutdown to prevent leaked work
        // after the runner exits. Without this, spawned tasks continue running
        // on the Tokio executor with no observer to collect their results.
        let in_flight_count = self.in_flight.len();
        for (task_id, in_flight) in self.in_flight.drain() {
            tracing::debug!(task_id = %task_id, "aborting in-flight task on shutdown");
            in_flight.handle.abort();
        }
        if in_flight_count > 0 {
            tracing::info!(
                nous_id = %self.nous_id,
                cancelled = in_flight_count,
                "in-flight tasks cancelled on shutdown"
            );
        }
    }

    fn tick(&mut self) {
        let now = jiff::Timestamp::now();
        let now_instant = Instant::now();

        // i is always in 0..self.tasks.len() so all self.tasks[i] accesses are valid
        #[expect(
            clippy::indexing_slicing,
            reason = "i ∈ 0..self.tasks.len() by for-loop bounds"
        )]
        for i in 0..self.tasks.len() {
            if !self.tasks[i].def.enabled {
                continue;
            }

            let Some(next) = self.tasks[i].next_run else {
                continue;
            };

            if next > now {
                continue;
            }

            if !Schedule::in_window(self.tasks[i].def.active_window) {
                continue;
            }

            // WHY: skip tasks still in-flight to prevent overlapping executions.
            if self.in_flight.contains_key(&self.tasks[i].def.id) {
                tracing::debug!(
                    task_id = %self.tasks[i].def.id,
                    "skipping  -  previous execution still in progress"
                );
                continue;
            }

            if let Some(backoff_until) = self.tasks[i].backoff_until
                && now_instant < backoff_until
            {
                tracing::debug!(
                    task_id = %self.tasks[i].def.id,
                    remaining_secs = (backoff_until - now_instant).as_secs(),
                    "skipping  -  in backoff period"
                );
                continue;
            }

            // WHY(#2212): record last_run BEFORE spawning the task so that a crash
            // during execution does not leave last_run stale, which would cause
            // the scheduler to re-execute the task immediately on recovery.
            self.tasks[i].last_run = Some(jiff::Timestamp::now());

            let action = self.tasks[i].def.action.clone();
            let nous_id = self.tasks[i].def.nous_id.clone();
            let task_id = self.tasks[i].def.id.clone();
            let timeout = self.tasks[i].def.timeout;

            let bridge = self.bridge.clone();
            let maintenance = self.maintenance.clone();
            let retention_executor = self.retention_executor.clone();
            let knowledge_executor = self.knowledge_executor.clone();

            let span = tracing::info_span!(
                "task_execute",
                task_id = %task_id,
                task_name = %self.tasks[i].def.name,
                nous_id = %nous_id,
            );

            let handle = tokio::spawn(
                async move {
                    execute_action(
                        &action,
                        &nous_id,
                        bridge.as_deref(),
                        maintenance.as_ref(),
                        retention_executor,
                        knowledge_executor,
                    )
                    .await
                }
                .instrument(span),
            );

            self.in_flight.insert(
                task_id,
                InFlightTask {
                    handle,
                    started_at: Instant::now(),
                    timeout,
                    warned: false,
                },
            );
        }
    }
}

use systemd::{sd_notify_ready, sd_notify_stopping, sd_notify_watchdog, sd_watchdog_interval};

#[cfg(test)]
#[path = "../runner_tests.rs"]
mod tests;
