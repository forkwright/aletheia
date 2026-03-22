#![deny(missing_docs)]
//! Behavioral and cognitive evaluation framework for Aletheia runtime instances.

/// HTTP client for communicating with the Aletheia API during evaluation runs.
pub(crate) mod client;
/// Cognitive evaluations: recall@k, sycophancy detection, adversarial testing.
pub(crate) mod cognitive;
/// Eval-specific error types and result alias.
pub(crate) mod error;
/// JSONL persistence for evaluation results as training data.
pub mod persistence;
/// Evaluation report types for summarizing scenario results.
pub mod report;
/// Evaluation scenario runner: executes scenarios and collects results.
pub mod runner;
/// Scenario definition types: steps, assertions, and expected outcomes.
pub(crate) mod scenario;
/// Built-in evaluation scenarios for validating Aletheia runtime behavior.
pub mod scenarios;
/// SSE stream consumer for real-time evaluation output.
pub mod sse;
/// Configurable evaluation trigger scheduling.
pub mod triggers;

#[cfg(test)]
mod tests {
    #[test]
    fn public_modules_accessible() {
        // NOTE: verifies that the crate's public module structure is intact
        let _: fn(&crate::runner::RunReport, &str) = crate::report::print_report;
    }
}
