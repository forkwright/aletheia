//! Task state persistence: save/restore and cron catch-up.

use std::time::Instant;

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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "daemon task runner configuration")
    )]
    pub(crate) fn set_last_run(&mut self, task_id: &str, last_run: jiff::Timestamp) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) {
            task.last_run = Some(last_run);
        }
    }

    /// Restore persisted task state from the task-state store (if attached).
    ///
    /// Called once at startup, before catch-up checking. Skips silently when
    /// no store is configured or when a task ID in the store no longer exists.
    ///
    /// Public so external tooling (e.g. the `maintenance status` CLI) can
    /// hydrate a fresh runner with the daemon's persisted state. (#5131)
    pub fn restore_state(&mut self) {
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

                    // WHY(#5130): restore the enabled flag so an auto-disabled
                    // task stays disabled across restarts.
                    if let Some(enabled) = saved.enabled {
                        task.def.enabled = enabled;
                    }
                    task.last_error = saved.last_error;

                    // WHY(#5130): a future backoff deadline must be re-armed
                    // against the monotonic clock. `Instant` is not persistable,
                    // so we recompute `now + remaining` from the wall-clock
                    // deadline. Past deadlines clear the backoff.
                    task.backoff_until = restore_backoff(saved.backoff_until_ts.as_deref());
                }
                tracing::info!(nous_id = %self.nous_id, "task state restored from store");
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

    /// Persist a single task's state to the store, if one is attached.
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

/// Re-arm a persisted wall-clock backoff deadline against the monotonic clock.
///
/// Returns `Some(Instant)` when the deadline is in the future, `None` when the
/// deadline has passed, is absent, or cannot be parsed.
fn restore_backoff(backoff_until_ts: Option<&str>) -> Option<Instant> {
    let parsed = backoff_until_ts?.parse::<jiff::Timestamp>().ok()?;
    let now = jiff::Timestamp::now();
    let remaining = parsed.duration_since(now);
    let nanos = remaining.as_nanos();
    if nanos <= 0 {
        return None;
    }
    let duration = std::time::Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX));
    Instant::now().checked_add(duration)
}
