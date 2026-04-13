//! Evidence validation for self-tuning proposals.
//!
//! Basic A/B comparison with significance checking:
//! - Splits observations into before/after halves
//! - Computes mean and standard deviation for each half
//! - Tests whether the delta exceeds `threshold * stddev`
//!
//! No heavy ML — basic statistics only.

/// Result of evidence validation.
#[derive(Debug, Clone)]
pub struct EvidenceResult {
    /// Mean of the first half of observations (baseline).
    pub mean_before: f64,
    /// Mean of the second half of observations (treatment).
    pub mean_after: f64,
    /// Difference: `mean_after - mean_before`.
    pub delta: f64,
    /// Standard deviation of the full observation set.
    pub stddev: f64,
    /// Whether the delta exceeds the significance threshold.
    pub is_significant: bool,
}

/// Validate evidence by splitting observations into before/after halves and
/// checking whether the delta is statistically significant.
///
/// Returns `None` when the input has fewer than 2 values or zero variance
/// (preventing division by zero).
///
/// # Arguments
///
/// * `values` — Ordered metric observations (oldest first).
/// * `significance_threshold` — Delta must exceed this many standard deviations.
#[must_use]
pub fn validate_evidence(
    values: &[f64],
    significance_threshold: f64,
) -> Option<EvidenceResult> {
    if values.len() < 2 {
        return None;
    }

    let midpoint = values.len() / 2;
    // WHY: split_at is bounds-safe; midpoint is values.len()/2 which is
    // >= 1 (len >= 2), so both slices are non-empty and within bounds.
    let (before, after) = values.split_at(midpoint);

    if before.is_empty() || after.is_empty() {
        return None;
    }

    let mean_before = mean(before);
    let mean_after = mean(after);
    let delta = mean_after - mean_before;
    let stddev = standard_deviation(values);

    // Zero variance means all values are identical — no signal.
    if stddev.abs() < f64::EPSILON {
        return None;
    }

    let is_significant = delta.abs() > significance_threshold * stddev;

    Some(EvidenceResult {
        mean_before,
        mean_after,
        delta,
        stddev,
        is_significant,
    })
}

/// Arithmetic mean.
///
/// Returns `0.0` for empty slices.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize->f64: evidence sample counts are bounded by config (tens of samples), far below f64 mantissa precision"
)]
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let n = values.len() as f64; // kanon:ignore RUST/as-cast
    values.iter().sum::<f64>() / n
}

/// Population standard deviation.
///
/// Returns `0.0` for slices with fewer than 2 elements.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "usize->f64: evidence sample counts are bounded by config (tens of samples), far below f64 mantissa precision"
)]
fn standard_deviation(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = mean(values);
    let n = values.len() as f64; // kanon:ignore RUST/as-cast
    let variance = values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / n;
    variance.sqrt()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn mean_of_empty() {
        assert!((mean(&[]) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mean_of_values() {
        assert!((mean(&[1.0, 2.0, 3.0]) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn stddev_of_constant() {
        assert!((standard_deviation(&[5.0, 5.0, 5.0]) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn stddev_of_values() {
        let sd = standard_deviation(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]);
        assert!((sd - 2.0).abs() < 0.01, "stddev should be ~2.0, got {sd}");
    }

    #[test]
    fn validate_insufficient_values() {
        assert!(validate_evidence(&[], 1.5).is_none());
        assert!(validate_evidence(&[1.0], 1.5).is_none());
    }

    #[test]
    fn validate_constant_values_no_signal() {
        assert!(validate_evidence(&[5.0, 5.0, 5.0, 5.0], 1.5).is_none());
    }

    #[test]
    fn validate_significant_delta() {
        // Clear signal: first half low, second half high
        let values = [0.1, 0.1, 0.1, 0.1, 0.1, 0.9, 0.9, 0.9, 0.9, 0.9];
        let result = validate_evidence(&values, 0.5).unwrap();
        assert!(result.is_significant, "large delta should be significant");
        assert!(result.delta > 0.0, "delta should be positive");
        assert!(result.mean_after > result.mean_before);
    }

    #[test]
    fn validate_insignificant_delta() {
        // Very high noise with small difference
        let values = [0.49, 0.51, 0.48, 0.52, 0.50, 0.51, 0.49, 0.50, 0.51, 0.50];
        let result = validate_evidence(&values, 1.5).unwrap();
        assert!(!result.is_significant, "tiny delta should not be significant");
    }

    #[test]
    fn validate_minimum_samples() {
        let values = [1.0, 5.0];
        let result = validate_evidence(&values, 0.5).unwrap();
        assert!(result.delta > 0.0);
    }

    #[test]
    fn validate_negative_delta() {
        // Declining signal
        let values = [0.9, 0.9, 0.9, 0.9, 0.9, 0.1, 0.1, 0.1, 0.1, 0.1];
        let result = validate_evidence(&values, 0.5).unwrap();
        assert!(result.delta < 0.0, "declining signal should have negative delta");
        assert!(result.is_significant);
    }

    #[test]
    fn evidence_min_samples_enforcement() {
        // With only 2 samples but requiring 20 min_samples, the proposer
        // rejects before evidence validation runs. This tests that the
        // evidence validator itself handles the 2-sample case gracefully.
        let result = validate_evidence(&[1.0, 2.0], 1.5);
        assert!(result.is_some(), "2 samples should produce a result");
    }
}
