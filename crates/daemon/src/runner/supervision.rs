//! `TaskRunner` integration for the per-process watchdog.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

use crate::watchdog::{ProcessHandle, ProcessStatus, RestartEvent, Watchdog, WatchdogConfig};

use super::TaskRunner;

pub(super) struct TaskWatchdog {
    watchdog: Watchdog,
    command_tx: UnboundedSender<TaskWatchdogCommand>,
    command_rx: UnboundedReceiver<TaskWatchdogCommand>,
}

impl TaskWatchdog {
    pub(super) fn new(config: WatchdogConfig, shutdown: CancellationToken) -> Self {
        let (command_tx, command_rx) = unbounded_channel();
        Self {
            watchdog: Watchdog::new(config, shutdown),
            command_tx,
            command_rx,
        }
    }

    pub(super) fn check_interval(&self) -> Duration {
        self.watchdog.check_interval()
    }

    pub(super) fn register(&mut self, task_id: &str) {
        let handle = std::sync::Arc::new(TaskProcessHandle {
            task_id: task_id.to_owned(),
            command_tx: self.command_tx.clone(),
        });
        self.watchdog.register(handle);
        self.watchdog.heartbeat(task_id);
    }

    pub(super) fn unregister(&mut self, task_id: &str) {
        self.watchdog.unregister(task_id);
    }

    pub(super) fn report_exit(&mut self, task_id: &str, reason: &str) {
        self.watchdog.report_exit(task_id, reason);
    }

    pub(super) async fn sweep(&mut self) -> Vec<TaskWatchdogCommand> {
        self.watchdog.sweep().await;
        let mut commands = Vec::new();
        while let Ok(command) = self.command_rx.try_recv() {
            commands.push(command);
        }
        commands
    }

    pub(super) fn status(&self) -> Vec<ProcessStatus> {
        self.watchdog.status()
    }

    pub(super) fn restart_log(&self) -> &[RestartEvent] {
        self.watchdog.restart_log()
    }
}

#[derive(Debug)]
pub(super) enum TaskWatchdogCommand {
    Kill { task_id: String },
    Restart { task_id: String },
}

struct TaskProcessHandle {
    task_id: String,
    command_tx: UnboundedSender<TaskWatchdogCommand>,
}

impl ProcessHandle for TaskProcessHandle {
    fn id(&self) -> &str {
        &self.task_id
    }

    fn kill(&self) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + '_>> {
        let task_id = self.task_id.clone();
        let command_tx = self.command_tx.clone();
        Box::pin(async move {
            let error_task_id = task_id.clone();
            command_tx
                .send(TaskWatchdogCommand::Kill { task_id })
                .map_err(|_send_error| {
                    crate::error::TaskFailedSnafu {
                        task_id: error_task_id,
                        reason: "watchdog command channel closed".to_owned(),
                    }
                    .build()
                })
        })
    }

    fn restart(&self) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + '_>> {
        let task_id = self.task_id.clone();
        let command_tx = self.command_tx.clone();
        Box::pin(async move {
            let error_task_id = task_id.clone();
            command_tx
                .send(TaskWatchdogCommand::Restart { task_id })
                .map_err(|_send_error| {
                    crate::error::TaskFailedSnafu {
                        task_id: error_task_id,
                        reason: "watchdog command channel closed".to_owned(),
                    }
                    .build()
                })
        })
    }
}

impl TaskRunner {
    pub(super) fn process_watchdog_interval(&self) -> Option<Duration> {
        self.watchdog.as_ref().map(TaskWatchdog::check_interval)
    }

    pub(super) fn register_watchdog_process(&mut self, task_id: &str) {
        if let Some(watchdog) = self.watchdog.as_mut() {
            watchdog.register(task_id);
        }
    }

    pub(super) fn unregister_watchdog_process(&mut self, task_id: &str) {
        if let Some(watchdog) = self.watchdog.as_mut() {
            watchdog.unregister(task_id);
        }
    }

    pub(super) fn report_watchdog_exit(&mut self, task_id: &str, reason: &str) {
        if let Some(watchdog) = self.watchdog.as_mut() {
            watchdog.report_exit(task_id, reason);
        }
    }

    pub(super) async fn check_task_watchdog(&mut self) {
        let Some(watchdog) = self.watchdog.as_mut() else {
            return;
        };
        let commands = watchdog.sweep().await;
        for command in commands {
            self.apply_watchdog_command(command);
        }
    }

    fn apply_watchdog_command(&mut self, command: TaskWatchdogCommand) {
        match command {
            TaskWatchdogCommand::Kill { task_id } => {
                if let Some(in_flight) = self.in_flight.remove(&task_id) {
                    tracing::warn!(
                        task_id = %task_id,
                        "watchdog requested task cancellation"
                    );
                    in_flight.cancel.cancel();
                    in_flight.handle.abort();
                    self.unregister_watchdog_process(&task_id);
                    self.record_task_failure(&task_id, "watchdog restart requested");
                }
            }
            TaskWatchdogCommand::Restart { task_id } => {
                let Some(task) = self.tasks.iter_mut().find(|task| task.def.id == task_id) else {
                    tracing::warn!(
                        task_id = %task_id,
                        "watchdog requested restart for unknown task"
                    );
                    return;
                };
                if !task.def.enabled {
                    tracing::warn!(
                        task_id = %task_id,
                        "watchdog restart skipped because task is disabled"
                    );
                    return;
                }
                task.backoff_until = None;
                task.next_run = Some(jiff::Timestamp::now());
                tracing::info!(
                    task_id = %task_id,
                    "watchdog scheduled task restart"
                );
            }
        }
    }
}
