//! Agent-native finding format for eval results.
//!
//! Rust translation of `shared/agent_format.py` from the quantified-self
//! pipeline. Each eval result that makes a claim about system quality should
//! be represented as a [`Finding`] so that:
//!
//! - Claims carry an explicit evidence level (statistical/exploratory/…)
//! - Every finding includes its strongest counter-argument
//! - Statistical metadata (CI, effect size, adjusted p-value) travel with the claim
//! - Results are serializable to JSON for programmatic consumption
//!
//! # Design
//!
//! The [`Finding`] is not a replacement for a full comparison report — it is a
//! wrapper that adds the epistemic framing needed for the recursive
//! self-improvement loop (Phase 06b). When aletheia reports on its own eval
//! results in agent context, it uses `Finding` so other agents can
//! reason about claim confidence without re-reading raw numbers.
//!
//! # Evidence levels
//!
//! | Level | Meaning |
//! |-------|---------|
//! | `Statistical` | Passes FDR correction, sample size adequate for inference |
//! | `Robust` | Consistent across scoring methods but p not FDR-corrected |
//! | `Exploratory` | Pattern present, hypothesis-generating only |
//! | `Interpretive` | Qualitative reading, no direct statistic |
//! | `Speculative` | Low-confidence extrapolation |

use serde::{Deserialize, Serialize};

/// Abstract source of comparison data for [`Finding::from_comparison`].
///
/// Implemented by `eval::stats::ComparisonReport` so that `eidos` does not
/// need to depend on `eval`.
pub trait ComparisonSource {
    /// FDR-adjusted p-value. `None` for non-statistical findings.
    fn p_adjusted(&self) -> Option<f64>;
    /// Whether the raw result is significant.
    fn significant_raw(&self) -> Option<bool>;
    /// Effect size value (Cohen's d).
    fn effect_d(&self) -> f64;
    /// 95% CI as `[low, high]` for the primary group.
    fn ci_a(&self) -> [f64; 2];
    /// Sample sizes: `[n_a, n_b]`.
    fn sample_sizes(&self) -> [usize; 2];
}

/// Epistemological tier of an evaluation finding.
///
/// Mirrors the `evidence_level` field in `shared/agent_format.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EvidenceLevel {
    /// Survives FDR correction, n is large enough for inference.
    Statistical,
    /// Consistent across methods but p-value is not FDR-corrected.
    Robust,
    /// Pattern present; useful for hypothesis generation only.
    Exploratory,
    /// Qualitative reading; no direct statistical backing.
    Interpretive,
    /// Low-confidence extrapolation; flag explicitly in reports.
    Speculative,
}

impl std::fmt::Display for EvidenceLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Statistical => write!(f, "statistical"),
            Self::Robust => write!(f, "robust"),
            Self::Exploratory => write!(f, "exploratory"),
            Self::Interpretive => write!(f, "interpretive"),
            Self::Speculative => write!(f, "speculative"),
        }
    }
}

/// Statistical metadata attached to a finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingStats {
    /// FDR-adjusted p-value. `None` for non-statistical findings.
    pub p_adjusted: Option<f64>,
    /// Effect size metric name (e.g. `"cohens_d"`).
    pub effect_metric: Option<String>,
    /// Effect size value.
    pub effect_value: Option<f64>,
    /// 95% CI as `[low, high]`. `None` when not computable.
    pub ci: Option<[f64; 2]>,
    /// Sample sizes: `[n_a, n_b]`.
    pub sample_sizes: Option<[usize; 2]>,
}

impl FindingStats {
    /// No-op stats for non-statistical evidence levels.
    #[must_use]
    pub fn none() -> Self {
        Self {
            p_adjusted: None,
            effect_metric: None,
            effect_value: None,
            ci: None,
            sample_sizes: None,
        }
    }
}

/// A structured finding from an eval run, suitable for agent consumption.
///
/// Mirrors `AgentFinding` in `shared/agent_format.py`, adapted for Rust
/// and the aletheia eval context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Stable identifier for this finding within the eval run.
    ///
    /// Convention: `<BENCHMARK>-<CATEGORY>-<N>`, e.g. `LME-factual-001`.
    pub finding_id: String,
    /// The claim as a single declarative sentence.
    ///
    /// Write as: "{system} {verb} {metric} ({value}), {framing qualifier}."
    /// Example: "Candidate recall@5 is 0.73 (↑0.08 vs baseline), exploratory."
    pub claim: String,
    /// Epistemological tier of the evidence.
    pub evidence_level: EvidenceLevel,
    /// The strongest objection a hostile reader would raise.
    ///
    /// Forces the producer to acknowledge scope limits before publication.
    /// Every finding must include one even if only "insufficient sample for
    /// inference" or "metric does not capture semantic correctness".
    pub counter_argument: String,
    /// Provenance: what produced this finding (e.g. `"benchmark/runner.rs"`).
    pub source: String,
    /// Statistical backing. Use [`FindingStats::none`] for non-statistical tiers.
    pub stats: FindingStats,
}

impl Finding {
    /// Convenience constructor for a statistical finding from a comparison report.
    ///
    /// Sets `evidence_level` to [`EvidenceLevel::Statistical`] when
    /// `report.p_adjusted()` is `Some(p)` and `p < 0.05`, otherwise
    /// [`EvidenceLevel::Exploratory`] or [`EvidenceLevel::Robust`].
    #[must_use]
    pub fn from_comparison(
        finding_id: impl Into<String>,
        claim: impl Into<String>,
        counter_argument: impl Into<String>,
        source: impl Into<String>,
        report: &impl ComparisonSource,
    ) -> Self {
        let evidence_level = match report.p_adjusted() {
            Some(p) if p < 0.05 => EvidenceLevel::Statistical,
            Some(_) => EvidenceLevel::Exploratory,
            None => {
                if report.significant_raw() == Some(true) {
                    EvidenceLevel::Robust
                } else {
                    EvidenceLevel::Exploratory
                }
            }
        };

        let stats = FindingStats {
            p_adjusted: report.p_adjusted(),
            effect_metric: Some("cohens_d".to_owned()),
            effect_value: Some(report.effect_d()),
            ci: Some(report.ci_a()),
            sample_sizes: Some(report.sample_sizes()),
        };

        Self {
            finding_id: finding_id.into(),
            claim: claim.into(),
            evidence_level,
            counter_argument: counter_argument.into(),
            source: source.into(),
            stats,
        }
    }
}

/// Summary of findings by evidence level, for scan-level reporting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfidenceSummary {
    /// Count of `Statistical` findings.
    pub statistical: usize,
    /// Count of `Robust` findings.
    pub robust: usize,
    /// Count of `Exploratory` findings.
    pub exploratory: usize,
    /// Count of `Interpretive` findings.
    pub interpretive: usize,
    /// Count of `Speculative` findings.
    pub speculative: usize,
}

impl ConfidenceSummary {
    /// Build a summary from a slice of findings.
    #[must_use]
    pub fn from_findings(findings: &[Finding]) -> Self {
        let mut summary = Self::default();
        for f in findings {
            match f.evidence_level {
                EvidenceLevel::Statistical => summary.statistical += 1,
                EvidenceLevel::Robust => summary.robust += 1,
                EvidenceLevel::Exploratory => summary.exploratory += 1,
                EvidenceLevel::Interpretive => summary.interpretive += 1,
                EvidenceLevel::Speculative => summary.speculative += 1,
            }
        }
        summary
    }

    /// Total findings across all evidence levels.
    #[must_use]
    pub fn total(&self) -> usize {
        self.statistical + self.robust + self.exploratory + self.interpretive + self.speculative
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockReport {
        p_adjusted: Option<f64>,
        significant_raw: Option<bool>,
        effect_d: f64,
        ci_a: [f64; 2],
        sample_sizes: [usize; 2],
    }

    impl ComparisonSource for MockReport {
        fn p_adjusted(&self) -> Option<f64> {
            self.p_adjusted
        }
        fn significant_raw(&self) -> Option<bool> {
            self.significant_raw
        }
        fn effect_d(&self) -> f64 {
            self.effect_d
        }
        fn ci_a(&self) -> [f64; 2] {
            self.ci_a
        }
        fn sample_sizes(&self) -> [usize; 2] {
            self.sample_sizes
        }
    }

    #[test]
    fn evidence_level_ordering() {
        // Statistical > Robust > Exploratory > Interpretive > Speculative
        assert!(EvidenceLevel::Statistical < EvidenceLevel::Robust);
        assert!(EvidenceLevel::Robust < EvidenceLevel::Exploratory);
    }

    #[test]
    fn finding_stats_none_has_no_values() {
        let s = FindingStats::none();
        assert!(s.p_adjusted.is_none());
        assert!(s.effect_metric.is_none());
        assert!(s.ci.is_none());
    }

    #[test]
    fn from_comparison_statistical_when_adjusted_p_small() {
        let report = MockReport {
            p_adjusted: Some(0.01),
            significant_raw: Some(true),
            effect_d: 0.8,
            ci_a: [0.1, 0.5],
            sample_sizes: [20, 20],
        };
        let finding = Finding::from_comparison(
            "TEST-001",
            "Candidate outperforms baseline.",
            "Small sample.",
            "tests",
            &report,
        );
        assert_eq!(finding.evidence_level, EvidenceLevel::Statistical);
        assert_eq!(finding.stats.p_adjusted, Some(0.01));
    }

    #[test]
    fn from_comparison_exploratory_when_no_adjusted_p_and_raw_not_sig() {
        let report = MockReport {
            p_adjusted: None,
            significant_raw: Some(false),
            effect_d: 0.0,
            ci_a: [0.0, 0.0],
            sample_sizes: [10, 10],
        };
        let finding = Finding::from_comparison("F-001", "no effect", "none", "tests", &report);
        assert!(matches!(finding.evidence_level, EvidenceLevel::Exploratory));
    }

    #[test]
    fn confidence_summary_counts_correctly() {
        let findings = vec![
            Finding {
                finding_id: "a".to_owned(),
                claim: "x".to_owned(),
                evidence_level: EvidenceLevel::Statistical,
                counter_argument: "y".to_owned(),
                source: "z".to_owned(),
                stats: FindingStats::none(),
            },
            Finding {
                finding_id: "b".to_owned(),
                claim: "x".to_owned(),
                evidence_level: EvidenceLevel::Exploratory,
                counter_argument: "y".to_owned(),
                source: "z".to_owned(),
                stats: FindingStats::none(),
            },
        ];
        let summary = ConfidenceSummary::from_findings(&findings);
        assert_eq!(summary.statistical, 1);
        assert_eq!(summary.exploratory, 1);
        assert_eq!(summary.total(), 2);
    }
}
