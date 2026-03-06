//! Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tokio::sync::watch;

use crate::bridge::DaemonBridge;
use crate::error::{self, Result};
use crate::maintenance::{
    DbMonitor, DriftDetector, MaintenanceConfig, RetentionExecutor, TraceRotator,
};
use crate::schedule::{BuiltinTask, Schedule, TaskAction, TaskDef, TaskStatus};

/// Per-nous background task runner.
pub struct TaskRunner {
    nous_id: String,
    tasks: Vec<RegisteredTask>,
    shutdown: watch::Receiver<bool>,
    bridge: Option<Arc<dyn DaemonBridge>>,
    maintenance: Option<MaintenanceConfig>,
    retention_executor: Option<Arc<dyn RetentionExecutor>>,
}

struct RegisteredTask {
    def: TaskDef,
    next_run: Option<jiff::Timestamp>,
    last_run: Option<jiff::Timestamp>,
    run_count: u64,
    consecutive_failures: u32,
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
    /// Create a runner for the given nous, listening for shutdown on the watch channel.
    pub fn new(nous_id: impl Into<String>, shutdown: watch::Receiver<bool>) -> Self {
        Self {
            nous_id: nous_id.into(),
            tasks: Vec::new(),
            shutdown,
            bridge: None,
            maintenance: None,
            retention_executor: None,
        }
    }

    /// Create a runner with a bridge for nous communication.
    pub fn with_bridge(
        nous_id: impl Into<String>,
        shutdown: watch::Receiver<bool>,
        bridge: Arc<dyn DaemonBridge>,
    ) -> Self {
        Self {
            nous_id: nous_id.into(),
            tasks: Vec::new(),
            shutdown,
            bridge: Some(bridge),
            maintenance: None,
            retention_executor: None,
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
                active_window: None,
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
                active_window: None,
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
                active_window: None,
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
                active_window: None,
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
        });
    }

    /// Run the event loop. Checks for due tasks every second, executes them.
    /// Returns when shutdown signal is received.
    pub async fn run(&mut self) {
        tracing::info!(nous_id = %self.nous_id, tasks = self.tasks.len(), "daemon started");

        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.tick().await;
                }
                result = self.shutdown.changed() => {
                    if result.is_err() || *self.shutdown.borrow() {
                        tracing::info!(nous_id = %self.nous_id, "daemon shutting down");
                        break;
                    }
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
            })
            .collect()
    }

    async fn tick(&mut self) {
        let now = jiff::Timestamp::now();

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

            // Clone action/nous_id to release borrow on self before calling methods.
            let action = self.tasks[i].def.action.clone();
            let nous_id = self.tasks[i].def.nous_id.clone();

            let result = self.execute_action(&action, &nous_id).await;
            let task = &mut self.tasks[i];
            task.last_run = Some(now);

            match result {
                Ok(_) => {
                    task.run_count += 1;
                    task.consecutive_failures = 0;
                    task.next_run = task.def.schedule.next_run().unwrap_or(None);
                    tracing::debug!(
                        task_id = %task.def.id,
                        run_count = task.run_count,
                        "task completed"
                    );
                }
                Err(e) => {
                    task.consecutive_failures += 1;
                    tracing::warn!(
                        task_id = %task.def.id,
                        failures = task.consecutive_failures,
                        error = %e,
                        "task failed"
                    );

                    if task.consecutive_failures >= 3 {
                        task.def.enabled = false;
                        tracing::warn!(
                            task_id = %task.def.id,
                            failures = task.consecutive_failures,
                            "task disabled after consecutive failures"
                        );
                    } else {
                        task.next_run = task.def.schedule.next_run().unwrap_or(None);
                    }
                }
            }
        }
    }

    async fn execute_action(&self, action: &TaskAction, nous_id: &str) -> Result<ExecutionResult> {
        match action {
            TaskAction::Command(cmd) => Self::execute_command(cmd).await,
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
                if let Some(bridge) = &self.bridge {
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
            TaskAction::Builtin(builtin) => self.execute_builtin(builtin, nous_id).await,
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
        &self,
        builtin: &BuiltinTask,
        nous_id: &str,
    ) -> Result<ExecutionResult> {
        match builtin {
            BuiltinTask::Prosoche => {
                if let Some(bridge) = &self.bridge {
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
            BuiltinTask::GraphMaintenance => {
                tracing::info!(
                    nous_id = %nous_id,
                    "graph maintenance not yet implemented — requires mneme integration"
                );
                Ok(ExecutionResult {
                    success: true,
                    output: None,
                })
            }
            BuiltinTask::MemoryConsolidation => {
                tracing::info!(
                    nous_id = %nous_id,
                    "memory consolidation not yet implemented — requires melete integration"
                );
                Ok(ExecutionResult {
                    success: true,
                    output: None,
                })
            }
            BuiltinTask::TraceRotation => {
                let config = self
                    .maintenance
                    .as_ref()
                    .map(|m| m.trace_rotation.clone())
                    .unwrap_or_default();
                let report =
                    tokio::task::spawn_blocking(move || TraceRotator::new(config).rotate())
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
                let config = self
                    .maintenance
                    .as_ref()
                    .map(|m| m.drift_detection.clone())
                    .unwrap_or_default();
                let report =
                    tokio::task::spawn_blocking(move || DriftDetector::new(config).check())
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
                let config = self
                    .maintenance
                    .as_ref()
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
                let Some(executor) = self.retention_executor.as_ref().map(Arc::clone) else {
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
            active_window: None,
        }
    }

    #[test]
    fn register_shows_in_status() {
        let (_tx, rx) = watch::channel(false);
        let mut runner = TaskRunner::new("test-nous", rx);
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
        let (tx, rx) = watch::channel(false);
        let mut runner = TaskRunner::new("test-nous", rx);

        let handle = tokio::spawn(async move {
            runner.run().await;
        });

        // Signal shutdown
        tx.send(true).expect("send shutdown");

        // Verify it exits within a reasonable time
        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        assert!(result.is_ok(), "runner should exit on shutdown signal");
    }

    #[tokio::test]
    async fn task_disabled_after_consecutive_failures() {
        tokio::time::pause();

        let (_tx, rx) = watch::channel(false);
        let mut runner = TaskRunner::new("test-nous", rx);

        // Register a task that will fail (nonexistent command)
        let task = TaskDef {
            id: "failing-task".to_owned(),
            name: "Failing task".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_millis(10)),
            action: TaskAction::Command("exit 1".to_owned()),
            enabled: true,
            active_window: None,
        };
        runner.register(task);

        // Force next_run to now so it fires immediately
        runner.tasks[0].next_run = Some(jiff::Timestamp::now());

        // Run 3 ticks — each should fail and increment consecutive_failures
        for _ in 0..3 {
            runner.tasks[0].next_run = Some(
                jiff::Timestamp::now()
                    .checked_add(jiff::SignedDuration::from_secs(-1))
                    .unwrap(),
            );
            runner.tick().await;
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
        let (_tx, rx) = watch::channel(false);
        let mut runner = TaskRunner::new("test-nous", rx);

        let task = TaskDef {
            id: "echo-task".to_owned(),
            name: "Echo task".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_secs(60)),
            action: TaskAction::Command("echo ok".to_owned()),
            enabled: true,
            active_window: None,
        };
        runner.register(task);

        // Set up as if it had 2 prior failures
        runner.tasks[0].consecutive_failures = 2;
        runner.tasks[0].next_run = Some(
            jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_secs(-1))
                .unwrap(),
        );

        runner.tick().await;

        let statuses = runner.status();
        assert_eq!(statuses[0].consecutive_failures, 0);
        assert_eq!(statuses[0].run_count, 1);
        assert!(statuses[0].enabled);
    }

    #[tokio::test]
    async fn builtin_prosoche_executes() {
        let (_tx, rx) = watch::channel(false);
        let mut runner = TaskRunner::new("test-nous", rx);

        let task = TaskDef {
            id: "prosoche".to_owned(),
            name: "Prosoche check".to_owned(),
            nous_id: "test-nous".to_owned(),
            schedule: Schedule::Interval(Duration::from_secs(60)),
            action: TaskAction::Builtin(BuiltinTask::Prosoche),
            enabled: true,
            active_window: None,
        };
        runner.register(task);

        runner.tasks[0].next_run = Some(
            jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_secs(-1))
                .unwrap(),
        );

        runner.tick().await;

        let statuses = runner.status();
        assert_eq!(statuses[0].run_count, 1);
        assert_eq!(statuses[0].consecutive_failures, 0);
    }

    #[test]
    fn register_maintenance_tasks_respects_enabled() {
        let (_tx, rx) = watch::channel(false);
        let mut config = MaintenanceConfig::default();
        config.trace_rotation.enabled = true;
        config.drift_detection.enabled = false;
        config.db_monitoring.enabled = true;
        config.retention.enabled = false;

        let mut runner = TaskRunner::new("system", rx).with_maintenance(config);
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
        let (_tx, rx) = watch::channel(false);
        let mut runner = TaskRunner::new("system", rx);
        runner.register_maintenance_tasks();
        assert!(runner.status().is_empty());
    }

    #[test]
    fn retention_requires_executor() {
        let (_tx, rx) = watch::channel(false);
        let mut config = MaintenanceConfig::default();
        config.retention.enabled = true;

        let mut runner = TaskRunner::new("system", rx).with_maintenance(config);
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
        let (_tx, rx) = watch::channel(false);
        let runner = TaskRunner::new("system", rx);

        let result = runner
            .execute_builtin(&BuiltinTask::RetentionExecution, "system")
            .await;
        assert!(result.is_ok());
        let output = result.unwrap().output.unwrap_or_default();
        assert!(output.contains("skipped"));
    }
}
