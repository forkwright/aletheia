//! Prometheus metric definitions for the session persistence layer.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct SessionLabels {
    session_type: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct BackupStatusLabels {
    status: String,
}

static SESSIONS_TOTAL: LazyLock<Family<SessionLabels, Counter>> = LazyLock::new(Family::default);

fn backup_duration_histogram() -> Histogram {
    Histogram::new([0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0])
}

type BackupDurationFamily = Family<BackupStatusLabels, Histogram, fn() -> Histogram>;

static BACKUP_DURATION_SECONDS: LazyLock<BackupDurationFamily> =
    LazyLock::new(|| Family::new_with_constructor(backup_duration_histogram));

// WHY(#6445): backup freshness is derived from persisted backup state, not from
// this process's event history. The duration histogram above only gains its
// `status="ok"` member after *this* process completes a backup, so a restart —
// or an instance that has never backed up — leaves the series absent and
// `increase(...) == 0` returns an empty vector instead of firing. These gauges
// are set from the manifests on disk, so they survive a restart and are
// present even when the count of successful attempts is zero.
static BACKUP_LAST_SUCCESS_UNIXTIME_SECONDS: LazyLock<Gauge> = LazyLock::new(Gauge::default);
static BACKUP_ENABLED: LazyLock<Gauge> = LazyLock::new(Gauge::default);
static BACKUP_INTERVAL_SECONDS: LazyLock<Gauge> = LazyLock::new(Gauge::default);

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_sessions",
        "Total sessions created",
        SESSIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_backup_duration_seconds",
        "Database backup duration in seconds",
        BACKUP_DURATION_SECONDS.clone(),
    );
    registry.register(
        "aletheia_backup_last_success_unixtime_seconds",
        "Unix timestamp of the newest valid backup manifest on disk; 0 when none exists",
        BACKUP_LAST_SUCCESS_UNIXTIME_SECONDS.clone(),
    );
    registry.register(
        "aletheia_backup_enabled",
        "Whether periodic whole-instance backups are enabled (1) or not (0)",
        BACKUP_ENABLED.clone(),
    );
    registry.register(
        "aletheia_backup_interval_seconds",
        "Configured interval between automatic whole-instance backups in seconds",
        BACKUP_INTERVAL_SECONDS.clone(),
    );
}

/// Record a session creation.
///
/// Compiled for the fjall store backend; called on successful session creation.
/// The `nous_id` parameter is intentionally not recorded as a label — agent/user
/// identifiers must not appear in default Prometheus exports.
pub(crate) fn record_session_created(_nous_id: &str, session_type: &str) {
    SESSIONS_TOTAL
        .get_or_create(&SessionLabels {
            session_type: session_type.to_owned(),
        })
        .inc();
}

/// Record a backup operation duration.
///
/// Called by the daemon's fjall backup task through the binary crate's
/// runtime hook so backup-staleness alerting observes real backup attempts.
pub fn record_backup_duration(duration_secs: f64, success: bool) {
    let status = if success { "ok" } else { "error" };
    BACKUP_DURATION_SECONDS
        .get_or_create(&BackupStatusLabels {
            status: status.to_owned(),
        })
        .observe(duration_secs);
}

/// Publish persisted whole-instance backup state for freshness alerting.
///
/// `last_success_unixtime` is the `created_at` of the newest valid backup
/// manifest on disk, or `None` when no valid backup set exists. `None` is
/// published as `0` rather than by omitting the series: an absent series
/// cannot be distinguished from an absent exporter, which is the defect this
/// replaces. Alerting treats `0` as "backups enabled but none present".
///
/// Called at startup and after each backup attempt, so the value reflects
/// durable recovery state rather than this process's uptime.
pub fn record_backup_state(last_success_unixtime: Option<i64>, enabled: bool, interval_secs: u64) {
    BACKUP_LAST_SUCCESS_UNIXTIME_SECONDS.set(last_success_unixtime.unwrap_or(0));
    BACKUP_ENABLED.set(i64::from(enabled));
    BACKUP_INTERVAL_SECONDS.set(i64::try_from(interval_secs).unwrap_or(i64::MAX));
}

#[cfg(test)]
mod tests {
    use koina::metrics::MetricsRegistry;

    use super::*;

    fn fresh_registry() -> MetricsRegistry {
        koina::metrics::fresh_registry_with(register)
    }

    fn encode(r: &MetricsRegistry) -> String {
        koina::metrics::encode_to_string(r)
    }

    #[test]
    fn register_and_record_session_created() {
        let r = fresh_registry();
        record_session_created("_test_nous_a", "primary");
        record_session_created("_test_nous_b", "primary");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_sessions_total{session_type=\"primary\"} 2"),
            "got: {out}"
        );
        assert!(
            !out.contains("_test_nous"),
            "raw nous_id must not appear in default metrics: {out}"
        );
    }

    #[test]
    fn register_and_record_backup_duration() {
        let r = fresh_registry();
        record_backup_duration(5.0, true);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_backup_duration_seconds_count{status=\"ok\"} 1"),
            "got: {out}"
        );
    }

    // WHY(#6445): the gauges are process-global, so exercising every case in one
    // test keeps concurrent tests from observing each other's writes.
    #[test]
    fn backup_state_is_exported_even_when_no_backup_exists() {
        let r = fresh_registry();

        // NOTE: first boot — backups enabled, nothing on disk. The series
        // must be PRESENT and zero; under the old duration-only rule it was
        // absent, and an absent series makes the alert unfirable.
        record_backup_state(None, true, 24 * 3600);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_backup_last_success_unixtime_seconds 0"),
            "missing zero freshness series: {out}"
        );
        assert!(out.contains("aletheia_backup_enabled 1"), "got: {out}");
        assert!(
            out.contains("aletheia_backup_interval_seconds 86400"),
            "non-default cadence must be exported verbatim: {out}"
        );

        // NOTE: restart after a backup — the timestamp comes from the
        // manifest, so it is published without this process having recorded
        // any attempt.
        record_backup_state(Some(1_800_000_000), true, 6 * 3600);
        let out = encode(&r);
        assert!(
            out.contains("aletheia_backup_last_success_unixtime_seconds 1800000000"),
            "got: {out}"
        );
        assert!(
            out.contains("aletheia_backup_interval_seconds 21600"),
            "got: {out}"
        );

        // NOTE: backups disabled — the alert must be able to suppress itself
        // rather than firing forever on an instance that opted out.
        record_backup_state(None, false, 24 * 3600);
        let out = encode(&r);
        assert!(out.contains("aletheia_backup_enabled 0"), "got: {out}");
    }
}
