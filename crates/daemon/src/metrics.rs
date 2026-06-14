//! Prometheus metric definitions for the daemon task runner and watchdog.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

// ── Label sets ──

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct WatchdogLabels {
    process_id: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct CronExecutionLabels {
    task_name: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct CronTaskLabels {
    task_name: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct CronErrorLabels {
    task_name: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct BackgroundFailureLabels {
    nous_id: String,
    task_type: String,
}

// ── Metric families ──

static WATCHDOG_RESTARTS_TOTAL: LazyLock<Family<WatchdogLabels, Counter>> =
    LazyLock::new(Family::default);

static WATCHDOG_HUNG_PROCESSES: LazyLock<Gauge> = LazyLock::new(Gauge::default);

static CRON_EXECUTIONS_TOTAL: LazyLock<Family<CronExecutionLabels, Counter>> =
    LazyLock::new(Family::default);

fn cron_duration_histogram() -> Histogram {
    Histogram::new([0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0])
}

type CronTaskHistogramFamily = Family<CronTaskLabels, Histogram, fn() -> Histogram>;

static CRON_DURATION_SECONDS: LazyLock<CronTaskHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(cron_duration_histogram));

static CRON_ERRORS_TOTAL: LazyLock<Family<CronErrorLabels, Counter>> =
    LazyLock::new(Family::default);

static BACKGROUND_TASK_FAILURES_TOTAL: LazyLock<Family<BackgroundFailureLabels, Counter>> =
    LazyLock::new(Family::default);

// WHY: prometheus-client does not expose a public API to iterate a metric
// family's label sets, but `ops_fact_extraction` needs aggregate counter reads
// to build its OpsSnapshot. Shadow counters mirror the family totals and are
// incremented alongside the instrumentation write.
static CRON_EXECUTIONS_OK: AtomicU64 = AtomicU64::new(0);
static CRON_EXECUTIONS_ERROR: AtomicU64 = AtomicU64::new(0);

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_watchdog_restarts",
        "Total watchdog-initiated process restarts",
        WATCHDOG_RESTARTS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_watchdog_hung_processes",
        "Number of processes currently detected as hung",
        WATCHDOG_HUNG_PROCESSES.clone(),
    );
    registry.register(
        "aletheia_cron_executions",
        "Total cron task executions",
        CRON_EXECUTIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_cron_duration_seconds",
        "Cron task execution duration in seconds",
        CRON_DURATION_SECONDS.clone(),
    );
    registry.register(
        "aletheia_cron_errors_total",
        "Non-fatal errors reported by cron task executions",
        CRON_ERRORS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_background_task_failures",
        "Total background task failures (self-prompt, gc, etc.)",
        BACKGROUND_TASK_FAILURES_TOTAL.clone(),
    );
}

// ── Recording ──

/// Record a watchdog process restart.
pub(crate) fn record_watchdog_restart(process_id: &str) {
    WATCHDOG_RESTARTS_TOTAL
        .get_or_create(&WatchdogLabels {
            process_id: process_id.to_owned(),
        })
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
        .get_or_create(&CronExecutionLabels {
            task_name: task_name.to_owned(),
            status: status.to_owned(),
        })
        .inc();
    CRON_DURATION_SECONDS
        .get_or_create(&CronTaskLabels {
            task_name: task_name.to_owned(),
        })
        .observe(duration_secs);
    if success {
        CRON_EXECUTIONS_OK.fetch_add(1, Ordering::Relaxed);
    } else {
        CRON_EXECUTIONS_ERROR.fetch_add(1, Ordering::Relaxed);
    }
}

/// Record non-fatal errors reported by a cron task execution.
///
/// WHY: maintenance tasks such as knowledge graph decay refresh may complete
/// with per-item persistence failures. Counting these separately from hard
/// failures lets operators alert on partial degradation.
pub(crate) fn record_cron_errors(task_name: &str, errors: u32) {
    if errors == 0 {
        return;
    }
    CRON_ERRORS_TOTAL
        .get_or_create(&CronErrorLabels {
            task_name: task_name.to_owned(),
        })
        .inc_by(u64::from(errors));
}

/// Record a background task failure.
///
/// WHY(#2724): background task failures are silent data loss. This counter
/// surfaces the failure rate for alerting.
pub(crate) fn record_background_failure(nous_id: &str, task_type: &str) {
    BACKGROUND_TASK_FAILURES_TOTAL
        .get_or_create(&BackgroundFailureLabels {
            nous_id: nous_id.to_owned(),
            task_type: task_type.to_owned(),
        })
        .inc();
}

// ── Readers ──
//
// WHY: reads come from the shadow counters — see the rationale above
// `CRON_EXECUTIONS_OK`.

/// Total completed cron executions across all tasks and statuses.
pub(crate) fn cron_executions_total() -> u64 {
    CRON_EXECUTIONS_OK
        .load(Ordering::Relaxed)
        .saturating_add(CRON_EXECUTIONS_ERROR.load(Ordering::Relaxed))
}

/// Cron executions that completed successfully.
pub(crate) fn cron_executions_ok() -> u64 {
    CRON_EXECUTIONS_OK.load(Ordering::Relaxed)
}

/// Cron executions that failed.
pub(crate) fn cron_executions_error() -> u64 {
    CRON_EXECUTIONS_ERROR.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use koina::metrics::MetricsRegistry;

    use super::*;

    fn fresh_registry() -> MetricsRegistry {
        let r = MetricsRegistry::new();
        r.with_registry(register);
        r
    }

    fn encode(r: &MetricsRegistry) -> String {
        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        buf
    }

    #[test]
    fn register_and_record_watchdog_restart() {
        let r = fresh_registry();
        record_watchdog_restart("_test_process");
        record_watchdog_restart("_test_process");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_watchdog_restarts_total{process_id=\"_test_process\"} 2"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_set_hung_processes() {
        let r = fresh_registry();
        set_hung_processes(7);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_watchdog_hung_processes 7"),
            "got: {out}"
        );

        set_hung_processes(0);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_watchdog_hung_processes 0"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_cron_execution_success() {
        let r = fresh_registry();
        record_cron_execution("_test_cron_success", 1.5, true);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_cron_executions_total{task_name=\"_test_cron_success\",status=\"ok\"} 1"
            ),
            "got: {out}"
        );
        assert!(
            out.contains(
                "aletheia_cron_duration_seconds_count{task_name=\"_test_cron_success\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_cron_execution_failure() {
        let r = fresh_registry();
        record_cron_execution("_test_cron_failure", 0.5, false);
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_cron_executions_total{task_name=\"_test_cron_failure\",status=\"error\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_background_failure() {
        let r = fresh_registry();
        record_background_failure("_test_nous", "self_prompt");
        record_background_failure("_test_nous", "self_prompt");
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_background_task_failures_total{nous_id=\"_test_nous\",task_type=\"self_prompt\"} 2"
            ),
            "got: {out}"
        );
    }
}
