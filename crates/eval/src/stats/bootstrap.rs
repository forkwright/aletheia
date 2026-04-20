//! Bootstrap confidence intervals.
//!
//! Provides i.i.d. percentile bootstrap and block bootstrap for autocorrelated
//! time series. Both are pure functions keyed by a seed for reproducibility.
//!
//! # References
//!
//! - Efron, B. & Hastie, T. (2021). *Computer Age Statistical Inference* (2nd ed.)
//! - Daza, E.J. (2018). Causal analysis of self-tracked time series data.

use serde::{Deserialize, Serialize};

use crate::error::{Error, StatsSnafu};

/// A bootstrap confidence interval result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootstrapCi {
    /// Point estimate of the statistic on the original data.
    pub point: f64,
    /// Lower bound of the confidence interval.
    pub ci_low: f64,
    /// Upper bound of the confidence interval.
    pub ci_high: f64,
    /// Confidence level used (e.g. `0.95` for 95%).
    pub confidence: f64,
    /// Number of bootstrap resamples used.
    pub n_resamples: usize,
}

/// Compute a percentile bootstrap CI for a scalar statistic.
///
/// Resamples `data` with replacement `n` times, applies `stat_fn` to each
/// resample, and returns the `ci`-level percentile interval. The point
/// estimate is `stat_fn` applied to the original data.
///
/// The percentile method (Efron & Tibshirani 1993) is the default here.
/// It is simpler than `BCa` and sufficient for the eval use case where sample
/// sizes are typically >20 and the statistic is close to symmetric.
///
/// # Errors
///
/// Returns an error if `data` has fewer than 2 elements or `ci` is not in
/// `(0, 1)`.
#[expect(
    clippy::cast_precision_loss,
    reason = "n and size are small bootstrap counts (<100K); f64 handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "n and size are small bootstrap counts; explicit precision loss accepted"
)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "percentile indices are in [0, n); n is a usize, so truncation cannot occur"
)]
#[expect(
    clippy::cast_sign_loss,
    reason = "alpha/2 * n is non-negative by construction"
)]
pub fn bootstrap_ci(
    data: &[f64],
    stat_fn: impl Fn(&[f64]) -> f64,
    n: usize,
    seed: u64,
    ci: f64,
) -> Result<BootstrapCi, Error> {
    if data.len() < 2 {
        return Err(StatsSnafu {
            message: format!(
                "bootstrap_ci: need at least 2 observations, got {}",
                data.len()
            ),
        }
        .build());
    }
    if !(0.0 < ci && ci < 1.0) {
        return Err(StatsSnafu {
            message: format!("bootstrap_ci: ci must be in (0, 1), got {ci}"),
        }
        .build());
    }

    let point = stat_fn(data);
    let mut boot_stats = Vec::with_capacity(n);

    let mut rng = Lcg64::new(seed);
    let size = data.len();
    let mut resample = vec![0.0_f64; size];

    for _ in 0..n {
        for slot in &mut resample {
            let idx = rng.next_bounded_usize(size);
            // INVARIANT: idx < size = data.len() by construction of next_bounded_usize.
            *slot = data.get(idx).copied().unwrap_or_default();
        }
        boot_stats.push(stat_fn(&resample));
    }

    boot_stats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let alpha = 1.0 - ci;
    let lo_idx = ((alpha / 2.0) * n as f64) as usize; // SAFETY: n < 100K; product non-negative; truncation to usize is exact
    let hi_idx = (((1.0 - alpha / 2.0) * n as f64) as usize).min(n - 1); // SAFETY: n < 100K; product non-negative; clamped to valid index

    // INVARIANT: lo_idx and hi_idx are derived percentiles in [0, n), boot_stats has n elements
    let ci_low = boot_stats.get(lo_idx).copied().unwrap_or(f64::NAN);
    let ci_high = boot_stats.get(hi_idx).copied().unwrap_or(f64::NAN);
    Ok(BootstrapCi {
        point,
        ci_low,
        ci_high,
        confidence: ci,
        n_resamples: n,
    })
}

/// Block bootstrap CI for autocorrelated time series (Daza 2018).
///
/// Preserves temporal autocorrelation by resampling contiguous blocks
/// rather than individual observations. Essential for N-of-1 designs where
/// consecutive measurements are correlated (e.g. daily metrics over weeks).
///
/// Block length defaults to `floor(sqrt(N))` — the standard heuristic that
/// balances bias and variance (Lahiri 2003).
///
/// # Errors
///
/// Returns an error if `series` has fewer than 4 elements, `ci` is not in
/// `(0, 1)`, or `block_length` exceeds the series length.
#[expect(
    clippy::cast_precision_loss,
    reason = "n and len are small bootstrap counts (<100K); f64 handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "n and len are small counts; explicit precision loss accepted"
)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "percentile indices are in [0, n); n is a usize, so truncation cannot occur"
)]
#[expect(
    clippy::cast_sign_loss,
    reason = "alpha/2 * n is non-negative by construction"
)]
pub fn block_bootstrap_ci(
    series: &[f64],
    stat_fn: impl Fn(&[f64]) -> f64,
    block_length: Option<usize>,
    n: usize,
    seed: u64,
    ci: f64,
) -> Result<BootstrapCi, Error> {
    let len = series.len();
    if len < 4 {
        return Err(StatsSnafu {
            message: format!("block_bootstrap_ci: need at least 4 observations, got {len}"),
        }
        .build());
    }
    if !(0.0 < ci && ci < 1.0) {
        return Err(StatsSnafu {
            message: format!("block_bootstrap_ci: ci must be in (0, 1), got {ci}"),
        }
        .build());
    }

    let blk = block_length
        .unwrap_or_else(|| (len as f64).sqrt().floor() as usize) // SAFETY: len is small bootstrap count (<100K); sqrt.floor fits in usize
        .max(2);
    if blk > len {
        return Err(StatsSnafu {
            message: format!(
                "block_bootstrap_ci: block_length ({blk}) exceeds series length ({len})"
            ),
        }
        .build());
    }

    let point = stat_fn(series);
    // WHY: div_ceil avoids under-allocating blocks at the tail
    let n_blocks = len.div_ceil(blk);
    let mut rng = Lcg64::new(seed);
    let mut boot_stats = Vec::with_capacity(n);
    let mut resampled = Vec::with_capacity(len);

    for _ in 0..n {
        resampled.clear();
        for _ in 0..n_blocks {
            let start = rng.next_bounded_usize(len);
            for offset in 0..blk {
                // INVARIANT: (start + offset) % len < len = series.len()
                let idx = (start + offset) % len;
                resampled.push(series.get(idx).copied().unwrap_or_default());
            }
        }
        resampled.truncate(len);
        boot_stats.push(stat_fn(&resampled));
    }

    boot_stats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let alpha = 1.0 - ci;
    let lo_idx = ((alpha / 2.0) * n as f64) as usize; // SAFETY: n < 100K; product non-negative; truncation to usize is exact
    let hi_idx = (((1.0 - alpha / 2.0) * n as f64) as usize).min(n - 1); // SAFETY: n < 100K; product non-negative; clamped to valid index

    // INVARIANT: lo_idx and hi_idx are derived percentiles in [0, n), boot_stats has n elements
    let ci_low = boot_stats.get(lo_idx).copied().unwrap_or(f64::NAN);
    let ci_high = boot_stats.get(hi_idx).copied().unwrap_or(f64::NAN);
    Ok(BootstrapCi {
        point,
        ci_low,
        ci_high,
        confidence: ci,
        n_resamples: n,
    })
}

// ── Minimal LCG RNG (no external dep) ────────────────────────────────

/// A minimal 64-bit linear congruential generator for deterministic resampling.
///
/// Uses the Knuth multiplier and addend (standard Knuth LCG).
/// This is not cryptographically secure; it is only used for reproducible
/// statistical resampling.
pub(super) struct Lcg64 {
    state: u64,
}

impl Lcg64 {
    pub(super) fn new(seed: u64) -> Self {
        // WHY: a seed of 0 causes the LCG to cycle immediately; offset by 1.
        Self {
            state: seed.wrapping_add(1),
        }
    }

    fn next_u64(&mut self) -> u64 {
        // Knuth's LCG: multiplier 6364136223846793005, addend 1442695040888963407
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Return a value in `[0, bound)` using rejection sampling for uniformity.
    ///
    /// WHY: rejection sampling (as opposed to modulo) eliminates modulo bias
    /// when `bound` does not evenly divide 2^64.
    pub(super) fn next_bounded(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next_u64();
            if r >= threshold {
                return r % bound;
            }
        }
    }

    /// Return a value in `[0, bound)` as `usize`.
    ///
    /// Bootstrap sample sizes are always `< usize::MAX`; the cast is safe.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "bound is data.len(); result is in [0, bound) which is always a valid usize"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize <-> u64 round-trip; bound fits in u64 and result fits in usize"
    )]
    pub(super) fn next_bounded_usize(&mut self, bound: usize) -> usize {
        self.next_bounded(bound as u64) as usize // SAFETY: bound fits in u64; result in [0, bound) fits in usize
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Mean of a slice. Returns `f64::NAN` if empty.
#[expect(
    clippy::cast_precision_loss,
    reason = "bootstrap data lengths are small (<100K); f64 handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64; length is bounded and small"
)]
pub(crate) fn mean(data: &[f64]) -> f64 {
    if data.is_empty() {
        return f64::NAN;
    }
    data.iter().sum::<f64>() / data.len() as f64 // SAFETY: length <100K per function-level #[expect]
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "exact equality checks in controlled tests"
)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_ci_mean_contains_true_mean() {
        // 100 values ~ uniform [0, 1], mean should be ~0.5
        let data: Vec<f64> = (0..100).map(|i| f64::from(i) / 100.0).collect();
        let ci = bootstrap_ci(&data, mean, 2000, 42, 0.95).unwrap();
        assert!(ci.ci_low < 0.5, "lower bound should be below mean");
        assert!(ci.ci_high > 0.5, "upper bound should be above mean");
        assert!(ci.ci_low < ci.point, "lower must be below point");
        assert!(ci.ci_high > ci.point, "upper must be above point");
    }

    #[test]
    fn bootstrap_ci_rejects_too_few_observations() {
        let data = vec![1.0];
        assert!(bootstrap_ci(&data, mean, 100, 42, 0.95).is_err());
    }

    #[test]
    fn bootstrap_ci_rejects_invalid_ci() {
        let data = vec![1.0, 2.0, 3.0];
        assert!(bootstrap_ci(&data, mean, 100, 42, 0.0).is_err());
        assert!(bootstrap_ci(&data, mean, 100, 42, 1.0).is_err());
        assert!(bootstrap_ci(&data, mean, 100, 42, -0.1).is_err());
    }

    #[test]
    fn block_bootstrap_ci_rejects_short_series() {
        let series = vec![1.0, 2.0, 3.0];
        assert!(block_bootstrap_ci(&series, mean, None, 100, 42, 0.95).is_err());
    }

    #[test]
    fn block_bootstrap_ci_contains_point() {
        let series: Vec<f64> = (0..50).map(|i| f64::from(i).sin()).collect();
        let ci = block_bootstrap_ci(&series, mean, None, 1000, 42, 0.95).unwrap();
        // CI should bracket the point estimate
        assert!(ci.ci_low <= ci.point + 1e-9);
        assert!(ci.ci_high >= ci.point - 1e-9);
    }

    #[test]
    fn bootstrap_ci_is_deterministic() {
        let data: Vec<f64> = (0..30).map(f64::from).collect();
        let a = bootstrap_ci(&data, mean, 500, 42, 0.95).unwrap();
        let b = bootstrap_ci(&data, mean, 500, 42, 0.95).unwrap();
        assert_eq!(a.ci_low, b.ci_low);
        assert_eq!(a.ci_high, b.ci_high);
    }

    #[test]
    fn lcg_never_returns_out_of_bound() {
        let mut rng = Lcg64::new(12345);
        for _ in 0..10_000 {
            let v = rng.next_bounded(10);
            assert!(v < 10);
        }
    }
}
