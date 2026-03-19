#![deny(missing_docs)]
//! Behavioral evaluation framework for Aletheia runtime instances.

/// HTTP client for communicating with the Aletheia API during evaluation runs.
pub(crate) mod client;
/// Eval-specific error types and result alias.
pub(crate) mod error;
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
