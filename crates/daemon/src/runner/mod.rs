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
use crate::schedule::{Schedule, TaskDef, TaskStatus};
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

    /// Get status of all registered tasks.
    ///
    /// # Complexity
    ///
    /// O(t) where t is the number of registered tasks.
    pub fn status(&self) -> Vec<TaskStatus> {
        self.tasks
            .iter()
            .map(|t| TaskStatus {
                id: t.def.id.clone(),
                name: t.def.name.clone(),
                enabled: t.def.enabled,
                next_run: t.next_run.map(|ts| ts.to_string()),
                last_run: t.last_run.map(|ts| ts.to_string()),
                run_count: t.run_count,
                consecutive_failures: t.consecutive_failures,
                in_flight: self.in_flight.contains_key(&t.def.id),
                last_error: t.last_error.clone(),
            })
            .collect()
    }

    /// Check in-flight tasks for completion, timeout warnings, and hung task cancellation.
    ///
    /// # Complexity
    ///
    /// O(i) where i is the number of in-flight tasks.
    async fn check_in_flight(&mut self) {
        let task_ids: Vec<String> = self.in_flight.keys().cloned().collect();

        for task_id in task_ids {
            let Some(in_flight) = self.in_flight.get_mut(&task_id) else {
                continue;
            };

            let elapsed = in_flight.started_at.elapsed();

            if elapsed > in_flight.timeout * 2 {
                tracing::warn!(
                    task_id = %task_id,
                    elapsed_secs = elapsed.as_secs(),
                    timeout_secs = in_flight.timeout.as_secs(),
                    "hung task detected  -  cancelling (exceeded 2x timeout)"
                );
                in_flight.handle.abort();

                self.in_flight.remove(&task_id);
                self.record_task_failure(&task_id, "cancelled: exceeded 2x timeout");
                continue;
            }

            if elapsed > in_flight.timeout && !in_flight.warned {
                tracing::warn!(
                    task_id = %task_id,
                    elapsed_secs = elapsed.as_secs(),
                    timeout_secs = in_flight.timeout.as_secs(),
                    "task running longer than configured timeout"
                );
                in_flight.warned = true;
            }

            if in_flight.handle.is_finished() {
                let Some(in_flight) = self.in_flight.remove(&task_id) else {
                    continue;
                };
                let duration = in_flight.started_at.elapsed();

                match in_flight.handle.await {
                    Ok(Ok(result)) => {
                        self.log_result(&task_id, &result);
                        self.maybe_queue_self_prompt(&task_id, &result);
                        self.record_task_completion(&task_id, duration);
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(
                            task_id = %task_id,
                            error = %e,
                            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
                            "spawned task failed"
                        );
                        self.record_task_failure(&task_id, &e.to_string());
                    }
                    Err(e) => {
                        tracing::warn!(
                            task_id = %task_id,
                            error = %e,
                            "spawned task panicked or was cancelled"
                        );
                        self.record_task_failure(&task_id, &e.to_string());
                    }
                }
            }
        }
    }

    /// Log task result, applying brief-mode truncation if configured.
    fn log_result(&self, task_id: &str, result: &ExecutionResult) {
        let Some(output) = result.output.as_deref() else {
            return;
        };

        match self.output_mode {
            DaemonOutputMode::Full => {
                tracing::debug!(task_id = %task_id, output = %output, "task output");
            }
            DaemonOutputMode::Brief => {
                let truncated = truncate_output(output);
                tracing::info!(task_id = %task_id, output = %truncated, "task output (brief)");
            }
        }
    }

    /// Check if a completed task's output contains a `## Follow-up` section
    /// and, if self-prompting is enabled and rate-allowed, spawn a self-prompt.
    ///
    /// WHY: self-prompting closes the feedback loop. A prosoche check that finds
    /// something wrong can request a follow-up action without human intervention.
    /// Rate limiting ensures this never runs away.
    fn maybe_queue_self_prompt(&mut self, task_id: &str, result: &ExecutionResult) {
        if !self.self_prompt_config.enabled {
            return;
        }

        let Some(output) = result.output.as_deref() else {
            return;
        };

        let Some(follow_up) = crate::self_prompt::extract_follow_up(output) else {
            return;
        };

        if !self.self_prompt_limiter.is_allowed(&self.nous_id) {
            tracing::info!(
                nous_id = %self.nous_id,
                task_id = %task_id,
                "self-prompt rate limited  -  skipping follow-up"
            );
            return;
        }

        self.self_prompt_limiter.record(&self.nous_id);

        let bridge = self.bridge.clone();
        let nous_id = self.nous_id.clone();
        let task_id_owned = task_id.to_owned();

        // WHY: spawn as a detached task. Self-prompt execution should not block
        // the main scheduler loop. Failures are logged but do not affect the
        // originating task's status.
        let task_name = "self_prompt";
        tokio::spawn(
            async move {
                tracing::info!(
                    nous_id = %nous_id,
                    source_task = %task_id_owned,
                    prompt_len = follow_up.len(),
                    "dispatching self-prompt from follow-up"
                );
                let result = crate::self_prompt::execute_self_prompt(
                    &nous_id,
                    &follow_up,
                    bridge.as_deref(),
                )
                .await;
                match result {
                    Ok(r) if r.success => {
                        tracing::info!(
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            "self-prompt dispatched successfully"
                        );
                    }
                    Ok(r) => {
                        tracing::warn!(
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            output = ?r.output,
                            "self-prompt dispatch returned failure"
                        );
                        crate::metrics::record_background_failure(&nous_id, task_name);
                    }
                    Err(e) => {
                        tracing::error!(
                            task = task_name,
                            nous_id = %nous_id,
                            source_task = %task_id_owned,
                            error = %e,
                            "background task failed"
                        );
                        crate::metrics::record_background_failure(&nous_id, task_name);
                    }
                }
            }
            .instrument(tracing::info_span!("background_task", task = task_name)),
        );
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
