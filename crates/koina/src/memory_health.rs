//! Composite memory-health scoring formula.
//!
//! SSOT for the weights: `theatron/proskenion` (client, computed from an
//! already-fetched fact/entity list) and `pylon` (server, computed from a
//! knowledge-store query -- see `crates/pylon/src/metrics.rs`) both call
//! this same function rather than each carrying its own copy of the
//! weights. The two sides can still disagree on the *inputs*
//! (visibility scope, staleness cutoff timing) since they query
//! independently; they cannot disagree on the *formula*.

/// Compute composite memory health score.
///
/// Weights: `avg_confidence` 0.4, `(1 - orphan_ratio)` 0.3,
/// `(1 - staleness_ratio)` 0.3.
#[must_use]
pub fn compute_health_score(avg_confidence: f64, orphan_ratio: f64, staleness_ratio: f64) -> f64 {
    // INVARIANT: All inputs should be 0.0--1.0; clamp defensively.
    let c = avg_confidence.clamp(0.0, 1.0);
    let o = orphan_ratio.clamp(0.0, 1.0);
    let s = staleness_ratio.clamp(0.0, 1.0);

    c * 0.4 + (1.0 - o) * 0.3 + (1.0 - s) * 0.3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_inputs_yield_max_score() {
        assert!((compute_health_score(1.0, 0.0, 0.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn worst_inputs_yield_min_score() {
        assert!(compute_health_score(0.0, 1.0, 1.0).abs() < 1e-9);
    }

    #[test]
    fn weights_sum_to_the_documented_split() {
        // confidence-only vs orphan-only vs staleness-only contribution,
        // isolating each weight -- regression guard against the 0.4/0.3/0.3
        // split silently drifting.
        assert!((compute_health_score(1.0, 1.0, 1.0) - 0.4).abs() < 1e-9);
        assert!((compute_health_score(0.0, 0.0, 1.0) - 0.3).abs() < 1e-9);
        assert!((compute_health_score(0.0, 1.0, 0.0) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn out_of_range_inputs_are_clamped_not_panicking() {
        let score = compute_health_score(2.0, -1.0, 2.0);
        assert!((0.0..=1.0).contains(&score));
    }
}
