//! Event loop and task scheduling: the main `run` loop and per-second `tick`.

use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::execution::{ExecutionContext, execute_action_with_cancel};
use crate::schedule::Schedule;

use super::systemd::{sd_notify_watchdog, sd_watchdog_interval};
use super::{InFlightTask, TaskRunner};

// WHY: maximum time the shutdown drain waits for cooperating tasks to observe
// their cancellation token and self-terminate before force-aborting. Tasks
// that finish within this window do so cleanly (flushing state, recording
// results). Tasks that exceed the window are force-aborted to bound teardown.
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(5);

impl TaskRunner {
    /// Run the event loop. Checks for due tasks every second, executes them.
    /// Returns when the shutdown token is cancelled.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe at the loop boundary. Each `select!` branch is cancel-safe:
    /// `interval.tick()` is cancel-safe (a dropped tick simply delays the next
    /// poll), and `CancellationToken::cancelled()` is cancel-safe. If this
    /// future is dropped between iterations, in-flight tasks continue running
    /// on the Tokio executor; the normal shutdown path below aborts and awaits
    /// them so that teardown does not outlive the work they are performing.
    #[tracing::instrument(skip_all)]
    pub async fn run(&mut self) {
        tracing::info!(nous_id = %self.nous_id, tasks = self.tasks.len(), "daemon started");

        self.restore_state();

        self.check_missed_cron_catchup();

        let mut interval = tokio::time::interval(Duration::from_secs(1));

        // WHY: watchdog interval for systemd WatchdogSec integration.
        let watchdog_interval = sd_watchdog_interval();
        let mut watchdog_tick =
            tokio::time::interval(watchdog_interval.unwrap_or(Duration::from_secs(30)));
        let process_watchdog_interval = self.process_watchdog_interval();
        let mut process_watchdog_tick =
            tokio::time::interval(process_watchdog_interval.unwrap_or(Duration::from_secs(30)));

        loop {
            tokio::select! {
                // SAFETY: cancel-safe. `interval.tick()` is cancel-safe; dropping it
                // before it fires simply delays the next tick without losing state.
                // `check_in_flight` polls already-spawned handles and does not
                // mutate scheduler state if cancelled mid-loop.
                _ = interval.tick() => {
                    self.check_in_flight().await;
                    self.tick();
                }
                // WHY: send WATCHDOG=1 on the systemd watchdog interval so
                // WatchdogSec integration enables automatic restart on hang.
                _ = watchdog_tick.tick(), if watchdog_interval.is_some() => {
                    sd_notify_watchdog();
                }
                _ = process_watchdog_tick.tick(), if process_watchdog_interval.is_some() => {
                    self.check_task_watchdog().await;
                }
                // SAFETY: cancel-safe. `CancellationToken::cancelled()` is cancel-safe;
                // dropping the future before it fires has no side effects.
                () = self.shutdown.cancelled() => {
                    tracing::info!(nous_id = %self.nous_id, "daemon shutting down");
                    break;
                }
            }
        }

        // WHY: cancel all in-flight tasks and drain them with a bounded grace
        // window before the runner returns. Cooperating tasks observe their
        // cancellation token and self-terminate cleanly within the window,
        // preserving their cleanup bodies (flush state, record results). Tasks
        // that do not cooperate within the grace window are force-aborted to
        // bound teardown time. Aborting immediately (before the grace window)
        // would prevent cooperating tasks from running their cleanup body,
        // violating the graceful-drain contract encoded in the shutdown path.
        let in_flight_count = self.in_flight.len();
        let drained: Vec<_> = self.in_flight.drain().collect();

        // Step 1: signal cancellation to all in-flight tasks simultaneously
        // so they all begin winding down before we start awaiting any of them.
        for (task_id, in_flight) in &drained {
            tracing::debug!(
                task_id = %task_id,
                "cancelling in-flight task on shutdown"
            );
            in_flight.cancel.cancel();
            self.unregister_watchdog_process(task_id);
        }
        if in_flight_count > 0 {
            tracing::info!(
                nous_id = %self.nous_id,
                cancelled = in_flight_count,
                "in-flight tasks cancelled on shutdown; awaiting graceful drain"
            );
        }

        // Step 2: await each handle within the remaining grace window.
        // Force-abort any that do not finish in time, then await those too.
        //
        // NOTE: all cancellation tokens were fired above before this loop so
        // all tasks are already winding down concurrently; sequential polling
        // here does not serialize their execution.
        let grace_deadline = Instant::now() + SHUTDOWN_GRACE_PERIOD;
        for (task_id, in_flight) in drained {
            let mut handle = in_flight.handle;
            let remaining = grace_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // NOTE: abort() is forcible — it preempts the task at its next
                // await point. We reach this path only when grace has already
                // been exhausted by prior tasks.
                tracing::warn!(
                    task_id = %task_id,
                    "grace period exhausted; force-aborting in-flight task"
                );
                handle.abort();
                let _ = handle.await;
                continue;
            }
            tokio::select! {
                biased;
                _ = &mut handle => {
                    tracing::debug!(
                        task_id = %task_id,
                        "in-flight task completed gracefully on shutdown"
                    );
                }
                () = tokio::time::sleep(remaining) => {
                    // NOTE: abort() is forcible — it preempts the task at its
                    // next await point. We reach this path only when the task
                    // has not cooperated within SHUTDOWN_GRACE_PERIOD.
                    tracing::warn!(
                        task_id = %task_id,
                        "grace period exceeded; force-aborting in-flight task"
                    );
                    handle.abort();
                    let _ = handle.await;
                }
            }
        }

        // WHY: drain tracked self-prompt tasks so panics surface as JoinErrors
        // instead of being silently dropped when the runtime shuts down.
        self.self_prompt_tasks.abort_all();
        while let Some(result) = self.self_prompt_tasks.join_next().await {
            match result {
                Ok(()) => {
                    tracing::debug!(
                        nous_id = %self.nous_id,
                        "self-prompt task completed on shutdown"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        nous_id = %self.nous_id,
                        error = %e,
                        "self-prompt task panicked or was cancelled on shutdown"
                    );
                }
            }
        }
    }

    pub(super) fn tick(&mut self) {
        let now = jiff::Timestamp::now();
        let now_instant = Instant::now();

        for i in 0..self.tasks.len() {
            // WHY: `get_mut` over the length we just read keeps the indexing
            // fallible; `continue` on None leaves the loop lenient to concurrent
            // resize (not possible here, but avoids the `[i]` suppression).
            let Some(task) = self.tasks.get_mut(i) else {
                continue;
            };

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

            let task_def_id = task.def.id.clone();
            let backoff_until = task.backoff_until;

            // WHY: skip tasks still in-flight to prevent overlapping executions.
            if self.in_flight.contains_key(&task_def_id) {
                tracing::debug!(
                    task_id = %task_def_id,
                    "skipping  -  previous execution still in progress"
                );
                continue;
            }

            if let Some(backoff_until) = backoff_until
                && now_instant < backoff_until
            {
                tracing::debug!(
                    task_id = %task_def_id,
                    remaining_secs = (backoff_until - now_instant).as_secs(),
                    "skipping  -  in backoff period"
                );
                continue;
            }

            // Re-borrow mutably to record last_run and collect the spawn inputs.
            let Some(task) = self.tasks.get_mut(i) else {
                continue;
            };

            // WHY(#2212): record last_run BEFORE spawning the task so that a crash
            // during execution does not leave last_run stale, which would cause
            // the scheduler to re-execute the task immediately on recovery.
            task.last_run = Some(jiff::Timestamp::now());

            let action = task.def.action.clone();
            let nous_id = task.def.nous_id.clone();
            let task_id = task.def.id.clone();
            let timeout = task.def.timeout;
            let task_name = task.def.name.clone();

            let bridge = self.bridge.clone();
            let maintenance = self.maintenance.clone();
            let retention_executor = self.retention_executor.clone();
            let knowledge_executor = self.knowledge_executor.clone();
            #[cfg(feature = "knowledge-store")]
            let knowledge_store = self.knowledge_store.clone();
            let daemon_behavior = self.daemon_behavior.clone();

            let span = tracing::info_span!(
                "task_execute",
                task_id = %task_id,
                task_name = %task_name,
                nous_id = %nous_id,
            );

            let task_cancel = CancellationToken::new();
            let task_cancel_child = task_cancel.child_token();

            let handle = tokio::spawn(
                async move {
                    execute_action_with_cancel(
                        &action,
                        ExecutionContext {
                            nous_id: &nous_id,
                            bridge: bridge.as_deref(),
                            maintenance: maintenance.as_ref(),
                            retention_executor,
                            knowledge_executor,
                            #[cfg(feature = "knowledge-store")]
                            knowledge_store,
                            daemon_behavior: &daemon_behavior,
                            cancel: task_cancel_child,
                            timeout,
                        },
                    )
                    .await
                }
                .instrument(span),
            );

            self.in_flight.insert(
                task_id.clone(),
                InFlightTask {
                    handle,
                    cancel: task_cancel,
                    started_at: Instant::now(),
                    timeout,
                    warned: false,
                },
            );
            self.register_watchdog_process(&task_id);
        }
    }
}
