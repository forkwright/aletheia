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
use crate::schedule::{BuiltinTask, Schedule, TaskAction, TaskDef, TaskStatus, backoff_delay};

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
    /// If set, the task is in backoff and should not run before this instant.
    backoff_until: Option<Instant>,
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
        }
    }

    /// Attach maintenance configuration.
    #[must_use]
    pub fn with_maintenance(mut self, config: MaintenanceConfig) -> Self {
        self.maintenance = Some(config);
        self
    }

    /// Attach a retention executor for data cleanup.
    #[must_use]
    pub fn with_retention(mut self, executor: Arc<dyn RetentionExecutor>) -> Self {
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
    pub fn with_state_store(mut self, store: crate::state::TaskStateStore) -> Self {
        self.state_store = Some(store);
        self
    }

    /// Register default maintenance tasks based on configuration.
    ///
    /// Skips disabled tasks and retention when no executor is provided.
    pub fn register_maintenance_tasks(&mut self) {
        let Some(config) = self.maintenance.clone() else {
            return;
        };
        let has_executor = self.retention_executor.is_some();

        if config.trace_rotation.enabled {
            self.register(TaskDef {
                id: "trace-rotation".to_owned(),
                name: "Trace rotation".to_owned(),
                nous_id: self.nous_id.clone(),
                schedule: Schedule::Cron("0 0 3 * * *".to_owned()),
                action: TaskAction::Builtin(BuiltinTask::TraceRotation),
                enabled: true,
                catch_up: true,
                ..TaskDef::default()
            });
        }

        if config.drift_detection.enabled {
            self.register(TaskDef {
                id: "drift-detection".to_owned(),
                name: "Instance drift detection".to_owned(),
                nous_id: self.nous_id.clone(),
                schedule: Schedule::Cron("0 0 4 * * *".to_owned()),
                action: TaskAction::Builtin(BuiltinTask::DriftDetection),
                enabled: true,
                catch_up: true,
                ..TaskDef::default()
            });
        }

        if config.db_monitoring.enabled {
            self.register(TaskDef {
                id: "db-monitor".to_owned(),
                name: "Database size monitor".to_owned(),
                nous_id: self.nous_id.clone(),
                schedule: Schedule::Interval(Duration::from_secs(6 * 3600)),
                action: TaskAction::Builtin(BuiltinTask::DbSizeMonitor),
                enabled: true,
                catch_up: true,
                ..TaskDef::default()
            });
        }

        if config.retention.enabled && has_executor {
            self.register(TaskDef {
                id: "retention-execution".to_owned(),
                name: "Data retention cleanup".to_owned(),
                nous_id: self.nous_id.clone(),
                schedule: Schedule::Cron("0 30 3 * * *".to_owned()),
                action: TaskAction::Builtin(BuiltinTask::RetentionExecution),
                enabled: true,
                catch_up: true,
                ..TaskDef::default()
            });
        }

        if config.knowledge_maintenance.enabled && self.knowledge_executor.is_some() {
            self.register_knowledge_maintenance_tasks();
        }
    }

    /// Register the 7 knowledge maintenance tasks with their schedules.
    fn register_knowledge_maintenance_tasks(&mut self) {
        let tasks = [
            (
                "decay-refresh",
                "Decay score refresh",
                Schedule::Interval(Duration::from_secs(4 * 3600)),
                BuiltinTask::DecayRefresh,
            ),
            (
                "entity-dedup",
                "Entity deduplication",
                Schedule::Interval(Duration::from_secs(6 * 3600)),
                BuiltinTask::EntityDedup,
            ),
            (
                "graph-recompute",
                "Graph score recomputation",
                Schedule::Interval(Duration::from_secs(8 * 3600)),
                BuiltinTask::GraphRecompute,
            ),
            (
                "embedding-refresh",
                "Embedding refresh",
                Schedule::Interval(Duration::from_secs(12 * 3600)),
                BuiltinTask::EmbeddingRefresh,
            ),
            (
                "knowledge-gc",
                "Knowledge garbage collection",
                Schedule::Cron("0 0 4 * * *".to_owned()),
                BuiltinTask::KnowledgeGc,
            ),
            (
                "index-maintenance",
                "Index maintenance",
                Schedule::Cron("0 30 4 * * *".to_owned()),
                BuiltinTask::IndexMaintenance,
            ),
            (
                "graph-health-check",
                "Graph health check",
                Schedule::Cron("0 0 5 * * *".to_owned()),
                BuiltinTask::GraphHealthCheck,
            ),
            (
                "skill-decay",
                "Skill decay and retirement",
                Schedule::Cron("0 0 6 * * *".to_owned()),
                BuiltinTask::SkillDecay,
            ),
        ];

        for (id, name, schedule, task) in tasks {
            self.register(TaskDef {
                id: id.to_owned(),
                name: name.to_owned(),
                nous_id: self.nous_id.clone(),
                schedule,
                action: TaskAction::Builtin(task),
                enabled: true,
                catch_up: true,
                ..TaskDef::default()
            });
        }
    }

    /// Register a task. Startup tasks are marked for immediate execution.
    pub fn register(&mut self, task: TaskDef) {
        let next_run = match &task.schedule {
            Schedule::Startup => Some(jiff::Timestamp::now()),
            other => other.next_run().unwrap_or(None),
        };

        tracing::info!(
            nous_id = %self.nous_id,
            task_id = %task.id,
            task_name = %task.name,
            "registered task"
        );

        self.tasks.push(RegisteredTask {
            def: task,
            next_run,
            last_run: None,
            run_count: 0,
            consecutive_failures: 0,
            backoff_until: None,
        });
    }

    /// Check each cron task for missed windows and run catch-up if needed.
    ///
    /// Called once at startup. For each task with `catch_up: true` and a cron
    /// schedule, checks if a window was missed within the last 24 hours.
    /// If so, schedules the task for immediate execution.
    pub fn check_missed_cron_catchup(&mut self) {
        for task in &mut self.tasks {
            if !task.def.enabled || !task.def.catch_up {
                continue;
            }

            let Some(last_run) = task.last_run else {
                continue;
            };

            match task.def.schedule.missed_since(last_run) {
                Ok(true) => {
                    tracing::info!(
                        task_id = %task.def.id,
                        task_name = %task.def.name,
                        last_run = %last_run,
                        "missed cron window detected — scheduling catch-up"
                    );
                    task.next_run = Some(jiff::Timestamp::now());
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(
                        task_id = %task.def.id,
                        error = %e,
                        "failed to check missed cron windows"
                    );
                }
            }
        }
    }

    /// Set the `last_run` timestamp for a task by ID (for catch-up testing/persistence).
    pub fn set_last_run(&mut self, task_id: &str, last_run: jiff::Timestamp) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) {
            task.last_run = Some(last_run);
        }
    }

    /// Run the event loop. Checks for due tasks every second, executes them.
    /// Returns when the shutdown token is cancelled.
    pub async fn run(&mut self) {
        tracing::info!(nous_id = %self.nous_id, tasks = self.tasks.len(), "daemon started");

        // Restore persisted state before checking for missed windows.
        self.restore_state();

        // Check for missed cron windows on startup.
        self.check_missed_cron_catchup();

        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.check_in_flight().await;
                    self.tick();
                }
                () = self.shutdown.cancelled() => {
                    tracing::info!(nous_id = %self.nous_id, "daemon shutting down");
                    break;
                }
            }
        }
    }

    /// Get status of all registered tasks.
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
            })
            .collect()
    }

    /// Check in-flight tasks for completion, timeout warnings, and hung task cancellation.
    #[expect(
        clippy::expect_used,
        reason = "key existence verified by is_finished() check immediately before"
    )]
    async fn check_in_flight(&mut self) {
        let task_ids: Vec<String> = self.in_flight.keys().cloned().collect();

        for task_id in task_ids {
            let Some(in_flight) = self.in_flight.get_mut(&task_id) else {
                continue;
            };

            let elapsed = in_flight.started_at.elapsed();

            // Check for 2x timeout — cancel the task.
            if elapsed > in_flight.timeout * 2 {
                tracing::warn!(
                    task_id = %task_id,
                    elapsed_secs = elapsed.as_secs(),
                    timeout_secs = in_flight.timeout.as_secs(),
                    "hung task detected — cancelling (exceeded 2x timeout)"
                );
                in_flight.handle.abort();

                self.in_flight.remove(&task_id);
                self.record_task_failure(&task_id, "cancelled: exceeded 2x timeout");
                continue;
            }

            // Check for 1x timeout — warn.
            if elapsed > in_flight.timeout && !in_flight.warned {
                tracing::warn!(
                    task_id = %task_id,
                    elapsed_secs = elapsed.as_secs(),
                    timeout_secs = in_flight.timeout.as_secs(),
                    "task running longer than configured timeout"
                );
                in_flight.warned = true;
            }

            // Check if the task completed.
            if in_flight.handle.is_finished() {
                let in_flight = self.in_flight.remove(&task_id).expect("just checked");
                let duration = in_flight.started_at.elapsed();

                match in_flight.handle.await {
                    Ok(Ok(_result)) => {
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

    /// Record a successful task completion and update scheduling.
    fn record_task_completion(&mut self, task_id: &str, duration: Duration) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) else {
            return;
        };

        task.last_run = Some(jiff::Timestamp::now());
        task.run_count += 1;
        task.consecutive_failures = 0;
        task.backoff_until = None;
        task.next_run = task.def.schedule.next_run().unwrap_or(None);

        tracing::info!(
            task_id = %task.def.id,
            task_name = %task.def.name,
            run_count = task.run_count,
            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            result = "success",
            "task completed"
        );

        let state_to_save = crate::state::TaskState {
            task_id: task.def.id.clone(),
            last_run_ts: task.last_run.map(|ts| ts.to_string()),
            run_count: task.run_count,
            consecutive_failures: task.consecutive_failures,
        };
        self.persist_task_state(&state_to_save);
    }

    /// Record a task failure: increment failures, apply backoff, possibly auto-disable.
    #[expect(
        clippy::expect_used,
        reason = "arithmetic on small bounded values (delay nanos < i64::MAX, timestamp addition within valid jiff range)"
    )]
    fn record_task_failure(&mut self, task_id: &str, reason: &str) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) else {
            return;
        };

        task.consecutive_failures += 1;
        task.last_run = Some(jiff::Timestamp::now());

        // GraphHealthCheck failures don't count toward auto-disable.
        let exempt = matches!(
            task.def.action,
            TaskAction::Builtin(BuiltinTask::GraphHealthCheck)
        );

        if !exempt && task.consecutive_failures >= 3 {
            task.def.enabled = false;
            tracing::warn!(
                task_id = %task.def.id,
                task_name = %task.def.name,
                failures = task.consecutive_failures,
                last_error = %reason,
                "task auto-disabled after 3 consecutive failures"
            );
        } else {
            // Apply exponential backoff.
            let delay = backoff_delay(task.consecutive_failures);
            task.backoff_until = Some(Instant::now() + delay);

            // Next run is the later of the schedule's next_run and the backoff.
            let scheduled_next = task.def.schedule.next_run().unwrap_or(None);
            let backoff_ts = jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_nanos(
                    i64::try_from(delay.as_nanos()).unwrap_or(i64::MAX),
                ))
                .expect("backoff addition overflow");

            task.next_run = match scheduled_next {
                Some(next) if next > backoff_ts => Some(next),
                _ => Some(backoff_ts),
            };

            tracing::warn!(
                task_id = %task.def.id,
                task_name = %task.def.name,
                failures = task.consecutive_failures,
                backoff_secs = delay.as_secs(),
                error = %reason,
                result = "failure",
                "task failed — backoff applied"
            );
        }

        let state_to_save = crate::state::TaskState {
            task_id: task.def.id.clone(),
            last_run_ts: task.last_run.map(|ts| ts.to_string()),
            run_count: task.run_count,
            consecutive_failures: task.consecutive_failures,
        };
        self.persist_task_state(&state_to_save);
    }

    /// Restore persisted task state from the `SQLite` store (if attached).
    ///
    /// Called once at startup, before catch-up checking. Skips silently when
    /// no store is configured or when a task ID in the store no longer exists.
    fn restore_state(&mut self) {
        let Some(ref store) = self.state_store else {
            return;
        };
        match store.load_all() {
            Ok(states) => {
                for saved in states {
                    let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == saved.task_id)
                    else {
                        continue;
                    };
                    if let Some(Ok(ts)) = saved
                        .last_run_ts
                        .as_deref()
                        .map(str::parse::<jiff::Timestamp>)
                    {
                        task.last_run = Some(ts);
                    }
                    task.run_count = saved.run_count;
                    task.consecutive_failures = saved.consecutive_failures;
                }
                tracing::info!(nous_id = %self.nous_id, "task state restored from SQLite");
            }
            Err(e) => {
                tracing::warn!(
                    nous_id = %self.nous_id,
                    error = %e,
                    "failed to restore task state — starting fresh"
                );
            }
        }
    }

    /// Persist a single task's state to the `SQLite` store, if one is attached.
    fn persist_task_state(&self, state: &crate::state::TaskState) {
        let Some(ref store) = self.state_store else {
            return;
        };
        if let Err(e) = store.save(state) {
            tracing::warn!(
                task_id = %state.task_id,
                error = %e,
                "failed to persist task state"
            );
        }
    }

    fn tick(&mut self) {
        let now = jiff::Timestamp::now();
        let now_instant = Instant::now();

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

            // Backpressure: skip if previous execution is still in progress.
            if self.in_flight.contains_key(&self.tasks[i].def.id) {
                tracing::debug!(
                    task_id = %self.tasks[i].def.id,
                    "skipping — previous execution still in progress"
                );
                continue;
            }

            // Check backoff.
            if let Some(backoff_until) = self.tasks[i].backoff_until
                && now_instant < backoff_until
            {
                tracing::debug!(
                    task_id = %self.tasks[i].def.id,
                    remaining_secs = (backoff_until - now_instant).as_secs(),
                    "skipping — in backoff period"
                );
                continue;
            }

            let action = self.tasks[i].def.action.clone();
            let nous_id = self.tasks[i].def.nous_id.clone();
            let task_id = self.tasks[i].def.id.clone();
            let timeout = self.tasks[i].def.timeout;

            // Clone Arc handles for the spawned task.
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

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
