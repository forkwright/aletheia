//! Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::bridge::DaemonBridge;
use crate::error::{self, Result};
use crate::maintenance::{
    DbMonitor, DriftDetector, KnowledgeMaintenanceExecutor, MaintenanceConfig, RetentionExecutor,
    TraceRotator,
};
use crate::schedule::{BuiltinTask, Schedule, TaskAction, TaskDef, TaskStatus, backoff_delay};

/// Maximum wall-clock duration for any single task execution (10 minutes).
///
/// Prevents a hung task (e.g., a blocking shell command or an unresponsive
/// knowledge store operation) from blocking the runner indefinitely.
const TASK_EXECUTION_TIMEOUT: Duration = Duration::from_secs(600);

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
                id: "db-size-monitor".to_owned(),
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
    }

    /// Record a task failure: increment failures, apply backoff, possibly auto-disable.
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
            if let Some(backoff_until) = self.tasks[i].backoff_until {
                if now_instant < backoff_until {
                    tracing::debug!(
                        task_id = %self.tasks[i].def.id,
                        remaining_secs = (backoff_until - now_instant).as_secs(),
                        "skipping — in backoff period"
                    );
                    continue;
                }
            }

            let action = self.tasks[i].def.action.clone();
            let nous_id = self.tasks[i].def.nous_id.clone();
            let task_id = self.tasks[i].def.id.clone();
            let task_name = self.tasks[i].def.name.clone();
            let timeout = self.tasks[i].def.timeout;

            // Clone Arc handles for the spawned task.
            let bridge = self.bridge.clone();
            let maintenance = self.maintenance.clone();
            let retention_executor = self.retention_executor.clone();
            let knowledge_executor = self.knowledge_executor.clone();

            let span = tracing::info_span!(
                "task_execute",
                task_id = %task_id,
                task_name = %task_name,
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

/// Execute a task action. Receives owned `Arc`s for executor references
/// so it can be spawned as a `'static` future.
async fn execute_action(
    action: &TaskAction,
    nous_id: &str,
    bridge: Option<&dyn DaemonBridge>,
    maintenance: Option<&MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    match action {
        TaskAction::Command(cmd) => execute_command(cmd).await,
        TaskAction::Tool { name, .. } => {
            tracing::info!(
                nous_id = %nous_id,
                tool = %name,
                "tool execution not yet wired — requires organon integration"
            );
            Ok(ExecutionResult {
                success: true,
                output: None,
            })
        }
        TaskAction::Prompt(prompt) => {
            if let Some(bridge) = bridge {
                bridge.send_prompt(nous_id, "daemon:prompt", prompt).await
            } else {
                tracing::warn!(
                    nous_id = %nous_id,
                    "prompt action skipped — no daemon bridge configured"
                );
                Ok(ExecutionResult {
                    success: false,
                    output: Some("no bridge configured".to_owned()),
                })
            }
        }
        TaskAction::Builtin(builtin) => {
            execute_builtin(
                builtin,
                nous_id,
                bridge,
                maintenance,
                retention_executor,
                knowledge_executor,
            )
            .await
        }
    }
}

async fn execute_command(cmd: &str) -> Result<ExecutionResult> {
    let output = tokio::process::Command::new("sh")
        .args(["-c", cmd])
        .output()
        .await
        .context(error::CommandFailedSnafu {
            command: cmd.to_owned(),
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        tracing::debug!(cmd = %cmd, stdout = %stdout, "command succeeded");
        Ok(ExecutionResult {
            success: true,
            output: Some(stdout.into_owned()),
        })
    } else {
        let reason = if stderr.is_empty() {
            format!("exit code: {}", output.status)
        } else {
            stderr.into_owned()
        };

        error::TaskFailedSnafu {
            task_id: cmd.to_owned(),
            reason,
        }
        .fail()
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "match dispatch over builtin variants"
)]
async fn execute_builtin(
    builtin: &BuiltinTask,
    nous_id: &str,
    bridge: Option<&dyn DaemonBridge>,
    maintenance: Option<&MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    match builtin {
        BuiltinTask::Prosoche => {
            if let Some(bridge) = bridge {
                let prompt = "Run your prosoche heartbeat check per PROSOCHE.md.";
                let _ = bridge.send_prompt(nous_id, "daemon:prosoche", prompt).await;
                Ok(ExecutionResult {
                    success: true,
                    output: Some("dispatched".to_owned()),
                })
            } else {
                Ok(ExecutionResult {
                    success: false,
                    output: Some("no bridge configured".to_owned()),
                })
            }
        }
        BuiltinTask::DecayRefresh
        | BuiltinTask::EntityDedup
        | BuiltinTask::GraphRecompute
        | BuiltinTask::EmbeddingRefresh
        | BuiltinTask::KnowledgeGc
        | BuiltinTask::IndexMaintenance
        | BuiltinTask::GraphHealthCheck => {
            execute_knowledge_task(builtin, nous_id, knowledge_executor).await
        }
        BuiltinTask::TraceRotation => {
            let config = maintenance
                .map(|m| m.trace_rotation.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || TraceRotator::new(config).rotate())
                .await
                .context(error::BlockingJoinSnafu {
                    context: "trace rotation",
                })??;

            tracing::info!(
                rotated = report.files_rotated,
                pruned = report.files_pruned,
                bytes_freed = report.bytes_freed,
                "maintenance: trace rotation complete"
            );
            Ok(ExecutionResult {
                success: true,
                output: Some(format!(
                    "{} files rotated, {} pruned, {} bytes freed",
                    report.files_rotated, report.files_pruned, report.bytes_freed
                )),
            })
        }
        BuiltinTask::DriftDetection => {
            let config = maintenance
                .map(|m| m.drift_detection.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || DriftDetector::new(config).check())
                .await
                .context(error::BlockingJoinSnafu {
                    context: "drift detection",
                })??;

            tracing::info!(
                missing = report.missing_files.len(),
                extra = report.extra_files.len(),
                permission_issues = report.permission_issues.len(),
                "maintenance: drift detection complete"
            );
            Ok(ExecutionResult {
                success: true,
                output: Some(format!(
                    "{} missing, {} extra",
                    report.missing_files.len(),
                    report.extra_files.len()
                )),
            })
        }
        BuiltinTask::DbSizeMonitor => {
            let config = maintenance
                .map(|m| m.db_monitoring.clone())
                .unwrap_or_default();
            let report = tokio::task::spawn_blocking(move || DbMonitor::new(config).check())
                .await
                .context(error::BlockingJoinSnafu {
                    context: "db size monitor",
                })??;

            let summary: Vec<String> = report
                .databases
                .iter()
                .map(|db| {
                    format!(
                        "{} {}MB ({})",
                        db.name,
                        db.size_bytes / (1024 * 1024),
                        db.status
                    )
                })
                .collect();
            tracing::info!(
                databases = %summary.join(", "),
                "maintenance: db monitor complete"
            );
            Ok(ExecutionResult {
                success: true,
                output: Some(summary.join(", ")),
            })
        }
        BuiltinTask::RetentionExecution => {
            let Some(executor) = retention_executor else {
                tracing::info!("retention execution skipped — no executor configured");
                return Ok(ExecutionResult {
                    success: true,
                    output: Some("skipped — no executor".to_owned()),
                });
            };
            let summary = tokio::task::spawn_blocking(move || executor.execute_retention())
                .await
                .context(error::BlockingJoinSnafu {
                    context: "retention execution",
                })??;

            tracing::info!(
                sessions = summary.sessions_cleaned,
                messages = summary.messages_cleaned,
                bytes_freed = summary.bytes_freed,
                "maintenance: retention complete"
            );
            Ok(ExecutionResult {
                success: true,
                output: Some(format!(
                    "{} sessions, {} messages cleaned, {} bytes freed",
                    summary.sessions_cleaned, summary.messages_cleaned, summary.bytes_freed
                )),
            })
        }
        BuiltinTask::SessionRetention => {
            tracing::info!(
                nous_id = %nous_id,
                "session retention not yet wired — requires store access from daemon"
            );
            Ok(ExecutionResult {
                success: true,
                output: None,
            })
        }
    }
}

/// Dispatch a knowledge maintenance task to the executor via `spawn_blocking`.
async fn execute_knowledge_task(
    builtin: &BuiltinTask,
    nous_id: &str,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> Result<ExecutionResult> {
    let Some(executor) = knowledge_executor else {
        tracing::info!(
            task = ?builtin,
            "knowledge maintenance skipped — no executor configured"
        );
        return Ok(ExecutionResult {
            success: true,
            output: Some("skipped — no executor".to_owned()),
        });
    };

    let task_name = format!("{builtin:?}");
    let nous_id_owned = nous_id.to_owned();
    let builtin_clone = builtin.clone();

    let report = tokio::task::spawn_blocking(move || {
        let _span = tracing::info_span!(
            "knowledge_maintenance",
            task = %task_name,
            nous_id = %nous_id_owned,
        )
        .entered();

        let start = std::time::Instant::now();
        let mut report = match builtin_clone {
            BuiltinTask::DecayRefresh => executor.refresh_decay_scores(&nous_id_owned),
            BuiltinTask::EntityDedup => executor.deduplicate_entities(&nous_id_owned),
            BuiltinTask::GraphRecompute => executor.recompute_graph_scores(&nous_id_owned),
            BuiltinTask::EmbeddingRefresh => executor.refresh_embeddings(&nous_id_owned),
            BuiltinTask::KnowledgeGc => executor.garbage_collect(&nous_id_owned),
            BuiltinTask::IndexMaintenance => executor.maintain_indexes(&nous_id_owned),
            BuiltinTask::GraphHealthCheck => executor.health_check(&nous_id_owned),
            _ => unreachable!("non-knowledge task routed to execute_knowledge_task"),
        }?;

        report.duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        tracing::info!(
            items_processed = report.items_processed,
            items_modified = report.items_modified,
            duration_ms = report.duration_ms,
            errors = report.errors,
            "knowledge maintenance complete"
        );

        Ok(report)
    })
    .await
    .context(error::BlockingJoinSnafu {
        context: format!("knowledge maintenance: {builtin:?}"),
    })??;

    Ok(ExecutionResult {
        success: true,
        output: Some(format!(
            "{} processed, {} modified in {}ms",
            report.items_processed, report.items_modified, report.duration_ms
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_echo_task(id: &str) -> TaskDef {
        TaskDef {
            id: id.to_owned(),
            name: format!("Test task {id}"),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_secs(60)),
            action: TaskAction::Command("echo hello".to_owned()),
            enabled: true,
            ..TaskDef::default()
        }
    }

    #[test]
    fn register_shows_in_status() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);
        runner.register(make_echo_task("task-1"));
        runner.register(make_echo_task("task-2"));

        let statuses = runner.status();
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].id, "task-1");
        assert_eq!(statuses[1].id, "task-2");
        assert!(statuses[0].enabled);
    }

    #[tokio::test]
    async fn shutdown_exits_run_loop() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token.clone());

        let handle = tokio::spawn(async move {
            runner.run().await;
        });

        token.cancel();

        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        assert!(result.is_ok(), "runner should exit on shutdown signal");
    }

    #[tokio::test]
    async fn task_disabled_after_consecutive_failures() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);

        let task = TaskDef {
            id: "failing-task".to_owned(),
            name: "Failing task".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_millis(10)),
            action: TaskAction::Command("exit 1".to_owned()),
            enabled: true,
            ..TaskDef::default()
        };
        runner.register(task);

        for _ in 0..3 {
            runner.record_task_failure("failing-task", "exit code 1");
        }

        let statuses = runner.status();
        assert!(
            !statuses[0].enabled,
            "task should be disabled after 3 failures"
        );
        assert_eq!(statuses[0].consecutive_failures, 3);
    }

    #[tokio::test]
    async fn successful_command_resets_failures() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);

        let task = TaskDef {
            id: "echo-task".to_owned(),
            name: "Echo task".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_secs(60)),
            action: TaskAction::Command("echo ok".to_owned()),
            enabled: true,
            ..TaskDef::default()
        };
        runner.register(task);

        runner.tasks[0].consecutive_failures = 2;
        runner.record_task_completion("echo-task", Duration::from_millis(10));

        let statuses = runner.status();
        assert_eq!(statuses[0].consecutive_failures, 0);
        assert_eq!(statuses[0].run_count, 1);
        assert!(statuses[0].enabled);
    }

    #[tokio::test]
    async fn builtin_prosoche_executes() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);

        let task = TaskDef {
            id: "prosoche".to_owned(),
            name: "Prosoche check".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_secs(60)),
            action: TaskAction::Builtin(BuiltinTask::Prosoche),
            enabled: true,
            catch_up: false,
            ..TaskDef::default()
        };
        runner.register(task);

        runner.tasks[0].next_run = Some(
            jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_secs(-1))
                .unwrap(),
        );

        runner.tick();

        // Wait for the spawned task to complete.
        tokio::time::sleep(Duration::from_millis(100)).await;
        runner.check_in_flight().await;

        let statuses = runner.status();
        assert_eq!(statuses[0].run_count, 1);
        assert_eq!(statuses[0].consecutive_failures, 0);
    }

    #[test]
    fn register_maintenance_tasks_respects_enabled() {
        let token = CancellationToken::new();
        let mut config = MaintenanceConfig::default();
        config.trace_rotation.enabled = true;
        config.drift_detection.enabled = false;
        config.db_monitoring.enabled = true;
        config.retention.enabled = false;

        let mut runner = TaskRunner::new("system", token).with_maintenance(config);
        runner.register_maintenance_tasks();

        let statuses = runner.status();
        let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"trace-rotation"));
        assert!(!ids.contains(&"drift-detection"));
        assert!(ids.contains(&"db-size-monitor"));
        assert!(!ids.contains(&"retention-execution"));
    }

    #[test]
    fn register_maintenance_tasks_skips_without_config() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("system", token);
        runner.register_maintenance_tasks();
        assert!(runner.status().is_empty());
    }

    #[test]
    fn retention_requires_executor() {
        let token = CancellationToken::new();
        let mut config = MaintenanceConfig::default();
        config.retention.enabled = true;

        let mut runner = TaskRunner::new("system", token).with_maintenance(config);
        runner.register_maintenance_tasks();

        let statuses = runner.status();
        let ids: Vec<&str> = statuses.iter().map(|s| s.id.as_str()).collect();
        assert!(
            !ids.contains(&"retention-execution"),
            "retention should not register without executor"
        );
    }

    #[tokio::test]
    async fn retention_without_executor_skips() {
        let result = execute_builtin(
            &BuiltinTask::RetentionExecution,
            "system",
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_ok());
        let output = result.unwrap().output.unwrap_or_default();
        assert!(output.contains("skipped"));
    }

    #[test]
    fn status_empty_runner() {
        let token = CancellationToken::new();
        let runner = TaskRunner::new("test-nous", token);
        assert!(
            runner.status().is_empty(),
            "new runner should have no tasks"
        );
    }

    #[test]
    fn register_startup_task_immediate() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);

        let task = TaskDef {
            id: "startup-task".to_owned(),
            name: "Startup".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Startup,
            action: TaskAction::Command("echo boot".to_owned()),
            enabled: true,
            ..TaskDef::default()
        };
        let before = jiff::Timestamp::now();
        runner.register(task);

        let statuses = runner.status();
        let next_run_str = statuses[0]
            .next_run
            .as_ref()
            .expect("startup should have next_run");
        let next_run: jiff::Timestamp = next_run_str.parse().expect("valid timestamp");
        assert!(
            next_run >= before,
            "startup task next_run should be >= time before registration"
        );
    }

    #[test]
    fn with_bridge_stores_bridge() {
        let token = CancellationToken::new();
        let bridge: Arc<dyn DaemonBridge> = Arc::new(crate::bridge::NoopBridge);
        let runner = TaskRunner::with_bridge("test-nous", token, bridge);
        assert!(runner.status().is_empty());
    }

    #[test]
    fn with_maintenance_builder_pattern() {
        let token = CancellationToken::new();
        let config = MaintenanceConfig::default();
        let runner = TaskRunner::new("test-nous", token).with_maintenance(config);
        assert!(runner.status().is_empty());
    }

    #[test]
    fn with_retention_builder_pattern() {
        let token = CancellationToken::new();
        let executor: Arc<dyn crate::maintenance::RetentionExecutor> =
            Arc::new(MockRetentionExecutor);
        let runner = TaskRunner::new("test-nous", token).with_retention(executor);
        assert!(runner.status().is_empty());
    }

    struct MockRetentionExecutor;

    impl crate::maintenance::RetentionExecutor for MockRetentionExecutor {
        fn execute_retention(&self) -> crate::error::Result<crate::maintenance::RetentionSummary> {
            Ok(crate::maintenance::RetentionSummary::default())
        }
    }

    #[test]
    fn execution_result_serialization() {
        let result = ExecutionResult {
            success: true,
            output: Some("hello".to_owned()),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let back: ExecutionResult = serde_json::from_str(&json).expect("deserialize");
        assert!(back.success);
        assert_eq!(back.output.as_deref(), Some("hello"));
    }

    #[tokio::test]
    async fn disabled_task_not_in_tick() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);

        let task = TaskDef {
            id: "disabled-task".to_owned(),
            name: "Disabled".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_secs(60)),
            action: TaskAction::Command("echo should-not-run".to_owned()),
            enabled: false,
            ..TaskDef::default()
        };
        runner.register(task);

        runner.tasks[0].next_run = Some(
            jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_secs(-10))
                .unwrap(),
        );

        runner.tick();

        assert!(runner.in_flight.is_empty());
        let statuses = runner.status();
        assert_eq!(
            statuses[0].run_count, 0,
            "disabled task should not have run"
        );
    }

    #[tokio::test]
    async fn child_token_cancelled_by_parent() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        let mut runner = TaskRunner::new("test-nous", child);

        let handle = tokio::spawn(async move {
            runner.run().await;
        });

        parent.cancel();

        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        assert!(
            result.is_ok(),
            "child runner should exit when parent token is cancelled"
        );
    }

    #[tokio::test]
    async fn dropped_token_stops_runner() {
        let token = CancellationToken::new();
        let child = token.child_token();
        let mut runner = TaskRunner::new("test-nous", child);

        let handle = tokio::spawn(async move {
            runner.run().await;
        });

        token.cancel();
        drop(token);

        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        assert!(result.is_ok(), "runner should exit when token is cancelled");
    }

    #[tokio::test]
    async fn shutdown_completes_within_timeout() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token.clone());

        let handle = tokio::spawn(async move {
            runner.run().await;
        });

        token.cancel();
        let timeout = Duration::from_secs(2);
        let result = tokio::time::timeout(timeout, handle).await;
        assert!(
            result.is_ok(),
            "shutdown should complete well within {timeout:?}"
        );
    }

    /// Multiple runners with independent child tokens: cancelling one does not affect others.
    #[tokio::test]
    async fn independent_child_tokens_isolated() {
        let parent = CancellationToken::new();
        let child_a = parent.child_token();
        let child_b = parent.child_token();

        let mut runner_a = TaskRunner::new("nous-a", child_a.clone());
        let mut runner_b = TaskRunner::new("nous-b", child_b);

        let handle_a = tokio::spawn(async move { runner_a.run().await });
        let handle_b = tokio::spawn(async move { runner_b.run().await });

        child_a.cancel();

        let result_a = tokio::time::timeout(Duration::from_secs(2), handle_a).await;
        assert!(
            result_a.is_ok(),
            "runner_a should stop when its token is cancelled"
        );

        assert!(!handle_b.is_finished(), "runner_b should still be running");

        parent.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle_b).await;
    }

    // --- Exponential backoff tests ---

    #[test]
    fn backoff_applied_on_failure() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);
        runner.register(make_echo_task("backoff-task"));

        // First failure → 60s backoff.
        runner.record_task_failure("backoff-task", "test error");
        assert_eq!(runner.tasks[0].consecutive_failures, 1);
        assert!(runner.tasks[0].backoff_until.is_some());

        let backoff = runner.tasks[0].backoff_until.unwrap();
        let expected_min = Instant::now() + Duration::from_secs(55);
        assert!(
            backoff > expected_min,
            "1st failure should have ~60s backoff"
        );

        // Second failure → 300s backoff.
        runner.record_task_failure("backoff-task", "test error 2");
        assert_eq!(runner.tasks[0].consecutive_failures, 2);
        let backoff = runner.tasks[0].backoff_until.unwrap();
        let expected_min = Instant::now() + Duration::from_secs(295);
        assert!(
            backoff > expected_min,
            "2nd failure should have ~300s backoff"
        );
    }

    #[test]
    fn backoff_cleared_on_success() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);
        runner.register(make_echo_task("backoff-clear"));

        runner.record_task_failure("backoff-clear", "fail");
        assert!(runner.tasks[0].backoff_until.is_some());

        runner.record_task_completion("backoff-clear", Duration::from_millis(1));
        assert!(runner.tasks[0].backoff_until.is_none());
        assert_eq!(runner.tasks[0].consecutive_failures, 0);
    }

    // --- Hung task detection tests ---

    #[tokio::test]
    async fn hung_task_cancelled_after_2x_timeout() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);

        let task = TaskDef {
            id: "hung-task".to_owned(),
            name: "Hung task".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_secs(60)),
            action: TaskAction::Command("echo ok".to_owned()),
            timeout: Duration::from_millis(50),
            enabled: true,
            ..TaskDef::default()
        };
        runner.register(task);

        // Simulate a hung task by spawning a long sleep.
        let handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(ExecutionResult {
                success: true,
                output: None,
            })
        });

        runner.in_flight.insert(
            "hung-task".to_owned(),
            InFlightTask {
                handle,
                started_at: Instant::now()
                    .checked_sub(Duration::from_millis(150))
                    .unwrap(),
                timeout: Duration::from_millis(50),
                warned: false,
            },
        );

        runner.check_in_flight().await;

        assert!(!runner.in_flight.contains_key("hung-task"));
        assert_eq!(runner.tasks[0].consecutive_failures, 1);
    }

    // --- Missed cron catch-up tests ---

    #[test]
    fn missed_cron_catchup_fires_on_startup() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);

        let task = TaskDef {
            id: "hourly-task".to_owned(),
            name: "Hourly task".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Cron("0 0 * * * *".to_owned()),
            action: TaskAction::Command("echo hello".to_owned()),
            enabled: true,
            catch_up: true,
            ..TaskDef::default()
        };
        runner.register(task);

        let three_hours_ago = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(3))
            .unwrap();
        runner.set_last_run("hourly-task", three_hours_ago);

        // Set next_run far in the future.
        runner.tasks[0].next_run = Some(
            jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_hours(1))
                .unwrap(),
        );

        runner.check_missed_cron_catchup();

        let next = runner.tasks[0].next_run.unwrap();
        let diff = next
            .since(jiff::Timestamp::now())
            .unwrap()
            .get_seconds()
            .abs();
        assert!(diff < 5, "catch-up should set next_run to ~now");
    }

    #[test]
    fn missed_cron_catchup_skips_disabled_catch_up() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);

        let task = TaskDef {
            id: "no-catchup".to_owned(),
            name: "No catch-up".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Cron("0 0 * * * *".to_owned()),
            action: TaskAction::Command("echo hello".to_owned()),
            enabled: true,
            catch_up: false,
            ..TaskDef::default()
        };
        runner.register(task);

        let three_hours_ago = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(3))
            .unwrap();
        runner.set_last_run("no-catchup", three_hours_ago);

        let future_run = jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_hours(1))
            .unwrap();
        runner.tasks[0].next_run = Some(future_run);

        runner.check_missed_cron_catchup();

        assert_eq!(runner.tasks[0].next_run.unwrap(), future_run);
    }

    // --- Task metrics tests ---

    #[test]
    fn task_metrics_on_success() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);
        runner.register(make_echo_task("metrics-task"));

        runner.record_task_completion("metrics-task", Duration::from_millis(42));

        let statuses = runner.status();
        assert_eq!(statuses[0].run_count, 1);
        assert_eq!(statuses[0].consecutive_failures, 0);
    }

    #[test]
    fn task_metrics_on_failure() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);
        runner.register(make_echo_task("metrics-fail"));

        runner.record_task_failure("metrics-fail", "boom");

        let statuses = runner.status();
        assert_eq!(statuses[0].consecutive_failures, 1);
        assert_eq!(statuses[0].run_count, 0);
    }

    #[tokio::test]
    async fn in_flight_reported_in_status() {
        let token = CancellationToken::new();
        let mut runner = TaskRunner::new("test-nous", token);
        runner.register(make_echo_task("inflight-task"));

        let handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(ExecutionResult {
                success: true,
                output: None,
            })
        });
        runner.in_flight.insert(
            "inflight-task".to_owned(),
            InFlightTask {
                handle,
                started_at: Instant::now(),
                timeout: Duration::from_secs(300),
                warned: false,
            },
        );

        let statuses = runner.status();
        assert!(statuses[0].in_flight);

        // Clean up — abort so the test doesn't hang.
        if let Some(task) = runner.in_flight.remove("inflight-task") {
            task.handle.abort();
        }
    }
}
