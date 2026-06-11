//! Statistical helpers for eval benchmark reporting.
//!
//! This module is the single source of truth for statistical discipline in
//! the eval crate. Every benchmark comparison that publishes scores **must**
//! report:
//!
//! - Point estimate
//! - 95% bootstrap CI
//! - Effect size (Cohen's d) with interpretation
//! - Raw p-value (permutation test)
//! - FDR-adjusted p-value when multiple comparisons are made
//!
//! # Design
//!
//! All functions are pure: no global RNG state. Determinism is achieved by
//! requiring a seed parameter. Defaults to `seed = 42` where unspecified.
//!
//! # References
//!
//! - Efron, B. & Hastie, T. (2021). *Computer Age Statistical Inference* (2nd ed.)
//!   — percentile bootstrap methodology.
//! - Daza, E.J. (2018). Causal analysis of self-tracked time series data.
//!   — block bootstrap for autocorrelated series.
//! - Cohen, J. (1988). *Statistical Power Analysis for the Behavioral Sciences*.
//!   — Cohen's d effect size thresholds.
//! - Benjamini, Y. & Hochberg, Y. (1995). Controlling the false discovery rate.
//!   — B-H FDR correction algorithm.
//! - Benjamini, Y. & Yekutieli, D. (2001). FDR under dependency.
//!   — B-Y FDR correction for correlated tests.

pub mod bootstrap;
pub mod effect_size;
pub mod fdr;
pub mod finding;
pub mod report;

pub use bootstrap::{BootstrapCi, block_bootstrap_ci, bootstrap_ci};
pub use effect_size::{CohensD, EffectSizeInterpretation, cohens_d};
pub use fdr::{FdrMethod, fdr_correct};
pub use finding::{ConfidenceSummary, EvalFinding, EvidenceLevel, FindingStats};
pub use report::{ComparisonReport, comparison_report};
