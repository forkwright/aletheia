//! Outcome signal definitions: how metric names map to descriptions.
//!
//! Each [`OutcomeSignal`] defines a named metric that the self-tuning loop can
//! observe. The registry here maps signal names to human-readable descriptions.
//!
//! NOTE: the `compute` field is currently vestigial. Live evaluation in
//! `TuningProposer` uses `evidence::validate_evidence` to compare before/after
//! halves rather than calling per-signal compute functions.

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
    ///
    /// NOTE: this field is currently vestigial; live evaluation in `TuningProposer`
    /// uses `evidence::validate_evidence` to compare before/after halves instead of
    /// calling the per-signal compute function.
    pub compute: fn(&[MetricSample]) -> Option<f64>,
}

impl OutcomeSignal {
    /// Register an outcome signal.
    #[must_use]
    pub const fn new(
        name: &'static str,
        description: &'static str,
        compute: fn(&[MetricSample]) -> Option<f64>,
    ) -> Self {
        Self {
            name,
            description,
            compute,
        }
    }
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
    OutcomeSignal::new(
        "turn_quality_post_distillation",
        "Mean turn quality of observed samples. \
         Higher values indicate better average turn quality.",
        compute_turn_quality_post_distillation,
    ),
    OutcomeSignal::new(
        "admission_recall_accuracy",
        "Measure precision of the recall admission filter. \
         Higher values indicate fewer irrelevant facts recalled.",
        compute_admission_recall_accuracy,
    ),
    OutcomeSignal::new(
        "competence_trajectory",
        "Slope of the competence score over a rolling window. \
         Positive slope indicates improving agent performance.",
        compute_competence_trajectory,
    ),
];

/// Minimum samples before a turn-quality mean is considered meaningful.
const TURN_QUALITY_MIN_SAMPLES: usize = 5;

/// Minimum samples before a recall-accuracy mean is considered meaningful.
const ADMISSION_RECALL_MIN_SAMPLES: usize = 3;

/// Arithmetic mean of `samples`, or `None` below `min_samples`.
///
/// WHY: the per-signal summaries differ only in how many samples they demand,
/// so the averaging itself lives in one place. A signal that needs different
/// arithmetic gets its own function rather than a flag on this one.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize->f64: sample counts bounded by config (tens of samples), far below f64 mantissa precision"
)]
fn mean_of_samples(samples: &[MetricSample], min_samples: usize) -> Option<f64> {
    if samples.len() < min_samples {
        return None;
    }
    let sum: f64 = samples.iter().map(|s| s.value).sum();
    let n = samples.len() as f64; // kanon:ignore RUST/as-cast
    Some(sum / n)
}

/// Mean turn quality across the observed samples.
///
/// NOTE: this is a plain mean over every sample handed to it — it does not
/// identify distillation-event boundaries and computes no pre/post split. The
/// before/after comparison that justifies a distillation-trigger change is done
/// by `evidence::validate_evidence`, which splits the sample stream in halves;
/// this function only summarises. The signal keeps its distillation-flavoured
/// name because that name is the join key between an emitted metric and the
/// `distillation*Trigger` [`taxis::registry::ParameterSpec`] entries it scores.
fn compute_turn_quality_post_distillation(samples: &[MetricSample]) -> Option<f64> {
    mean_of_samples(samples, TURN_QUALITY_MIN_SAMPLES)
}

/// Mean recall precision across observations.
fn compute_admission_recall_accuracy(samples: &[MetricSample]) -> Option<f64> {
    mean_of_samples(samples, ADMISSION_RECALL_MIN_SAMPLES)
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

    // NOTE: simple linear regression y = a + b*x, where x is the sample index.
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
        let base = jiff::Timestamp::from_second(1_700_000_000).expect("valid timestamp");
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
    fn turn_quality_demands_more_samples_than_admission_recall() {
        // WHY: both signals reduce to a mean over the same helper, so the only
        // thing separating them is the sample floor. Pin the gap here — a
        // dedupe that collapsed the two thresholds would otherwise be silent.
        let four = make_samples(&[0.5, 0.6, 0.7, 0.8]);
        assert!(
            compute_turn_quality_post_distillation(&four).is_none(),
            "turn quality needs 5 samples"
        );
        assert!(
            compute_admission_recall_accuracy(&four).is_some(),
            "admission recall needs only 3 samples"
        );
    }

    #[test]
    fn turn_quality_means_every_sample_not_a_post_distillation_window() {
        // WHY(#5840): the name implies a pre/post-distillation split. It is a
        // plain mean over the whole stream; a windowed implementation would
        // not return the grand mean here.
        let samples = make_samples(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        let result = compute_turn_quality_post_distillation(&samples);
        assert!((result.unwrap() - 0.5).abs() < 0.001);
    }

    #[test]
    fn competence_trajectory_positive_slope() {
        // Linearly increasing: 0.1, 0.2, 0.3, 0.4, 0.5
        let samples = make_samples(&[0.1, 0.2, 0.3, 0.4, 0.5]);
        let slope = compute_competence_trajectory(&samples);
        assert!(
            slope.unwrap() > 0.0,
            "increasing values should have positive slope"
        );
        assert!((slope.unwrap() - 0.1).abs() < 0.001, "slope should be ~0.1");
    }

    #[test]
    fn competence_trajectory_negative_slope() {
        let samples = make_samples(&[0.5, 0.4, 0.3, 0.2, 0.1]);
        let slope = compute_competence_trajectory(&samples);
        assert!(
            slope.unwrap() < 0.0,
            "decreasing values should have negative slope"
        );
    }

    #[test]
    fn competence_trajectory_flat() {
        let samples = make_samples(&[0.5, 0.5, 0.5, 0.5, 0.5]);
        let slope = compute_competence_trajectory(&samples);
        // With constant values, numerator is 0, denominator is non-zero
        assert!(
            (slope.unwrap()).abs() < f64::EPSILON,
            "flat values should have zero slope"
        );
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
        assert_eq!(names.len(), orig_len, "signal names must be unique");
    }

    #[test]
    fn signal_by_name_lookup() {
        assert!(signal_by_name("turn_quality_post_distillation").is_some());
        assert!(signal_by_name("competence_trajectory").is_some());
        assert!(signal_by_name("nonexistent_signal").is_none());
    }
}
