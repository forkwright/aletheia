//! Event loop and task scheduling: the main `run` loop and per-second `tick`.

use std::time::{Duration, Instant};

use tracing::Instrument;

use crate::execution::execute_action;
use crate::schedule::Schedule;

use super::systemd::{sd_notify_watchdog, sd_watchdog_interval};
use super::{InFlightTask, TaskRunner};

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
    /// on the Tokio executor; their `JoinHandle`s are held in `self.in_flight`
    /// and will be abandoned (not awaited) on DROP.
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

        #[cfg(feature = "dispatch-cron")]
        let _cron_handle = self.cron_scheduler.clone().map(|scheduler| {
            let cancel = self.shutdown.child_token();
            tokio::spawn(
                async move {
                    let _ = scheduler
                        .run(cancel, |task| {
                            let name = task.name.clone();
                            let project = task.dispatch_spec.project.clone();
                            async move {
                                tracing::info!(
                                    task = %name,
                                    project = %project,
                                    "cron dispatch task fired"
                                );
                            }
                        })
                        .await;
                }
                .instrument(tracing::info_span!("daemon.cron_scheduler")),
            )
        });

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
                // SAFETY: cancel-safe. `CancellationToken::cancelled()` is cancel-safe;
                // dropping the future before it fires has no side effects.
                () = self.shutdown.cancelled() => {
                    tracing::info!(nous_id = %self.nous_id, "daemon shutting down");
                    break;
                }
            }
        }

        // WHY: abort all in-flight tasks on shutdown to prevent leaked work
        // after the runner exits. Without this, spawned tasks continue running
        // on the Tokio executor with no observer to collect their results.
        let in_flight_count = self.in_flight.len();
        for (task_id, in_flight) in self.in_flight.drain() {
            tracing::debug!(task_id = %task_id, "aborting in-flight task on shutdown");
            in_flight.handle.abort();
        }
        if in_flight_count > 0 {
            tracing::info!(
                nous_id = %self.nous_id,
                cancelled = in_flight_count,
                "in-flight tasks cancelled on shutdown"
            );
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
            let daemon_behavior = self.daemon_behavior.clone();

            let span = tracing::info_span!(
                "task_execute",
                task_id = %task_id,
                task_name = %task_name,
                nous_id = %nous_id,
            );

            let handle = tokio::spawn(
                async move {
                    execute_action(
                        &action,
                        &nous_id,
                        bridge.as_deref(),
                        maintenance.as_ref(),
                        retention_executor,
                        knowledge_executor,
                        &daemon_behavior,
                    )
                    .await
                }
                .instrument(span),
            );

            self.in_flight.insert(
                task_id,
                InFlightTask {
                    handle,
                    started_at: Instant::now(),
                    timeout,
                    warned: false,
                },
            );
        }
    }
}
