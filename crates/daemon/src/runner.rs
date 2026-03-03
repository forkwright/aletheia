//! Per-nous background task runner with cron scheduling, failure tracking, and graceful shutdown.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tokio::sync::watch;

use crate::error::{self, Result};
use crate::prosoche::ProsocheCheck;
use crate::schedule::{BuiltinTask, Schedule, TaskAction, TaskDef, TaskStatus};

/// Per-nous background task runner.
pub struct TaskRunner {
    nous_id: String,
    tasks: Vec<RegisteredTask>,
    shutdown: watch::Receiver<bool>,
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
    pub success: bool,
    pub output: Option<String>,
}

impl TaskRunner {
    /// Create a task runner for a nous. The runner stops when `shutdown` emits `true`.
    pub fn new(nous_id: impl Into<String>, shutdown: watch::Receiver<bool>) -> Self {
        Self {
            nous_id: nous_id.into(),
            tasks: Vec::new(),
            shutdown,
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

        for task in &mut self.tasks {
            if !task.def.enabled {
                continue;
            }

            let Some(next) = task.next_run else {
                continue;
            };

            if next > now {
                continue;
            }

            if !Schedule::in_window(task.def.active_window) {
                continue;
            }

            let result = execute_action(&task.def.action, &task.def.nous_id).await;
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
}

async fn execute_action(action: &TaskAction, nous_id: &str) -> Result<ExecutionResult> {
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
            tracing::info!(
                nous_id = %nous_id,
                prompt_len = prompt.len(),
                "prompt injection not yet wired — requires nous pipeline access"
            );
            Ok(ExecutionResult {
                success: true,
                output: None,
            })
        }
        TaskAction::Builtin(builtin) => execute_builtin(builtin, nous_id).await,
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

async fn execute_builtin(builtin: &BuiltinTask, nous_id: &str) -> Result<ExecutionResult> {
    match builtin {
        BuiltinTask::Prosoche => {
            let check = ProsocheCheck::new(nous_id);
            let result = check.run().await?;
            Ok(ExecutionResult {
                success: true,
                output: Some(format!("{} items", result.items.len())),
            })
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
            action: TaskAction::Command(
                "exit 1".to_owned(),
            ),
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
        assert!(!statuses[0].enabled, "task should be disabled after 3 failures");
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
}
