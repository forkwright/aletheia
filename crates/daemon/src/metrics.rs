//! Prometheus metric definitions for the daemon task runner and watchdog.

#![expect(
    clippy::expect_used,
    reason = "metric registration is infallible at startup"
)]

use std::sync::LazyLock;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, register_histogram_vec,
    register_int_counter_vec, register_int_gauge,
};

static WATCHDOG_RESTARTS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_watchdog_restarts_total",
            "Total watchdog-initiated process restarts"
        ),
        &["process_id"]
    )
    .expect("metric registration")
});

static WATCHDOG_HUNG_PROCESSES: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "aletheia_watchdog_hung_processes",
        "Number of processes currently detected as hung"
    )
    .expect("metric registration")
});

static CRON_EXECUTIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_cron_executions_total",
            "Total cron task executions"
        ),
        &["task_name", "status"]
    )
    .expect("metric registration")
});

static CRON_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HistogramOpts::new(
            "aletheia_cron_duration_seconds",
            "Cron task execution duration in seconds"
        )
        .buckets(vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0]),
        &["task_name"]
    )
    .expect("metric registration")
});

/// Force-initialize all lazy metric statics.
pub fn init() {
    // kanon:ignore RUST/pub-visibility
    LazyLock::force(&WATCHDOG_RESTARTS_TOTAL);
    LazyLock::force(&WATCHDOG_HUNG_PROCESSES);
    LazyLock::force(&CRON_EXECUTIONS_TOTAL);
    LazyLock::force(&CRON_DURATION_SECONDS);
}

/// Record a watchdog process restart.
pub(crate) fn record_watchdog_restart(process_id: &str) {
    WATCHDOG_RESTARTS_TOTAL
        .with_label_values(&[process_id])
        .inc();
}

/// Set the current number of hung processes.
pub(crate) fn set_hung_processes(count: i64) {
    WATCHDOG_HUNG_PROCESSES.set(count);
}

/// Record a completed cron task execution.
pub(crate) fn record_cron_execution(task_name: &str, duration_secs: f64, success: bool) {
    let status = if success { "ok" } else { "error" };
    CRON_EXECUTIONS_TOTAL
        .with_label_values(&[task_name, status])
        .inc();
    CRON_DURATION_SECONDS
        .with_label_values(&[task_name])
        .observe(duration_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_does_not_panic() {
        init();
    }

    #[test]
    fn record_watchdog_restart_does_not_panic() {
        record_watchdog_restart("nous-actor");
    }

    #[test]
    fn set_hung_processes_does_not_panic() {
        set_hung_processes(2);
        set_hung_processes(0);
    }

    #[test]
    fn record_cron_execution_does_not_panic() {
        record_cron_execution("evolution", 1.5, true);
        record_cron_execution("reflection", 0.5, false);
    }
}
