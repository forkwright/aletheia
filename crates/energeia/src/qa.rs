//! Quality assurance gate: trait for evaluating dispatch output.
//!
//! The [`QaGate`] trait separates mechanical pre-screening (fast, no LLM cost)
//! from semantic evaluation (uses hermeneus for LLM-based assessment).

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::types::{MechanicalIssue, QaResult};

// ---------------------------------------------------------------------------
// QaGate trait
// ---------------------------------------------------------------------------

/// Abstraction over quality assurance evaluation.
///
/// Implementations use hermeneus for LLM-based semantic evaluation and
/// perform mechanical checks (blast radius, lint, format) without LLM calls.
pub trait QaGate: Send + Sync {
    /// Evaluate a pull request against the prompt's acceptance criteria.
    ///
    /// Combines mechanical pre-screening with LLM-based semantic evaluation.
    /// Returns a [`QaResult`] with per-criterion results and overall verdict.
    fn evaluate<'a>(
        &'a self,
        prompt: &'a PromptSpec,
        pr_number: u64,
        diff: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<QaResult>> + Send + 'a>>;

    /// Run mechanical checks only (no LLM cost).
    ///
    /// Returns issues detectable by static analysis: blast radius violations,
    /// anti-patterns, lint failures, format violations.
    fn mechanical_check(&self, diff: &str, prompt: &PromptSpec) -> Vec<MechanicalIssue>;
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Specification of a prompt for QA evaluation.
///
/// Contains the acceptance criteria and blast radius constraints that the
/// QA gate evaluates against.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PromptSpec {
    /// Prompt number within the dispatch.
    pub prompt_number: u32,
    /// Human-readable task description.
    pub description: String,
    /// Acceptance criteria that the PR must satisfy.
    pub acceptance_criteria: Vec<String>,
    /// Files that the prompt is allowed to modify.
    pub blast_radius: Vec<String>,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn prompt_spec_roundtrip() {
        let spec = PromptSpec {
            prompt_number: 1,
            description: "add health endpoint".to_owned(),
            acceptance_criteria: vec![
                "GET /health returns 200".to_owned(),
                "response includes version".to_owned(),
            ],
            blast_radius: vec!["src/handlers/health.rs".to_owned()],
        };
        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: PromptSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt_number, 1);
        assert_eq!(deserialized.acceptance_criteria.len(), 2);
        assert_eq!(deserialized.blast_radius.len(), 1);
    }
}
