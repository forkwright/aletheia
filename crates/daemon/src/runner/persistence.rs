//! Task state persistence: save/restore, live external-write sync, and cron catch-up.

use std::time::Instant;

use crate::state::{DisableCause, TaskState};

use super::{RegisteredTask, TaskRunner};

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
                    apply_saved_state(task, saved, Hydrating::Yes);
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

    /// Re-apply persisted state for already-registered tasks from the
    /// attached state store, without the hydration-only auto-disable retry.
    ///
    /// WHY(#7206): the persisted [`crate::state::TaskStateStore`] is the only
    /// channel that crosses the process boundary between this live runner and
    /// external writers -- the `aletheia maintenance reset` CLI, and the
    /// pylon daemon-task admin API (list/enable/disable/retry) added
    /// alongside this method. Before this, an external write only took
    /// effect at the next full process restart (`restore_state`) or
    /// live config-driven re-registration (`hydrate_task_state`); an operator
    /// calling `reset` or the new API had no way to make it stick without a
    /// restart. Called periodically from [`super::lifecycle::TaskRunner::run`]
    /// (not every 1s tick -- see the caller for the interval).
    ///
    /// Deliberately does not call [`apply_saved_state`]'s hydration-only
    /// retry: an auto-disabled task that nobody has touched must stay
    /// disabled between restarts (#5130); only an external write that
    /// actually changes `enabled` or `disable_cause` should have any
    /// observable effect here, which literal field mirroring already gives.
    pub(crate) fn sync_external_state(&mut self) {
        let Some(ref store) = self.state_store else {
            return;
        };
        let Ok(states) = store.load_all() else {
            // WHY: a transient read failure here is not worth warning about
            // on every sync interval -- `restore_state` already warns loudly
            // for the startup case, which is the one that matters most.
            return;
        };
        for saved in states {
            // WHY: skip a task currently executing -- applying an external
            // flag mid-run would race the completion handler's own persist
            // moments later, and the in-flight run itself is unaffected by
            // `enabled` either way (`tick` only consults it before spawning).
            if self.in_flight.contains_key(&saved.task_id) {
                continue;
            }
            let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == saved.task_id) else {
                continue;
            };
            apply_saved_state(task, saved, Hydrating::No);
        }
    }

    /// Hydrate a single task's persisted execution history from the state
    /// store, if one is attached and a record exists.
    ///
    /// WHY: [`super::TaskRunner::reconcile_maintenance`] re-registers a task
    /// fresh (`run_count: 0`, ...) when config re-enables it after being
    /// disabled. Without this, a disable/re-enable round-trip during a live
    /// reload would read as a brand-new task even though [`Self::restore_state`]
    /// would have restored the exact same history had the process instead
    /// been restarted -- this keeps the two paths consistent.
    pub(super) fn hydrate_task_state(&mut self, task_id: &str) {
        let Some(ref store) = self.state_store else {
            return;
        };
        let Ok(states) = store.load_all() else {
            return;
        };
        let Some(saved) = states.into_iter().find(|s| s.task_id == task_id) else {
            return;
        };
        let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) else {
            return;
        };
        apply_saved_state(task, saved, Hydrating::Yes);
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

/// Whether [`apply_saved_state`] is running as part of a hydration (process
/// startup, or a live reconciliation re-add) versus a periodic external-write
/// sync while the runner is already up.
///
/// WHY(#7206): only a hydration gets the one-retry treatment for an
/// auto-disabled task -- see [`apply_saved_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hydrating {
    /// [`TaskRunner::restore_state`] or [`TaskRunner::hydrate_task_state`].
    Yes,
    /// [`TaskRunner::sync_external_state`].
    No,
}

/// Apply one persisted [`TaskState`] record onto its matching in-memory task.
///
/// Shared by [`TaskRunner::restore_state`] (bulk, at startup),
/// [`TaskRunner::hydrate_task_state`] (single task, on a live reconciliation
/// add), and [`TaskRunner::sync_external_state`] (periodic, while running) so
/// the paths cannot drift on the base field-copy behavior. `hydrating`
/// controls the one part that must NOT be shared: the auto-disable retry
/// below only fires for an actual hydration.
fn apply_saved_state(task: &mut RegisteredTask, saved: TaskState, hydrating: Hydrating) {
    if let Some(Ok(ts)) = saved
        .last_run_ts
        .as_deref()
        .map(str::parse::<jiff::Timestamp>)
    {
        task.last_run = Some(ts);
    }
    task.run_count = saved.run_count;
    task.consecutive_failures = saved.consecutive_failures;

    let was_enabled = task.def.enabled;

    // WHY(#5130): restore the enabled flag so a disabled task stays disabled
    // across restarts (and across a reconciliation re-add) by default.
    if let Some(enabled) = saved.enabled {
        task.def.enabled = enabled;
    }
    task.last_error = saved.last_error;
    task.disable_cause = saved.disable_cause;

    // WHY(#5130): a future backoff deadline must be re-armed against the
    // monotonic clock. `Instant` is not persistable, so we recompute
    // `now + remaining` from the wall-clock deadline. Past deadlines clear
    // the backoff.
    task.backoff_until = restore_backoff(saved.backoff_until_ts.as_deref());

    // WHY(#7206): a persisted disable must not mean "disabled forever" by
    // persistence alone. #5130's stated worry was a restart silently
    // *hiding* an auto-disable (re-enabling with no evidence anything was
    // fixed) -- not that an auto-disabled task must never run again. This
    // reconciles both: an `AutoFailure` disable (the runner's own
    // 3-consecutive-failure policy -- frequently an environmental condition
    // like a directory that did not exist yet at boot, see the #7206
    // `AfterActionStore` fix) gets re-armed for exactly one retry per
    // hydration, scheduled to fire on the very next tick so the operator does
    // not wait out the task's normal cadence to find out whether the
    // environment recovered. `consecutive_failures` is deliberately left at
    // its persisted value (already >= 3): a single further failure trips
    // `record_task_failure`'s existing `>= 3` auto-disable check immediately,
    // so a genuinely still-broken task goes right back to `disabled` with
    // `disable_cause: AutoFailure` after one visible, logged attempt --
    // nothing is silently healed or silently re-hidden. `None` is folded into
    // the same treatment as `AutoFailure`: every persisted `enabled:
    // Some(false)` written before this field existed was, by construction,
    // an auto-disable (the admin API and this cause distinction did not
    // exist yet). An `Operator` disable is a decision, not a symptom, and
    // is intentionally exempt: it stays disabled across every hydration
    // until an operator/agent re-enables it through the admin API (or the
    // `aletheia maintenance reset` CLI), exactly matching #5130's acceptance
    // criterion that only "an explicit operator reset" un-disables a task.
    if hydrating == Hydrating::Yes
        && !task.def.enabled
        && !matches!(task.disable_cause, Some(DisableCause::Operator))
    {
        tracing::info!(
            task_id = %task.def.id,
            task_name = %task.def.name,
            consecutive_failures = task.consecutive_failures,
            disable_cause = ?task.disable_cause,
            "auto-disabled task re-armed for one retry on hydration"
        );
        task.def.enabled = true;
        // WHY: `disable_cause` describes why a task IS disabled; this task is
        // not, for the moment, so a stale `AutoFailure` from before the retry
        // must not linger and read as a contradiction in `status()`/the admin
        // API. `record_task_failure` sets it again immediately if the retry
        // fails.
        task.disable_cause = None;
        task.next_run = Some(jiff::Timestamp::now());
        return;
    }

    // WHY(#7206): a live external write (the admin API's enable/retry, or
    // `aletheia maintenance reset`) that flips a task back on should not have
    // to wait out its normal schedule to prove itself -- the whole point of
    // exposing "retry" as an action is an immediate attempt, not a promise to
    // try again eventually. Scoped to the disabled->enabled transition only:
    // an already-enabled task's `next_run` is scheduler state this function
    // has no business overwriting.
    if hydrating == Hydrating::No && !was_enabled && task.def.enabled {
        task.next_run = Some(jiff::Timestamp::now());
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
