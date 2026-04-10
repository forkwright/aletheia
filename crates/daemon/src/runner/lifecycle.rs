//! Event loop and task scheduling: the main `run` loop and per-second `tick`.

use std::time::{Duration, Instant};

use tracing::Instrument;

use crate::execution::execute_action;
use crate::schedule::Schedule;

use super::{InFlightTask, TaskRunner};
use super::systemd::{sd_notify_ready, sd_notify_stopping, sd_notify_watchdog, sd_watchdog_interval};

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

        // WHY: send READY=1 to systemd after initialization is complete.
        sd_notify_ready();

        let mut interval = tokio::time::interval(Duration::from_secs(1));

        // WHY: watchdog interval for systemd WatchdogSec integration.
        let watchdog_interval = sd_watchdog_interval();
        let mut watchdog_tick =
            tokio::time::interval(watchdog_interval.unwrap_or(Duration::from_secs(30)));

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

        // WHY: send STOPPING=1 to systemd before cleanup.
        sd_notify_stopping();

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

        // i is always in 0..self.tasks.len() so all self.tasks[i] accesses are valid
        #[expect(
            clippy::indexing_slicing,
            reason = "i ∈ 0..self.tasks.len() by for-loop bounds"
        )]
        for i in 0..self.tasks.len() {
            if !self.tasks[i].def.enabled {
                continue;
            }

            let Some(next) = self.tasks[i].next_run else {
                continue;
            };

            if next > now {
                continue;
            }

            if !Schedule::in_window(self.tasks[i].def.active_window) {
                continue;
            }

            // WHY: skip tasks still in-flight to prevent overlapping executions.
            if self.in_flight.contains_key(&self.tasks[i].def.id) {
                tracing::debug!(
                    task_id = %self.tasks[i].def.id,
                    "skipping  -  previous execution still in progress"
                );
                continue;
            }

            if let Some(backoff_until) = self.tasks[i].backoff_until
                && now_instant < backoff_until
            {
                tracing::debug!(
                    task_id = %self.tasks[i].def.id,
                    remaining_secs = (backoff_until - now_instant).as_secs(),
                    "skipping  -  in backoff period"
                );
                continue;
            }

            // WHY(#2212): record last_run BEFORE spawning the task so that a crash
            // during execution does not leave last_run stale, which would cause
            // the scheduler to re-execute the task immediately on recovery.
            self.tasks[i].last_run = Some(jiff::Timestamp::now());

            let action = self.tasks[i].def.action.clone();
            let nous_id = self.tasks[i].def.nous_id.clone();
            let task_id = self.tasks[i].def.id.clone();
            let timeout = self.tasks[i].def.timeout;

            let bridge = self.bridge.clone();
            let maintenance = self.maintenance.clone();
            let retention_executor = self.retention_executor.clone();
            let knowledge_executor = self.knowledge_executor.clone();

            let span = tracing::info_span!(
                "task_execute",
                task_id = %task_id,
                task_name = %self.tasks[i].def.name,
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
