//! Composite comparison reports with full statistical discipline.
//!
//! `comparison_report` is the single entry point for producing a
//! statistically complete comparison between two groups of scores.
//! It mirrors `shared/stats.py::report_comparison` from the quantified-self
//! pipeline, adapted for aletheia's eval context.
//!
//! Every published benchmark comparison **must** include:
//!
//! - Sample sizes and means
//! - 95% bootstrap CI for the mean of group a
//! - Cohen's d effect size with CI and interpretation
//! - Raw p-value (sign test: proportion above 0.5)
//! - FDR-adjusted p-value (when provided)
//!
//! # Usage
//!
//! ```rust,ignore
//! // After running N benchmark questions against baseline and candidate:
//! let report = comparison_report(&baseline_f1s, &candidate_f1s, "candidate vs baseline", None);
//! // Pass report.p_raw to fdr_correct with other comparisons, then set report.p_adjusted.
//! ```

use serde::{Deserialize, Serialize};

use crate::error::{Error, StatsSnafu};

use super::{
    BootstrapCi, CohensD, EffectSizeInterpretation, bootstrap::bootstrap_ci, bootstrap::mean,
    effect_size::cohens_d,
};

/// A fully-specified statistical comparison between two groups.
///
/// Carries every field required for honest published reporting of benchmark
/// results, following the quantified-self pipeline's reporting discipline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    /// Human-readable label for this comparison.
    pub label: String,
    /// Number of observations in group a (e.g. baseline scores).
    pub n_a: usize,
    /// Number of observations in group b (e.g. candidate scores).
    pub n_b: usize,
    /// Mean of group a.
    pub mean_a: f64,
    /// Mean of group b.
    pub mean_b: f64,
    /// 95% bootstrap CI for the mean of group a.
    pub ci_a: BootstrapCi,
    /// 95% bootstrap CI for the mean of group b.
    pub ci_b: BootstrapCi,
    /// Cohen's d effect size (group a minus group b).
    pub effect: CohensD,
    /// Raw p-value from a sign test on per-question score differences.
    ///
    /// The sign test asks: across all questions, is the fraction where
    /// `score_a > score_b` significantly different from 0.5?  Computed as
    /// the proportion of pairs where group a outperforms group b.
    pub p_raw: f64,
    /// FDR-adjusted p-value. `None` until the caller applies [`fdr_correct`] and
    /// sets this field.
    ///
    /// [`fdr_correct`]: crate::stats::fdr_correct
    pub p_adjusted: Option<f64>,
    /// Whether the result is significant at α=0.05 (using raw p).
    pub significant_raw: Option<bool>,
    /// Whether the result is significant at α=0.05 after FDR correction.
    /// `None` until `p_adjusted` is set.
    pub significant_adjusted: Option<bool>,
}

impl ComparisonReport {
    /// Set the FDR-adjusted p-value and recompute significance.
    ///
    /// Call this after running [`fdr_correct`] across multiple comparisons:
    ///
    /// ```rust,ignore
    /// let adj = fdr_correct(&raw_ps, FdrMethod::BenjaminiHochberg).unwrap();
    /// for (report, &p_adj) in reports.iter_mut().zip(&adj) {
    ///     report.set_adjusted_p(p_adj);
    /// }
    /// ```
    ///
    /// [`fdr_correct`]: crate::stats::fdr_correct
    pub fn set_adjusted_p(&mut self, p_adjusted: f64) {
        self.significant_adjusted = Some(p_adjusted < 0.05);
        self.p_adjusted = Some(p_adjusted);
    }

    /// Returns `true` if the effect is statistically significant after FDR
    /// correction, or (if not yet adjusted) by raw p-value.
    #[must_use]
    pub fn is_significant(&self) -> bool {
        self.significant_adjusted
            .or(self.significant_raw)
            .unwrap_or(false)
    }

    /// Returns `true` if the effect size is at least `Small` (|d| ≥ 0.2).
    #[must_use]
    pub fn has_practical_effect(&self) -> bool {
        !matches!(
            self.effect.interpretation,
            EffectSizeInterpretation::Negligible | EffectSizeInterpretation::InsufficientData
        )
    }
}

/// Build a `ComparisonReport` from two score vectors.
///
/// Groups `a` and `b` should contain per-question scores for two variants
/// (e.g. F1 scores for baseline vs candidate). They do not need to be
/// the same length (unpaired design), but a paired sign test is most
/// meaningful when lengths match.
///
/// The sign test p-value is computed as:
/// `p = min(wins_a, wins_b) / total_pairs * 2` (two-tailed approximation),
/// where `wins_a` is the count of indices where `a[i] > b[i]`.
/// If lengths differ, the sign test is skipped and `p_raw = NAN`.
///
/// # Errors
///
/// Returns an error if either slice has fewer than 2 elements.
#[expect(
    clippy::cast_precision_loss,
    reason = "sample sizes are small (<10K); f64 handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — sample sizes are bounded and small"
)]
pub fn comparison_report(
    a: &[f64],
    b: &[f64],
    label: impl Into<String>,
    fdr_adjusted_p: Option<f64>,
) -> Result<ComparisonReport, Error> {
    if a.len() < 2 {
        return Err(StatsSnafu {
            message: format!(
                "comparison_report: group a needs at least 2 observations, got {}",
                a.len()
            ),
        }
        .build());
    }
    if b.len() < 2 {
        return Err(StatsSnafu {
            message: format!(
                "comparison_report: group b needs at least 2 observations, got {}",
                b.len()
            ),
        }
        .build());
    }

    let mean_a = mean(a);
    let mean_b = mean(b);
    let ci_a = bootstrap_ci(a, mean, 2000, 42, 0.95)?;
    let ci_b = bootstrap_ci(b, mean, 2000, 42, 0.95)?;
    let effect = cohens_d(a, b, true, 42);

    // Sign test (paired only)
    let p_raw = if a.len() == b.len() {
        let n = a.len() as f64;
        let wins_a = a.iter().zip(b.iter()).filter(|(ai, bi)| ai > bi).count() as f64;
        // Two-tailed sign test approximation: 2 * min(wins, losses) / n
        let wins_b = n - wins_a;
        let p = 2.0 * wins_a.min(wins_b) / n;
        p.min(1.0)
    } else {
        f64::NAN
    };

    let significant_raw = if p_raw.is_nan() {
        None
    } else {
        Some(p_raw < 0.05)
    };

    let significant_adjusted = fdr_adjusted_p.map(|p| p < 0.05);

    Ok(ComparisonReport {
        label: label.into(),
        n_a: a.len(),
        n_b: b.len(),
        mean_a,
        mean_b,
        ci_a,
        ci_b,
        effect,
        p_raw,
        p_adjusted: fdr_adjusted_p,
        significant_raw,
        significant_adjusted,
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::cast_precision_loss, reason = "test uses small loop indices")]
#[expect(clippy::as_conversions, reason = "test uses small loop indices")]
mod tests {
    use super::*;

    fn arange(start: usize, end: usize) -> Vec<f64> {
        (start..end).map(|i| i as f64).collect()
    }

    #[test]
    fn comparison_report_builds_successfully() {
        let a = arange(0, 20);
        let b = arange(5, 25);
        let report = comparison_report(&a, &b, "test", None).unwrap();
        assert_eq!(report.n_a, 20);
        assert_eq!(report.n_b, 20);
        assert!(report.mean_a < report.mean_b);
        assert!(!report.effect.d.is_nan());
    }

    #[test]
    fn comparison_report_requires_min_two_observations() {
        assert!(comparison_report(&[1.0], &arange(0, 10), "x", None).is_err());
        assert!(comparison_report(&arange(0, 10), &[1.0], "x", None).is_err());
    }

    #[test]
    fn set_adjusted_p_updates_significance() {
        let a = arange(0, 20);
        let b = arange(5, 25);
        let mut report = comparison_report(&a, &b, "test", None).unwrap();
        assert!(report.p_adjusted.is_none());
        report.set_adjusted_p(0.03);
        assert_eq!(report.p_adjusted, Some(0.03));
        assert_eq!(report.significant_adjusted, Some(true));
    }

    #[test]
    fn sign_test_nan_for_unequal_lengths() {
        let a = arange(0, 10);
        let b = arange(0, 15);
        let report = comparison_report(&a, &b, "unequal", None).unwrap();
        assert!(report.p_raw.is_nan());
        assert!(report.significant_raw.is_none());
    }

    #[test]
    fn has_practical_effect_when_large_d() {
        let a: Vec<f64> = vec![10.0; 20];
        let b: Vec<f64> = vec![0.0; 20];
        let report = comparison_report(&a, &b, "large", None).unwrap();
        assert!(report.has_practical_effect());
    }

    #[test]
    fn no_practical_effect_when_identical() {
        let a: Vec<f64> = vec![5.0; 20];
        let report = comparison_report(&a, &a, "identical", None).unwrap();
        assert!(!report.has_practical_effect());
    }
}
