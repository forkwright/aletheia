//! Prometheus metric definitions for energeia dispatch orchestration.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;
use std::sync::atomic::AtomicU64;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

// ── Label sets ──

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProjectStatusLabels {
    project: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProjectModelRadiusLabels {
    project: String,
    model: String,
    blast_radius: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProjectLabels {
    project: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProjectVerdictLabels {
    project: String,
    verdict: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ProjectStatusModelFailureLabels {
    project: String,
    status: String,
    model: String,
    failure_class: String,
}

// ── Metric families ──

static DISPATCHES_TOTAL: LazyLock<Family<ProjectStatusLabels, Counter>> =
    LazyLock::new(Family::default);

static SESSIONS_TOTAL: LazyLock<Family<ProjectStatusLabels, Counter>> =
    LazyLock::new(Family::default);

/// Float counter: USD is not integer-valued.
static COST_USD_TOTAL: LazyLock<Family<ProjectModelRadiusLabels, Counter<f64, AtomicU64>>> =
    LazyLock::new(Family::default);

/// Counter for total turns by blast radius and model.
static TURNS_TOTAL: LazyLock<Family<ProjectModelRadiusLabels, Counter>> =
    LazyLock::new(Family::default);

fn session_duration_histogram() -> Histogram {
    // WHY: sessions range from <1 minute (infra failure) to several hours
    // (complex implementation prompts). Buckets cover this full range.
    Histogram::new([
        60.0, 300.0, 900.0, 1_800.0, 3_600.0, 7_200.0, 14_400.0, 28_800.0,
    ])
}

type ProjectHistogramFamily = Family<ProjectLabels, Histogram, fn() -> Histogram>;

static SESSION_DURATION_SECONDS: LazyLock<ProjectHistogramFamily> =
    LazyLock::new(|| Family::new_with_constructor(session_duration_histogram));

static QA_VERDICTS_TOTAL: LazyLock<Family<ProjectVerdictLabels, Counter>> =
    LazyLock::new(Family::default);

static SESSION_FAILURES_TOTAL: LazyLock<Family<ProjectStatusModelFailureLabels, Counter>> =
    LazyLock::new(Family::default);

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "energeia_dispatches",
        "Total dispatch runs completed",
        DISPATCHES_TOTAL.clone(),
    );
    registry.register(
        "energeia_sessions",
        "Total agent sessions dispatched",
        SESSIONS_TOTAL.clone(),
    );
    registry.register(
        "energeia_cost_usd",
        "Cumulative LLM cost in USD by project, model, and blast radius",
        COST_USD_TOTAL.clone(),
    );
    registry.register(
        "energeia_turns",
        "Total LLM turns by project, model, and blast radius",
        TURNS_TOTAL.clone(),
    );
    registry.register(
        "energeia_session_duration_seconds",
        "Agent session wall-clock duration in seconds",
        SESSION_DURATION_SECONDS.clone(),
    );
    registry.register(
        "energeia_qa_verdicts",
        "Total QA evaluation verdicts by project and verdict",
        QA_VERDICTS_TOTAL.clone(),
    );
    registry.register(
        "energeia_session_failures",
        "Total failed agent sessions by project, status, model, and failure class",
        SESSION_FAILURES_TOTAL.clone(),
    );
}

/// Record a completed dispatch run.
///
/// Call once per dispatch when it finishes (Completed or Failed).
pub fn record_dispatch(project: &str, status: &str) {
    DISPATCHES_TOTAL
        .get_or_create(&ProjectStatusLabels {
            project: project.to_owned(),
            status: status.to_owned(),
        })
        .inc();
}

/// Record a completed agent session.
///
/// - `cost_usd` — session cost; silently skipped when zero.
/// - `duration_ms` — wall-clock duration in milliseconds.
/// - `model` — LLM model used (e.g., "claude-3-5-sonnet").
/// - `blast_radius` — blast radius identifier for cost attribution.
pub fn record_session(
    project: &str,
    status: &str,
    cost_usd: f64,
    duration_ms: u64,
    model: &str,
    blast_radius: &str,
) {
    SESSIONS_TOTAL
        .get_or_create(&ProjectStatusLabels {
            project: project.to_owned(),
            status: status.to_owned(),
        })
        .inc();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "u64 ms → f64: realistic session durations (bounded by 1h timeout = 3_600_000 ms) are well below f64 mantissa 2^53"
    )]
    let duration_secs = duration_ms as f64 / 1_000.0; // SAFETY: duration_ms bounded by 1h session timeout
    SESSION_DURATION_SECONDS
        .get_or_create(&ProjectLabels {
            project: project.to_owned(),
        })
        .observe(duration_secs);

    if cost_usd > 0.0 {
        COST_USD_TOTAL
            .get_or_create(&ProjectModelRadiusLabels {
                project: project.to_owned(),
                model: model.to_owned(),
                blast_radius: blast_radius.to_owned(),
            })
            .inc_by(cost_usd);
    }

    // NOTE: turns are tracked separately via record_turns
}

/// Record turns consumed by a session.
///
/// Call this to update the `energeia_turns_total` metric.
pub fn record_turns(project: &str, turns: u32, model: &str, blast_radius: &str) {
    TURNS_TOTAL
        .get_or_create(&ProjectModelRadiusLabels {
            project: project.to_owned(),
            model: model.to_owned(),
            blast_radius: blast_radius.to_owned(),
        })
        .inc_by(u64::from(turns));
}

/// Record a classified failed session for health dashboards.
pub fn record_session_failure(project: &str, status: &str, model: &str, failure_class: &str) {
    SESSION_FAILURES_TOTAL
        .get_or_create(&ProjectStatusModelFailureLabels {
            project: project.to_owned(),
            status: status.to_owned(),
            model: model.to_owned(),
            failure_class: failure_class.to_owned(),
        })
        .inc();
}

/// Record a QA evaluation verdict.
///
/// `verdict` should be one of `"pass"`, `"partial"`, or `"fail"`.
pub fn record_qa_verdict(project: &str, verdict: &str) {
    QA_VERDICTS_TOTAL
        .get_or_create(&ProjectVerdictLabels {
            project: project.to_owned(),
            verdict: verdict.to_owned(),
        })
        .inc();
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
    fn register_and_record_dispatch() {
        let r = fresh_registry();
        record_dispatch("_test_acme", "completed");
        record_dispatch("_test_acme", "completed");
        let out = encode(&r);
        assert!(
            out.contains(
                "energeia_dispatches_total{project=\"_test_acme\",status=\"completed\"} 2"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_session_with_cost() {
        let r = fresh_registry();
        record_session(
            "_test_acme",
            "success",
            0.50,
            30_000,
            "claude-3-5-sonnet",
            "crates/foo/",
        );
        let out = encode(&r);
        assert!(
            out.contains("energeia_sessions_total{project=\"_test_acme\",status=\"success\"} 1"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_session_zero_cost_skips_cost_counter() {
        let r = fresh_registry();
        record_session(
            "_test_nocost",
            "failed",
            0.0,
            5_000,
            "claude-3-5-sonnet",
            "crates/foo/",
        );
        let out = encode(&r);
        // WHY: record_session still records the session count and duration
        // histogram for zero-cost failed runs (useful for debugging infra
        // failures); only the cost counter is skipped.
        assert!(
            !out.contains("energeia_cost_usd_total{project=\"_test_nocost\""),
            "cost sample should be skipped for zero-cost session; got: {out}"
        );
        assert!(
            out.contains("energeia_sessions_total{project=\"_test_nocost\",status=\"failed\"} 1"),
            "expected session counter to still record; got: {out}"
        );
    }

    #[test]
    fn register_and_record_turns() {
        let r = fresh_registry();
        record_turns("_test_turns", 15, "claude-3-5-sonnet", "crates/foo/");
        let out = encode(&r);
        assert!(
            out.contains(
                "energeia_turns_total{project=\"_test_turns\",model=\"claude-3-5-sonnet\",blast_radius=\"crates/foo/\"} 15"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_session_failure_class() {
        let r = fresh_registry();
        record_session_failure(
            "_test_failure_class",
            "infra_failure",
            "claude-3-5-sonnet",
            "auth",
        );
        let out = encode(&r);
        assert!(
            out.contains(
                "energeia_session_failures_total{project=\"_test_failure_class\",status=\"infra_failure\",model=\"claude-3-5-sonnet\",failure_class=\"auth\"} 1"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_qa_verdict() {
        let r = fresh_registry();
        record_qa_verdict("_test_qa", "pass");
        let out = encode(&r);
        assert!(
            out.contains("energeia_qa_verdicts_total{project=\"_test_qa\",verdict=\"pass\"} 1"),
            "got: {out}"
        );
    }
}
