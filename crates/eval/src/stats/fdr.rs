//! False discovery rate correction.
//!
//! Implements Benjamini-Hochberg (B-H) and Benjamini-Yekutieli (B-Y) FDR
//! correction, translated from `shared/stats.py` in the quantified-self
//! pipeline.
//!
//! Use B-H (default) for independent or positively correlated tests.
//! Use B-Y when tests share samples or are otherwise dependent.
//!
//! # References
//!
//! - Benjamini, Y. & Hochberg, Y. (1995). Controlling the false discovery rate:
//!   A practical and powerful approach to multiple testing. *JRSS-B*, 57(1), 289–300.
//! - Benjamini, Y. & Yekutieli, D. (2001). The control of the false discovery rate
//!   in multiple testing under dependency. *Annals of Statistics*, 29(4), 1165–1188.

use crate::error::{Error, StatsSnafu};

/// FDR correction method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FdrMethod {
    /// Benjamini-Hochberg (1995): controls FDR under independence or PRDS.
    /// Appropriate for independent or positively-correlated tests.
    #[default]
    BenjaminiHochberg,
    /// Benjamini-Yekutieli (2001): controls FDR under arbitrary dependency.
    /// More conservative; use when tests share samples.
    BenjaminiYekutieli,
}

/// Apply FDR correction to a slice of raw p-values.
///
/// Returns adjusted p-values in the **same order** as the input. Values
/// are clamped to `[0.0, 1.0]` after adjustment.
///
/// # Algorithm (B-H step-up)
///
/// 1. Sort p-values ascending, tracking original indices.
/// 2. For rank `i` of `m` tests: `p_adj[i] = p[i] * m * c(m) / i`.
///    `c(m) = 1.0` for B-H, `c(m) = sum(1/k, k=1..m)` for B-Y.
/// 3. Enforce monotonicity: walk from highest rank down, taking
///    cumulative minimum so that `p_adj` is non-decreasing in sort order.
/// 4. Clamp to `[0.0, 1.0]`.
///
/// # Errors
///
/// Returns an error if any p-value is outside `[0.0, 1.0]`.
#[expect(
    clippy::cast_precision_loss,
    reason = "p-value counts are small (<10K); f64 handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — counts are bounded and small"
)]
pub fn fdr_correct(p_values: &[f64], method: FdrMethod) -> Result<Vec<f64>, Error> {
    let m = p_values.len();
    if m == 0 {
        return Ok(vec![]);
    }

    for (i, &p) in p_values.iter().enumerate() {
        if !(0.0..=1.0).contains(&p) {
            return Err(StatsSnafu {
                message: format!("fdr_correct: p_values[{i}] = {p} is not in [0, 1]"),
            }
            .build());
        }
    }

    // Correction factor
    let c_m: f64 = match method {
        FdrMethod::BenjaminiHochberg => 1.0,
        FdrMethod::BenjaminiYekutieli => {
            // harmonic sum: sum(1/k, k=1..m)
            (1..=m).map(|k| 1.0 / k as f64).sum() // SAFETY: m <10K per function-level #[expect]
        }
    };

    let mut indexed: Vec<(usize, f64)> = p_values.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Step-up adjustment
    let mut adjusted = vec![0.0_f64; m];
    for (rank_0, &(orig_idx, p)) in indexed.iter().enumerate() {
        let rank = rank_0 + 1; // 1-based
        // INVARIANT: orig_idx in [0, m) (permutation of range), adjusted has length m
        if let Some(slot) = adjusted.get_mut(orig_idx) {
            *slot = p * m as f64 * c_m / rank as f64; // SAFETY: m and rank <10K per function-level #[expect]
        }
    }

    // Enforce monotonicity: walk from last sorted index backwards,
    // ensuring adjusted[i] <= adjusted[i+1] in sort order.
    let sorted_indices: Vec<usize> = indexed.iter().map(|&(idx, _)| idx).collect();
    // INVARIANT: m > 0 checked at top (early-return for m == 0); sorted_indices has m entries
    if let Some(&last) = sorted_indices.get(m - 1)
        && let Some(last_slot) = adjusted.get_mut(last)
    {
        *last_slot = last_slot.min(1.0);
    }
    for i in (0..m - 1).rev() {
        // INVARIANT: i, i+1 < m = sorted_indices.len(); values are valid adjusted[] indices
        let Some(&idx) = sorted_indices.get(i) else {
            continue;
        };
        let Some(&next_idx) = sorted_indices.get(i + 1) else {
            continue;
        };
        let next_val = adjusted.get(next_idx).copied().unwrap_or(1.0);
        if let Some(slot) = adjusted.get_mut(idx) {
            *slot = slot.min(next_val).min(1.0);
        }
    }

    Ok(adjusted)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test asserts against known-length vectors"
)]
mod tests {
    use super::*;

    #[test]
    fn fdr_empty_input_returns_empty() {
        let result: Vec<f64> = fdr_correct(&[], FdrMethod::BenjaminiHochberg).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn fdr_single_p_value_unchanged() {
        let adj = fdr_correct(&[0.04], FdrMethod::BenjaminiHochberg).unwrap();
        assert_eq!(adj.len(), 1);
        // Only one test: p_adj = p * 1 / 1 = p
        assert!((adj[0] - 0.04).abs() < 1e-10);
    }

    #[test]
    fn fdr_bh_adjusts_upward() {
        // Three tests: [0.01, 0.04, 0.3]
        // Rank 1: 0.01 * 3/1 = 0.03
        // Rank 2: 0.04 * 3/2 = 0.06
        // Rank 3: 0.3  * 3/3 = 0.30
        // Monotonicity: all non-decreasing, no change.
        let adj = fdr_correct(&[0.01, 0.04, 0.3], FdrMethod::BenjaminiHochberg).unwrap();
        assert!((adj[0] - 0.03).abs() < 1e-10, "p[0] adj: {}", adj[0]);
        assert!((adj[1] - 0.06).abs() < 1e-10, "p[1] adj: {}", adj[1]);
        assert!((adj[2] - 0.30).abs() < 1e-10, "p[2] adj: {}", adj[2]);
    }

    #[test]
    fn fdr_monotonicity_enforced() {
        // When a smaller raw p at higher rank would produce a larger adjusted p
        // than the value at rank+1, monotonicity enforcement pulls it down.
        // [0.01, 0.01, 0.01] — rank 1: 0.03, rank 2: 0.015, rank 3: 0.01
        // After monotonicity: all should equal 0.03 -> 0.03, then min(0.015, 0.01) -> NO:
        // Walk from last: adjusted[2] = 0.01, adjusted[1] = min(0.015, 0.01) = 0.01,
        // adjusted[0] = min(0.03, 0.01) = 0.01
        let adj = fdr_correct(&[0.01, 0.01, 0.01], FdrMethod::BenjaminiHochberg).unwrap();
        for &v in &adj {
            assert!((v - 0.01).abs() < 1e-10, "expected 0.01, got {v}");
        }
    }

    #[test]
    fn fdr_by_is_more_conservative_than_bh() {
        let p = vec![0.01, 0.02, 0.05, 0.1, 0.2];
        let bh = fdr_correct(&p, FdrMethod::BenjaminiHochberg).unwrap();
        let by = fdr_correct(&p, FdrMethod::BenjaminiYekutieli).unwrap();
        // B-Y uses a larger correction factor, so adjusted values are larger (more conservative)
        for i in 0..p.len() {
            assert!(
                by[i] >= bh[i] - 1e-10,
                "B-Y[{i}]={} should be >= BH[{i}]={}",
                by[i],
                bh[i]
            );
        }
    }

    #[test]
    fn fdr_preserves_order_of_input() {
        // input in descending order; output must match input positions
        let p = vec![0.5, 0.1, 0.01];
        let adj = fdr_correct(&p, FdrMethod::BenjaminiHochberg).unwrap();
        assert_eq!(adj.len(), 3);
        // The smallest p-value (p[2]=0.01) gets the most adjustment (rank 1 of 3)
        // adj[2] = 0.01 * 3/1 = 0.03
        // adj[1] = 0.1  * 3/2 = 0.15
        // adj[0] = 0.5  * 3/3 = 0.5
        assert!((adj[0] - 0.5).abs() < 1e-10, "p[0]: {}", adj[0]);
        assert!((adj[1] - 0.15).abs() < 1e-10, "p[1]: {}", adj[1]);
        assert!((adj[2] - 0.03).abs() < 1e-10, "p[2]: {}", adj[2]);
    }

    #[test]
    fn fdr_rejects_out_of_range_p_values() {
        assert!(fdr_correct(&[-0.1, 0.5], FdrMethod::BenjaminiHochberg).is_err());
        assert!(fdr_correct(&[0.5, 1.1], FdrMethod::BenjaminiHochberg).is_err());
    }

    #[test]
    fn fdr_clamps_adjusted_to_one() {
        // A very small p-value with many tests can produce adj > 1; must clamp.
        let p = vec![0.9999; 100];
        let adj = fdr_correct(&p, FdrMethod::BenjaminiHochberg).unwrap();
        for &v in &adj {
            assert!(v <= 1.0, "adjusted p must be <= 1.0, got {v}");
        }
    }
}
