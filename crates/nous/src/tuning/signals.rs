//! Outcome signal definitions: how metric names map to computable values.
//!
//! Each [`OutcomeSignal`] defines a named metric that the self-tuning loop can
//! observe. The `compute` function accepts a slice of raw metric samples and
//! produces a single summary value (or `None` if insufficient data).

use crate::tuning::MetricSample;

/// An outcome signal that the self-tuning loop can observe and optimise.
pub struct OutcomeSignal {
    /// Signal name (matches `ParameterSpec::outcome_signal`).
    pub name: &'static str,
    /// Human-readable description of what this signal measures.
    pub description: &'static str,
    /// Computation function: takes raw samples, returns a summary value.
    ///
    /// Returns `None` when there are insufficient samples for a meaningful result.
    pub compute: fn(&[MetricSample]) -> Option<f64>,
}

/// Return all registered outcome signals.
#[must_use]
pub fn all_signals() -> &'static [OutcomeSignal] {
    &SIGNALS
}

/// Look up a signal by name.
#[must_use]
pub fn signal_by_name(name: &str) -> Option<&'static OutcomeSignal> {
    SIGNALS.iter().find(|s| s.name == name)
}

static SIGNALS: [OutcomeSignal; 3] = [
    OutcomeSignal {
        name: "turn_quality_post_distillation",
        description: "Compare mean turn quality scores before vs. after distillation events. \
                       Higher values indicate better quality retention through distillation.",
        compute: compute_turn_quality_post_distillation,
    },
    OutcomeSignal {
        name: "admission_recall_accuracy",
        description: "Measure precision of the recall admission filter. \
                       Higher values indicate fewer irrelevant facts recalled.",
        compute: compute_admission_recall_accuracy,
    },
    OutcomeSignal {
        name: "competence_trajectory",
        description: "Slope of the competence score over a rolling window. \
                       Positive slope indicates improving agent performance.",
        compute: compute_competence_trajectory,
    },
];

/// Mean of all sample values, representing average turn quality around
/// distillation events. Requires at least 5 samples.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize->f64: sample counts bounded by config (tens of samples), far below f64 mantissa precision"
)]
fn compute_turn_quality_post_distillation(samples: &[MetricSample]) -> Option<f64> {
    if samples.len() < 5 {
        return None;
    }
    let sum: f64 = samples.iter().map(|s| s.value).sum();
    let n = samples.len() as f64; // kanon:ignore RUST/as-cast
    Some(sum / n)
}

/// Mean recall precision across observations. Requires at least 3 samples.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize->f64: sample counts bounded by config (tens of samples), far below f64 mantissa precision"
)]
fn compute_admission_recall_accuracy(samples: &[MetricSample]) -> Option<f64> {
    if samples.len() < 3 {
        return None;
    }
    let sum: f64 = samples.iter().map(|s| s.value).sum();
    let n = samples.len() as f64; // kanon:ignore RUST/as-cast
    Some(sum / n)
}

/// Linear regression slope of competence scores over time.
///
/// Uses simple least-squares regression on sample indices (equally spaced
/// time points). A positive slope means competence is improving.
///
/// Requires at least 5 samples.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize->f64: sample counts and indices bounded by config (tens of samples), far below f64 mantissa precision"
)]
fn compute_competence_trajectory(samples: &[MetricSample]) -> Option<f64> {
    if samples.len() < 5 {
        return None;
    }

    let n = samples.len() as f64; // kanon:ignore RUST/as-cast
    let values: Vec<f64> = samples.iter().map(|s| s.value).collect();

    // Simple linear regression: y = a + b*x where x is the sample index.
    let x_mean = (n - 1.0) / 2.0;
    let y_mean: f64 = values.iter().sum::<f64>() / n;

    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for (i, &y) in values.iter().enumerate() {
        let x = i as f64; // kanon:ignore RUST/as-cast
        numerator += (x - x_mean) * (y - y_mean);
        denominator += (x - x_mean) * (x - x_mean);
    }

    if denominator.abs() < f64::EPSILON {
        return None;
    }

    Some(numerator / denominator)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions may panic on failure"
)]
mod tests {
    use super::*;

    fn make_samples(values: &[f64]) -> Vec<MetricSample> {
        let base = jiff::Timestamp::from_second(1_700_000_000)
            .expect("valid timestamp");
        values
            .iter()
            .enumerate()
            .map(|(i, &v)| MetricSample {
                metric_name: "test".to_owned(),
                value: v,
                timestamp: base
                    .checked_add(jiff::SignedDuration::from_secs(
                        i64::try_from(i).expect("index fits i64") * 60,
                    ))
                    .expect("valid duration"),
            })
            .collect()
    }

    #[test]
    fn turn_quality_insufficient_samples() {
        let samples = make_samples(&[0.5, 0.6, 0.7]);
        assert!(compute_turn_quality_post_distillation(&samples).is_none());
    }

    #[test]
    fn turn_quality_computes_mean() {
        let samples = make_samples(&[0.5, 0.6, 0.7, 0.8, 0.9]);
        let result = compute_turn_quality_post_distillation(&samples);
        assert!((result.unwrap() - 0.7).abs() < 0.001);
    }

    #[test]
    fn admission_recall_insufficient_samples() {
        let samples = make_samples(&[0.5, 0.6]);
        assert!(compute_admission_recall_accuracy(&samples).is_none());
    }

    #[test]
    fn admission_recall_computes_mean() {
        let samples = make_samples(&[0.8, 0.85, 0.9]);
        let result = compute_admission_recall_accuracy(&samples);
        assert!((result.unwrap() - 0.85).abs() < 0.001);
    }

    #[test]
    fn competence_trajectory_positive_slope() {
        // Linearly increasing: 0.1, 0.2, 0.3, 0.4, 0.5
        let samples = make_samples(&[0.1, 0.2, 0.3, 0.4, 0.5]);
        let slope = compute_competence_trajectory(&samples);
        assert!(slope.unwrap() > 0.0, "increasing values should have positive slope");
        assert!((slope.unwrap() - 0.1).abs() < 0.001, "slope should be ~0.1");
    }

    #[test]
    fn competence_trajectory_negative_slope() {
        let samples = make_samples(&[0.5, 0.4, 0.3, 0.2, 0.1]);
        let slope = compute_competence_trajectory(&samples);
        assert!(slope.unwrap() < 0.0, "decreasing values should have negative slope");
    }

    #[test]
    fn competence_trajectory_flat() {
        let samples = make_samples(&[0.5, 0.5, 0.5, 0.5, 0.5]);
        let slope = compute_competence_trajectory(&samples);
        // With constant values, numerator is 0, denominator is non-zero
        assert!((slope.unwrap()).abs() < f64::EPSILON, "flat values should have zero slope");
    }

    #[test]
    fn competence_trajectory_insufficient_samples() {
        let samples = make_samples(&[0.5, 0.6, 0.7]);
        assert!(compute_competence_trajectory(&samples).is_none());
    }

    #[test]
    fn all_signals_have_unique_names() {
        let signals = all_signals();
        let mut names: Vec<&str> = signals.iter().map(|s| s.name).collect();
        let orig_len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            orig_len,
            "signal names must be unique"
        );
    }

    #[test]
    fn signal_by_name_lookup() {
        assert!(signal_by_name("turn_quality_post_distillation").is_some());
        assert!(signal_by_name("competence_trajectory").is_some());
        assert!(signal_by_name("nonexistent_signal").is_none());
    }
}
