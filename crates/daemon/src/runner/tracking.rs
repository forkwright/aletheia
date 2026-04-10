//! Task completion and failure tracking: metrics, backoff, auto-disable.

use std::time::{Duration, Instant};

use crate::schedule::{
    BuiltinTask, TaskAction, apply_jitter, backoff_delay,
};

use super::TaskRunner;

impl TaskRunner {
    /// Record a successful task completion and UPDATE scheduling.
    pub(super) fn record_task_completion(&mut self, task_id: &str, duration: Duration) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) else {
            return;
        };

        task.last_run = Some(jiff::Timestamp::now());
        task.run_count += 1;
        task.consecutive_failures = 0;
        task.backoff_until = None;
        task.last_error = None;

        // WHY: apply jitter to the next scheduled run to maintain spread.
        let base_next = task.def.schedule.next_run().unwrap_or(None);
        task.next_run = apply_jitter(base_next, &task.def.id, task.def.jitter).or(base_next);

        crate::metrics::record_cron_execution(&task.def.name, duration.as_secs_f64(), true);

        tracing::info!(
            task_id = %task.def.id,
            task_name = %task.def.name,
            run_count = task.run_count,
            duration_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            result = "success",
            "task completed"
        );

        let state_to_save = crate::state::TaskState {
            task_id: task.def.id.clone(),
            last_run_ts: task.last_run.map(|ts| ts.to_string()),
            run_count: task.run_count,
            consecutive_failures: task.consecutive_failures,
        };
        self.persist_task_state(&state_to_save);
    }

    /// Record a task failure: increment failures, apply backoff, possibly auto-disable.
    pub(super) fn record_task_failure(&mut self, task_id: &str, reason: &str) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.def.id == task_id) else {
            return;
        };

        crate::metrics::record_cron_execution(&task.def.name, 0.0, false);
        task.consecutive_failures += 1;
        task.last_run = Some(jiff::Timestamp::now());
        task.last_error = Some(reason.to_owned());

        // WHY: GraphHealthCheck is a diagnostic: failures don't count toward auto-disable.
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
            let delay = backoff_delay(task.consecutive_failures);
            task.backoff_until = Some(Instant::now() + delay);

            let scheduled_next = task.def.schedule.next_run().unwrap_or(None);
            let backoff_ts = jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_nanos(
                    i64::try_from(delay.as_nanos()).unwrap_or(i64::MAX),
                ))
                .unwrap_or_default();

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
                "task failed  -  backoff applied"
            );
        }

        let state_to_save = crate::state::TaskState {
            task_id: task.def.id.clone(),
            last_run_ts: task.last_run.map(|ts| ts.to_string()),
            run_count: task.run_count,
            consecutive_failures: task.consecutive_failures,
        };
        self.persist_task_state(&state_to_save);
    }
}
