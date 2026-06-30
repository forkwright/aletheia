#![deny(missing_docs)]
//! Behavioral and cognitive evaluation framework for Aletheia runtime instances.

/// Memory benchmark harness: LongMemEval, LoCoMo scoring against aletheia.
pub mod benchmarks;
/// HTTP client for communicating with the Aletheia API during evaluation runs.
pub(crate) mod client;
/// Cognitive evaluations: recall@k, sycophancy detection, adversarial testing.
pub(crate) mod cognitive;
/// Coverage policies and skip classification for eval reports.
pub mod coverage;
/// Eval-specific error types and result alias.
pub mod error;
/// JSONL persistence for evaluation results as training data.
pub mod persistence;
/// Durable provenance envelope shared by eval runs and benchmark reports.
pub mod provenance;
/// Evaluation provider trait: pluggable scenario sources for programmatic use.
pub mod provider;
/// Evaluation report types for summarizing scenario results.
pub mod report;
/// Evaluation scenario runner: executes scenarios and collects results.
pub mod runner;
/// Scenario definition types: steps, assertions, and expected outcomes.
pub mod scenario;
/// Built-in evaluation scenarios for validating Aletheia runtime behavior.
pub mod scenarios;
/// SSE stream consumer for real-time evaluation output.
pub mod sse;
/// Statistical helpers: bootstrap CI, effect size, FDR correction.
///
/// Absorbed from the quantified-self pipeline (`shared/stats.py`).
/// Every benchmark comparison that publishes results must report
/// CI + effect size + FDR-adjusted p-value via this module.
pub mod stats;
/// Typed-tag namespace over RunReport for SFT/distillation pipeline.
pub mod tags;

#[cfg(test)]
mod tag_tests;

#[cfg(test)]
// kanon:ignore RUST/test-missing-use-super — tests reference symbols via fully-qualified `crate::` paths; no items need importing from super
mod tests {
    #[test]
    fn public_modules_accessible() {
        let report = crate::runner::RunReport {
            passed: 0,
            failed: 0,
            skipped: 0,
            total_duration: std::time::Duration::from_secs(0),
            results: vec![],
            provenance: crate::provenance::EvalProvenance::new("er-test", "http://localhost"),
        };
        crate::report::print_report(&report, "http://localhost");
        assert_eq!(report.passed, 0, "public module should be accessible");
    }
}
