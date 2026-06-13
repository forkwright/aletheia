//! Task registration: builtin task setup, maintenance tasks, cron tasks.

use crate::maintenance::registry::{MaintenanceRuntimeCapabilities, maintenance_task_registry};
use crate::schedule::{BuiltinTask, Schedule, TaskAction, TaskDef, apply_jitter};

use super::{RegisteredTask, TaskRunner};

impl TaskRunner {
    /// Register a builtin task with standard defaults, binding it to this runner's `nous_id`.
    fn register_builtin(
        &mut self,
        id: &str,
        name: &str,
        schedule: Schedule,
        task: BuiltinTask,
        catch_up: bool,
    ) {
        self.register(TaskDef {
            id: id.to_owned(),
            name: name.to_owned(),
            nous_id: self.nous_id.clone(),
            schedule,
            action: TaskAction::Builtin(task),
            enabled: true,
            catch_up,
            ..TaskDef::default()
        });
    }

    /// Register default maintenance tasks based on configuration.
    ///
    /// Skips disabled tasks and retention when no executor is provided.
    pub fn register_maintenance_tasks(&mut self) {
        let Some(config) = self.maintenance.clone() else {
            return;
        };
        let capabilities = MaintenanceRuntimeCapabilities {
            has_retention_executor: self.retention_executor.is_some(),
            has_knowledge_executor: self.knowledge_executor.is_some(),
            has_bridge: self.bridge.is_some(),
        };

        for definition in maintenance_task_registry() {
            if let Some(warning) = definition.skipped_warning(&config, capabilities) {
                tracing::warn!(
                    task = warning.task_id,
                    reason = warning.reason,
                    "skipping configured maintenance task"
                );
            }

            let Some(task) = definition.scheduled_task(&config, capabilities) else {
                continue;
            };
            self.register_builtin(
                task.id,
                task.name,
                task.schedule,
                task.builtin,
                task.catch_up,
            );
        }
    }

    /// Register a task. Startup tasks are marked for immediate execution.
    ///
    /// If the task has jitter configured, it is applied to the initial `next_run`.
    pub fn register(&mut self, task: TaskDef) {
        let base_next_run = match &task.schedule {
            Schedule::Startup => Some(jiff::Timestamp::now()),
            other => other.next_run().unwrap_or(None),
        };

        // WHY: apply jitter to spread task executions that share the same schedule.
        let next_run = apply_jitter(base_next_run, &task.id, task.jitter).or(base_next_run);

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
            last_error: None,
        });
    }
}
