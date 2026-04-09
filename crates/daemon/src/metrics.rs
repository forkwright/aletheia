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

static BACKGROUND_TASK_FAILURES_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        Opts::new(
            "aletheia_background_task_failures_total",
            "Total background task failures (self-prompt, gc, etc.)"
        ),
        &["nous_id", "task_type"]
    )
    .expect("metric registration")
});

#[expect(dead_code, reason = "metric init called from server startup")]
/// Force-initialize all lazy metric statics.
pub(crate) fn init() {
    LazyLock::force(&WATCHDOG_RESTARTS_TOTAL);
    LazyLock::force(&WATCHDOG_HUNG_PROCESSES);
    LazyLock::force(&CRON_EXECUTIONS_TOTAL);
    LazyLock::force(&CRON_DURATION_SECONDS);
    LazyLock::force(&BACKGROUND_TASK_FAILURES_TOTAL);
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

/// Record a background task failure.
///
/// WHY: Background task failures are silent data loss. This counter
/// surfaces the failure rate for alerting. Closes #2724.
pub(crate) fn record_background_failure(nous_id: &str, task_type: &str) {
    BACKGROUND_TASK_FAILURES_TOTAL
        .with_label_values(&[nous_id, task_type])
        .inc();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_registers_all_metrics() {
        init();
        // Verify metrics are registered by accessing them
        let _ = WATCHDOG_RESTARTS_TOTAL.with_label_values(&["test"]).get();
        let _ = WATCHDOG_HUNG_PROCESSES.get();
        let _ = CRON_EXECUTIONS_TOTAL.with_label_values(&["test", "ok"]).get();
        let _ = CRON_DURATION_SECONDS.with_label_values(&["test"]).get_sample_count();
        let _ = BACKGROUND_TASK_FAILURES_TOTAL.with_label_values(&["test", "self_prompt"]).get();
    }

    #[test]
    fn record_watchdog_restart_increments_counter() {
        let process_id = "test-restart-process";
        let before = WATCHDOG_RESTARTS_TOTAL.with_label_values(&[process_id]).get();

        record_watchdog_restart(process_id);
        assert_eq!(
            WATCHDOG_RESTARTS_TOTAL.with_label_values(&[process_id]).get(),
            before + 1,
            "restart counter should increment by 1"
        );

        record_watchdog_restart(process_id);
        assert_eq!(
            WATCHDOG_RESTARTS_TOTAL.with_label_values(&[process_id]).get(),
            before + 2,
            "restart counter should be cumulative"
        );
    }

    #[test]
    fn set_hung_processes_updates_gauge() {
        // Test setting various values
        set_hung_processes(5);
        assert_eq!(
            WATCHDOG_HUNG_PROCESSES.get(),
            5,
            "gauge should be set to 5"
        );

        set_hung_processes(3);
        assert_eq!(
            WATCHDOG_HUNG_PROCESSES.get(),
            3,
            "gauge should be updated to 3"
        );

        set_hung_processes(0);
        assert_eq!(
            WATCHDOG_HUNG_PROCESSES.get(),
            0,
            "gauge should be resettable to 0"
        );
    }

    #[test]
    fn record_cron_execution_records_success_and_failure() {
        let task_name = "test-cron-task";
        let ok_before = CRON_EXECUTIONS_TOTAL.with_label_values(&[task_name, "ok"]).get();
        let error_before = CRON_EXECUTIONS_TOTAL.with_label_values(&[task_name, "error"]).get();
        let hist_before = CRON_DURATION_SECONDS
            .with_label_values(&[task_name])
            .get_sample_count();

        // Record successful execution
        record_cron_execution(task_name, 1.5, true);
        assert_eq!(
            CRON_EXECUTIONS_TOTAL.with_label_values(&[task_name, "ok"]).get(),
            ok_before + 1,
            "ok counter should increment for success=true"
        );
        assert_eq!(
            CRON_EXECUTIONS_TOTAL.with_label_values(&[task_name, "error"]).get(),
            error_before,
            "error counter should not change for success=true"
        );

        // Record failed execution
        record_cron_execution(task_name, 0.5, false);
        assert_eq!(
            CRON_EXECUTIONS_TOTAL.with_label_values(&[task_name, "ok"]).get(),
            ok_before + 1,
            "ok counter should be unchanged after error"
        );
        assert_eq!(
            CRON_EXECUTIONS_TOTAL.with_label_values(&[task_name, "error"]).get(),
            error_before + 1,
            "error counter should increment for success=false"
        );

        // Verify histogram has 2 samples
        assert_eq!(
            CRON_DURATION_SECONDS
                .with_label_values(&[task_name])
                .get_sample_count(),
            hist_before + 2,
            "histogram should have 2 samples"
        );
    }

    #[test]
    fn record_background_failure_increments_counter() {
        let nous_id = "test-nous";
        let task_type = "self_prompt";
        let before = BACKGROUND_TASK_FAILURES_TOTAL
            .with_label_values(&[nous_id, task_type])
            .get();

        record_background_failure(nous_id, task_type);
        assert_eq!(
            BACKGROUND_TASK_FAILURES_TOTAL
                .with_label_values(&[nous_id, task_type])
                .get(),
            before + 1,
            "failure counter should increment by 1"
        );

        record_background_failure(nous_id, task_type);
        assert_eq!(
            BACKGROUND_TASK_FAILURES_TOTAL
                .with_label_values(&[nous_id, task_type])
                .get(),
            before + 2,
            "failure counter should be cumulative"
        );
    }

    #[test]
    fn record_background_failure_different_task_types() {
        let nous_id = "test-nous";
        let before_prompt = BACKGROUND_TASK_FAILURES_TOTAL
            .with_label_values(&[nous_id, "self_prompt"])
            .get();
        let before_gc = BACKGROUND_TASK_FAILURES_TOTAL
            .with_label_values(&[nous_id, "gc"])
            .get();

        record_background_failure(nous_id, "self_prompt");
        record_background_failure(nous_id, "gc");

        assert_eq!(
            BACKGROUND_TASK_FAILURES_TOTAL
                .with_label_values(&[nous_id, "self_prompt"])
                .get(),
            before_prompt + 1,
            "self_prompt counter should increment"
        );
        assert_eq!(
            BACKGROUND_TASK_FAILURES_TOTAL
                .with_label_values(&[nous_id, "gc"])
                .get(),
            before_gc + 1,
            "gc counter should increment"
        );
    }
}
