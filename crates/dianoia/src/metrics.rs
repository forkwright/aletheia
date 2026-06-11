//! Prometheus metric definitions for planning and project orchestration.
//!
//! Metrics are registered against a shared [`koina::metrics::MetricsRegistry`]
//! via [`register`]. Recording functions operate on global `LazyLock` families
//! that share `Arc`-internal state with the registered copies.

use std::sync::LazyLock;

use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::registry::Registry;

// ── Label sets ──

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct PhaseTransitionLabels {
    from: String,
    to: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct StuckPatternLabels {
    pattern: String,
}

// ── Metric families ──

static PHASE_TRANSITIONS_TOTAL: LazyLock<Family<PhaseTransitionLabels, Counter>> =
    LazyLock::new(Family::default);

static STUCK_DETECTIONS_TOTAL: LazyLock<Family<StuckPatternLabels, Counter>> =
    LazyLock::new(Family::default);

/// Register this crate's metrics with the shared registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "aletheia_phase_transitions",
        "Total project state transitions",
        PHASE_TRANSITIONS_TOTAL.clone(),
    );
    registry.register(
        "aletheia_stuck_detections",
        "Total stuck pattern detections",
        STUCK_DETECTIONS_TOTAL.clone(),
    );
}

/// Force-initialize all lazy metric statics.
///
/// Primarily a compatibility shim for the binary crate's startup; prefer
/// [`register`] which installs the families into a shared registry.
pub fn init() {
    LazyLock::force(&PHASE_TRANSITIONS_TOTAL);
    LazyLock::force(&STUCK_DETECTIONS_TOTAL);
}

// ── Recording ──

/// Record a project state transition.
pub(crate) fn record_phase_transition(from: &str, to: &str) {
    PHASE_TRANSITIONS_TOTAL
        .get_or_create(&PhaseTransitionLabels {
            from: from.to_owned(),
            to: to.to_owned(),
        })
        .inc();
}

/// Record a stuck pattern detection.
pub(crate) fn record_stuck_detection(pattern: &str) {
    STUCK_DETECTIONS_TOTAL
        .get_or_create(&StuckPatternLabels {
            pattern: pattern.to_owned(),
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
    fn init_initializes_metrics() {
        init();
        record_phase_transition("planning", "executing");
    }

    #[test]
    fn register_and_record_phase_transition() {
        let r = fresh_registry();
        record_phase_transition("_test_from", "_test_to");
        record_phase_transition("_test_from", "_test_to");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_phase_transitions_total{from=\"_test_from\",to=\"_test_to\"} 2"),
            "got: {out}"
        );
    }

    #[test]
    fn register_and_record_stuck_detection() {
        let r = fresh_registry();
        record_stuck_detection("_test_repeated_error");
        let out = encode(&r);
        assert!(
            out.contains("aletheia_stuck_detections_total{pattern=\"_test_repeated_error\"} 1"),
            "got: {out}"
        );
    }
}
