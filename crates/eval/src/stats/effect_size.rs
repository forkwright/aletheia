//! Effect size measures for benchmark comparisons.
//!
//! Implements Cohen's d with pooled SD and optional bootstrap CI.
//! All functions are pure and deterministic given a seed.
//!
//! # Reference
//!
//! Cohen, J. (1988). *Statistical Power Analysis for the Behavioral Sciences*
//! (2nd ed.). Threshold conventions: d < 0.2 negligible, < 0.5 small,
//! < 0.8 medium, ≥ 0.8 large.

use serde::{Deserialize, Serialize};

use super::bootstrap::{BootstrapCi, Lcg64};

/// Interpretation of an effect size magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EffectSizeInterpretation {
    /// |d| < 0.2 (Cohen 1988).
    Negligible,
    /// 0.2 ≤ |d| < 0.5.
    Small,
    /// 0.5 ≤ |d| < 0.8.
    Medium,
    /// |d| ≥ 0.8.
    Large,
    /// Fewer than 2 observations in one group.
    InsufficientData,
}

impl std::fmt::Display for EffectSizeInterpretation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Negligible => write!(f, "negligible"),
            Self::Small => write!(f, "small"),
            Self::Medium => write!(f, "medium"),
            Self::Large => write!(f, "large"),
            Self::InsufficientData => write!(f, "insufficient_data"),
        }
    }
}

/// Result of a Cohen's d computation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CohensD {
    /// The Cohen's d value `(mean_a - mean_b) / pooled_sd`.
    pub d: f64,
    /// 95% bootstrap CI lower bound. `None` when CI was not requested.
    pub ci_low: Option<f64>,
    /// 95% bootstrap CI upper bound. `None` when CI was not requested.
    pub ci_high: Option<f64>,
    /// Verbal interpretation of the magnitude.
    pub interpretation: EffectSizeInterpretation,
}

/// Compute Cohen's d with pooled standard deviation.
///
/// Uses the pooled SD formula:
///
/// ```text
/// pooled_sd = sqrt(((n1-1)*s1² + (n2-1)*s2²) / (n1 + n2 - 2))
/// d = (mean_a - mean_b) / pooled_sd
/// ```
///
/// This assumes equal variances (Cohen 1988, ch. 2). For eval benchmarks
/// comparing a system against a baseline, this is appropriate.
///
/// When `compute_ci` is true, a 95% percentile bootstrap CI for d is
/// computed using 2000 resamples.
///
/// Returns `CohensD` with `d = NAN` and interpretation
/// `InsufficientData` when either slice has fewer than 2 elements.
#[expect(
    clippy::cast_precision_loss,
    reason = "bootstrap counts and indices are small (<100K); f64 handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "bootstrap indices and counts; explicit precision loss accepted"
)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "percentile indices are in [0, n_boot); n_boot is a usize"
)]
#[expect(
    clippy::cast_sign_loss,
    reason = "0.025 * n_boot is non-negative by construction"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "a[idx]/b[idx] use LCG indices in [0, len); boot_ds[lo_idx/hi_idx] are percentile indices within [0, n_boot)"
)]
pub fn cohens_d(a: &[f64], b: &[f64], compute_ci: bool, seed: u64) -> CohensD {
    if a.len() < 2 || b.len() < 2 {
        return CohensD {
            d: f64::NAN,
            ci_low: None,
            ci_high: None,
            interpretation: EffectSizeInterpretation::InsufficientData,
        };
    }

    let d = raw_cohens_d(a, b);
    let interpretation = interpret_d(d);

    if !compute_ci {
        return CohensD {
            d,
            ci_low: None,
            ci_high: None,
            interpretation,
        };
    }

    // Bootstrap CI for d: resample a and b jointly.
    let na = a.len();
    let nb = b.len();
    let n_boot = 2000_usize;
    let mut boot_ds = Vec::with_capacity(n_boot);
    let mut rng = Lcg64::new(seed);
    let mut sa = vec![0.0_f64; na];
    let mut sb = vec![0.0_f64; nb];

    for _ in 0..n_boot {
        for slot in &mut sa {
            let idx = rng.next_bounded_usize(na);
            *slot = a[idx];
        }
        for slot in &mut sb {
            let idx = rng.next_bounded_usize(nb);
            *slot = b[idx];
        }
        boot_ds.push(raw_cohens_d(&sa, &sb));
    }

    boot_ds.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));

    let lo_idx = (0.025 * n_boot as f64) as usize;
    let hi_idx = (0.975 * n_boot as f64).min((n_boot - 1) as f64) as usize;

    CohensD {
        d,
        ci_low: Some(boot_ds[lo_idx]),
        ci_high: Some(boot_ds[hi_idx]),
        interpretation,
    }
}

/// Compute Cohen's d without CI (cheap path for quick comparisons).
#[expect(
    clippy::cast_precision_loss,
    reason = "sample sizes are small (<10K); f64 handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — sample sizes are bounded and small"
)]
pub(super) fn raw_cohens_d(a: &[f64], b: &[f64]) -> f64 {
    let na = a.len();
    let nb = b.len();
    let mean_a = a.iter().sum::<f64>() / na as f64;
    let mean_b = b.iter().sum::<f64>() / nb as f64;
    let var_a = a.iter().map(|x| (x - mean_a).powi(2)).sum::<f64>() / (na - 1) as f64;
    let var_b = b.iter().map(|x| (x - mean_b).powi(2)).sum::<f64>() / (nb - 1) as f64;
    let pooled_sd =
        (((na - 1) as f64 * var_a + (nb - 1) as f64 * var_b) / (na + nb - 2) as f64).sqrt();
    if pooled_sd == 0.0 {
        // WHY: when both groups have zero variance, Cohen's d is undefined in
        // the pooled-SD form. If the means also coincide, return 0 (no effect);
        // otherwise return ±∞ — the groups are perfectly separated with no
        // overlap, which corresponds to an effectively unbounded effect.
        let diff = mean_a - mean_b;
        if diff == 0.0 {
            0.0
        } else {
            diff.signum() * f64::INFINITY
        }
    } else {
        (mean_a - mean_b) / pooled_sd
    }
}

fn interpret_d(d: f64) -> EffectSizeInterpretation {
    let ad = d.abs();
    if ad < 0.2 {
        EffectSizeInterpretation::Negligible
    } else if ad < 0.5 {
        EffectSizeInterpretation::Small
    } else if ad < 0.8 {
        EffectSizeInterpretation::Medium
    } else {
        EffectSizeInterpretation::Large
    }
}

/// A convenience wrapper around `BootstrapCi` that also includes the effect
/// size, making the combined stat easier to pass through the reporting layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectWithCi {
    /// The Cohen's d effect size computation.
    pub effect: CohensD,
    /// The mean of group a.
    pub mean_a: f64,
    /// The mean of group b.
    pub mean_b: f64,
    /// Sample sizes.
    pub n_a: usize,
    /// Sample size of group b.
    pub n_b: usize,
    /// Bootstrap CI for the mean of group a.
    pub ci_a: BootstrapCi,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn cohens_d_identical_groups_is_zero() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = cohens_d(&a, &a, false, 42);
        assert!((result.d).abs() < f64::EPSILON);
        assert_eq!(result.interpretation, EffectSizeInterpretation::Negligible);
    }

    #[test]
    fn cohens_d_well_separated_groups() {
        // a: all 10s, b: all 0s — d should be large
        let a: Vec<f64> = vec![10.0; 20];
        let b: Vec<f64> = vec![0.0; 20];
        let result = cohens_d(&a, &b, false, 42);
        assert!(result.d > 5.0, "d should be very large: {}", result.d);
        assert_eq!(result.interpretation, EffectSizeInterpretation::Large);
    }

    #[test]
    fn cohens_d_with_ci_brackets_point() {
        let a: Vec<f64> = (0..30).map(f64::from).collect();
        let b: Vec<f64> = (5..35).map(f64::from).collect();
        let result = cohens_d(&a, &b, true, 42);
        let lo = result.ci_low.unwrap();
        let hi = result.ci_high.unwrap();
        assert!(lo < hi, "CI must be ordered");
    }

    #[test]
    fn cohens_d_insufficient_data_returns_nan() {
        let result = cohens_d(&[1.0], &[1.0, 2.0], false, 42);
        assert!(result.d.is_nan());
        assert_eq!(
            result.interpretation,
            EffectSizeInterpretation::InsufficientData
        );
    }

    #[test]
    fn interpretation_thresholds() {
        assert_eq!(interpret_d(0.1), EffectSizeInterpretation::Negligible);
        assert_eq!(interpret_d(0.3), EffectSizeInterpretation::Small);
        assert_eq!(interpret_d(0.6), EffectSizeInterpretation::Medium);
        assert_eq!(interpret_d(1.0), EffectSizeInterpretation::Large);
        // Negative d uses absolute value
        assert_eq!(interpret_d(-0.6), EffectSizeInterpretation::Medium);
    }
}
