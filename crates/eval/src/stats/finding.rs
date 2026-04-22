//! Back-compat re-export of `eidos::knowledge::finding`.
//!
//! The canonical types live in `eidos` so that multiple crates (`eval`,
//! `nous::self_audit`, `daemon::prosoche`) can share a single finding shape.
//! This module preserves the old `eval::stats::finding` path until callers
//! migrate.

pub use eidos::knowledge::finding::{
    ComparisonSource, ConfidenceSummary, EvidenceLevel, Finding, FindingStats,
};

/// Historical alias for [`Finding`].
pub use eidos::knowledge::finding::Finding as EvalFinding;

use super::ComparisonReport;

impl ComparisonSource for ComparisonReport {
    fn p_adjusted(&self) -> Option<f64> {
        self.p_adjusted
    }

    fn significant_raw(&self) -> Option<bool> {
        self.significant_raw
    }

    fn effect_d(&self) -> f64 {
        self.effect.d
    }

    fn ci_a(&self) -> [f64; 2] {
        [self.ci_a.ci_low, self.ci_a.ci_high]
    }

    fn sample_sizes(&self) -> [usize; 2] {
        [self.n_a, self.n_b]
    }
}
