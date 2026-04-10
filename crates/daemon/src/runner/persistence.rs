//! Task state persistence: SQLite-backed save/restore and cron catch-up.

use super::TaskRunner;

impl TaskRunner {
    /// Check each cron task for missed windows and run catch-up if needed.
    ///
    /// Called once at startup. For each task with `catch_up: true` and a cron
    /// schedule, checks if a window was missed within the last 24 hours.
    /// If so, schedules the task for immediate execution.
    pub(crate) fn check_missed_cron_catchup(&mut self) {
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
                        "missed cron window detected  -  scheduling catch-up"
                    );
                    task.next_run = Some(jiff::Timestamp::now());
                }
                // NOTE: no missed cron window, no catch-up needed
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
    #[cfg_attr(not(test), expect(dead_code, reason = "daemon task runner configuration"))]
    pub(crate) fn set_last_run(&mut self, task_id: &str, last_run: jiff::Timestamp) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) {
            task.last_run = Some(last_run);
        }
    }

    /// Restore persisted task state FROM the `SQLite` store (if attached).
    ///
    /// Called once at startup, before catch-up checking. Skips silently when
    /// no store is configured or when a task ID in the store no longer exists.
    pub(super) fn restore_state(&mut self) {
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
                tracing::info!(nous_id = %self.nous_id, "task state restored FROM SQLite");
            }
            Err(e) => {
                tracing::warn!(
                    nous_id = %self.nous_id,
                    error = %e,
                    "failed to restore task state  -  starting fresh"
                );
            }
        }
    }

    /// Persist a single task's state to the `SQLite` store, if one is attached.
    pub(super) fn persist_task_state(&self, state: &crate::state::TaskState) {
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
}
