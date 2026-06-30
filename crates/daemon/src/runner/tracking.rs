//! Task completion and failure tracking: metrics, backoff, auto-disable.

use std::time::{Duration, Instant};

use crate::schedule::{apply_jitter, backoff_delay};
use crate::state::{TASK_STATE_SCHEMA_VERSION, TaskState};

use super::{RegisteredTask, TaskRunner, redact_task_text};

/// Build a persisted [`TaskState`] snapshot from a registered task.
///
/// WHY(#5130): the persisted record captures enough state to faithfully
/// restore scheduling decisions across restarts: enabled flag, backoff
/// deadline, last error, and last outcome.
fn snapshot_task_state(
    task: &RegisteredTask,
    backoff_until_ts: Option<String>,
    last_outcome: &str,
) -> TaskState {
    TaskState {
        task_id: task.def.id.clone(),
        last_run_ts: task.last_run.map(|ts| ts.to_string()),
        run_count: task.run_count,
        consecutive_failures: task.consecutive_failures,
        schema_version: TASK_STATE_SCHEMA_VERSION,
        enabled: Some(task.def.enabled),
        backoff_until_ts,
        last_error: task.last_error.clone(),
        last_outcome: Some(last_outcome.to_owned()),
    }
}

impl TaskRunner {
    /// Record a successful task completion and UPDATE scheduling.
    pub(super) fn record_task_completion(
        &mut self,
        task_id: &str,
        duration: Duration,
        errors: u32,
    ) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) else {
            return;
        };

        task.last_run = Some(jiff::Timestamp::now());
        task.run_count += 1;
        task.consecutive_failures = 0;
        task.backoff_until = None;
        task.last_error = None;
        task.last_errors = errors;

        // WHY: apply jitter to the next scheduled run to maintain spread.
        let base_next = task.def.schedule.next_run().unwrap_or(None);
        task.next_run = apply_jitter(base_next, &task.def.id, task.def.jitter).or(base_next);

        crate::metrics::record_cron_execution(&task.def.name, duration.as_secs_f64(), true);
        crate::metrics::record_cron_errors(&task.def.name, errors);

        let result = if errors == 0 { "success" } else { "degraded" };
        tracing::info!(
            task_id = %task.def.id,
            task_name = %task.def.name,
            run_count = task.run_count,
            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            errors,
            result,
            "task completed"
        );

        let state_to_save = snapshot_task_state(task, None, "success");
        self.persist_task_state(&state_to_save);
    }

    /// Record a soft-skip: the task could not run because a dependency was
    /// not configured.
    ///
    /// WHY(#5129): a skip advances `last_run`/`run_count` for observability but
    /// must NOT touch `consecutive_failures` or backoff, since a missing
    /// dependency is a benign no-op rather than an execution failure.
    pub(super) fn record_task_skip(&mut self, task_id: &str) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) else {
            return;
        };

        task.last_run = Some(jiff::Timestamp::now());
        task.run_count += 1;

        // WHY: re-arm the next scheduled run so a skip does not stall the task.
        let base_next = task.def.schedule.next_run().unwrap_or(None);
        task.next_run = apply_jitter(base_next, &task.def.id, task.def.jitter).or(base_next);

        tracing::info!(
            task_id = %task.def.id,
            task_name = %task.def.name,
            run_count = task.run_count,
            result = "skipped",
            "task skipped  -  dependency not configured"
        );

        let backoff_ts = task
            .backoff_until
            .map(|_| jiff::Timestamp::now().to_string());
        let state_to_save = snapshot_task_state(task, backoff_ts, "skipped");
        self.persist_task_state(&state_to_save);
    }

    /// Record a task failure: increment failures, apply backoff, possibly auto-disable.
    pub(super) fn record_task_failure(&mut self, task_id: &str, reason: &str) {
        let safe_reason = redact_task_text(reason);
        let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) else {
            return;
        };

        crate::metrics::record_cron_execution(&task.def.name, 0.0, false);
        task.consecutive_failures += 1;
        task.last_run = Some(jiff::Timestamp::now());
        task.last_error = Some(safe_reason.clone());
        task.last_errors = 0;

        let mut backoff_until_ts: Option<String> = None;

        if task.consecutive_failures >= 3 {
            task.def.enabled = false;
            tracing::warn!(
                task_id = %task.def.id,
                task_name = %task.def.name,
                failures = task.consecutive_failures,
                last_error = %safe_reason,
                "task auto-disabled after 3 consecutive failures"
            );
        } else {
            let delay = backoff_delay(task.consecutive_failures);
            task.backoff_until = Some(Instant::now() + delay);

            let scheduled_next = task.def.schedule.next_run().unwrap_or(None);
            let backoff_ts = jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_nanos(
                    i64::try_from(delay.as_nanos()).unwrap_or(i64::MAX),
                ))
                // kanon:ignore RUST/no-result-unwrap-or-default — timestamp overflow on backoff addition is unreachable for realistic delays; default falls back to epoch, always < scheduled_next
                .unwrap_or_default();
            backoff_until_ts = Some(backoff_ts.to_string());

            task.next_run = match scheduled_next {
                Some(next) if next > backoff_ts => Some(next),
                _ => Some(backoff_ts),
            };

            tracing::warn!(
                task_id = %task.def.id,
                task_name = %task.def.name,
                failures = task.consecutive_failures,
                backoff_secs = delay.as_secs(),
                error = %safe_reason,
                result = "failure",
                "task failed  -  backoff applied"
            );
        }

        let state_to_save = snapshot_task_state(task, backoff_until_ts, "failed");
        self.persist_task_state(&state_to_save);
    }
}
