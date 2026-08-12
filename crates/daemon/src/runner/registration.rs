//! Task registration: builtin task setup, maintenance tasks, cron tasks.

use crate::maintenance::MaintenanceConfig;
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
            last_errors: 0,
        });
    }

    /// Reconcile live scheduler state against an updated [`MaintenanceConfig`].
    ///
    /// Recomputes the desired maintenance-registry task set from `config`
    /// and this runner's existing capabilities (retention/knowledge
    /// executors, bridge -- unaffected by a config reload), then applies the
    /// difference: a task the registry no longer produces is REMOVED, a task
    /// newly produced is ADDED (hydrating any persisted run history so a
    /// disable/re-enable round-trip does not read as a brand-new task), and
    /// a task whose schedule changed is RE-ARMED. `self.maintenance` is then
    /// replaced, so every subsequent tick executes tasks against the fresh
    /// config values (thresholds, paths, retention windows) regardless of
    /// whether their schedule changed.
    ///
    /// INVARIANT: never touches [`Self::in_flight`]. A task already running
    /// keeps running to completion under the config it started with; this
    /// method only decides what the schedule looks like AFTER that
    /// execution. A task still in-flight when its schedule changes has its
    /// `next_run` left untouched here -- [`Self::record_task_completion`]
    /// re-arms it against the new `def.schedule` once the run finishes, so
    /// there is never a second `next_run` computed while one is still live,
    /// and no run is ever spawned twice for the same task id (`tick` already
    /// refuses to spawn while `in_flight` holds the id). A task removed from
    /// the desired set while in-flight is dropped from `self.tasks`
    /// immediately -- it will not fire again -- but its running execution is
    /// left alone; `check_in_flight` still logs its outcome, and the
    /// `record_task_*` id lookup it feeds into on completion silently finds
    /// nothing to update, the same no-op path already used whenever a
    /// completion arrives for a task id that is no longer registered.
    pub fn reconcile_maintenance(&mut self, config: MaintenanceConfig) {
        let capabilities = MaintenanceRuntimeCapabilities {
            has_retention_executor: self.retention_executor.is_some(),
            has_knowledge_executor: self.knowledge_executor.is_some(),
            has_bridge: self.bridge.is_some(),
        };

        let mut added = 0u32;
        let mut removed = 0u32;
        let mut rescheduled = 0u32;

        for definition in maintenance_task_registry() {
            let desired = definition.scheduled_task(&config, capabilities);
            let existing_idx = self.tasks.iter().position(|t| t.def.id == definition.id());

            match (desired, existing_idx) {
                (Some(task), None) => {
                    self.register_builtin(
                        task.id,
                        task.name,
                        task.schedule,
                        task.builtin,
                        task.catch_up,
                    );
                    self.hydrate_task_state(task.id);
                    added += 1;
                }
                (Some(task), Some(idx)) => {
                    if let Some(existing) = self.tasks.get_mut(idx)
                        && existing.def.schedule != task.schedule
                    {
                        existing.def.schedule = task.schedule;
                        // WHY: leave an in-flight task's next_run untouched;
                        // record_task_completion re-arms it against the
                        // fresh schedule when the run finishes (see
                        // INVARIANT above).
                        if !self.in_flight.contains_key(&existing.def.id) {
                            let base = existing.def.schedule.next_run().unwrap_or(None);
                            existing.next_run =
                                apply_jitter(base, &existing.def.id, existing.def.jitter)
                                    .or(base);
                        }
                        rescheduled += 1;
                    }
                }
                (None, Some(idx)) => {
                    self.tasks.remove(idx);
                    removed += 1;
                }
                (None, None) => {}
            }
        }

        self.maintenance = Some(config);

        if added > 0 || removed > 0 || rescheduled > 0 {
            tracing::info!(
                nous_id = %self.nous_id,
                added,
                removed,
                rescheduled,
                "maintenance scheduler reconciled against updated config"
            );
        }
    }

    /// Non-blocking check for a pending maintenance config update.
    ///
    /// Called once per scheduler tick (<=1s latency) rather than as a
    /// `tokio::select!` branch in [`super::lifecycle`]: a select branch would
    /// need to borrow `self.maintenance_reload_rx` for the whole poll
    /// alongside the loop's other per-field borrows, and a plain per-tick
    /// check is simpler to reason about for a channel with a single writer
    /// (the config-reload task).
    pub(super) fn poll_maintenance_reload(&mut self) {
        let Some(rx) = self.maintenance_reload_rx.as_mut() else {
            return;
        };
        match rx.has_changed() {
            Ok(true) => {
                let config = rx.borrow_and_update().clone();
                self.reconcile_maintenance(config);
            }
            Ok(false) => {}
            Err(_) => {
                // WHY: sender dropped -- stop polling a channel that can
                // never produce another update.
                self.maintenance_reload_rx = None;
            }
        }
    }
}
