//! Quality assurance evaluation engine for dispatch output.
//!
//! Orchestrates the QA flow:
//! 1. Run mechanical pre-screening (blast radius, anti-patterns)
//! 2. Classify acceptance criteria (mechanical vs semantic)
//! 3. Auto-pass mechanical criteria if no mechanical issues found
//! 4. Run LLM evaluation on semantic criteria via hermeneus
//! 5. Determine verdict: Pass / Partial / Fail
//! 6. Capture training data as a lesson
//! 7. If Partial/Fail, generate corrective prompt
//!
//! The [`QaGate`] trait separates mechanical pre-screening (fast, no LLM cost)
//! from semantic evaluation (uses hermeneus for LLM-based assessment).

use std::future::Future;
use std::pin::Pin;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::types::{CriterionResult, CriterionType, MechanicalIssue, QaResult};

/// Corrective prompt generation for failed QA evaluations.
pub mod corrective;
/// Mechanical pre-screening: blast radius, anti-patterns, lint, format.
pub mod mechanical;
/// LLM-based semantic evaluation: criteria classification, prompt building,
/// response parsing.
pub mod semantic;
/// Verdict determination from mechanical issues and criterion results.
pub mod verdict;

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

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Run a full QA evaluation on a PR diff.
///
/// Orchestrates the complete flow: mechanical pre-screening, criteria
/// classification, LLM evaluation of semantic criteria, verdict determination,
/// and training data capture.
///
/// # Arguments
///
/// * `diff` - The unified diff of the PR
/// * `prompt` - The prompt specification with criteria and blast radius
/// * `pr_number` - The pull request number
///
/// # LLM evaluation
///
/// Semantic criteria evaluation requires hermeneus, which is currently disabled
/// due to compilation errors. When hermeneus is unavailable, semantic criteria
/// are marked as failed with explanatory evidence. The mechanical checks, verdict
/// logic, and corrective generation all work without hermeneus.
pub fn run_qa(diff: &str, prompt: &PromptSpec, pr_number: u64) -> QaResult {
    tracing::info!(
        prompt_number = prompt.prompt_number,
        pr_number,
        "starting QA evaluation"
    );

    // NOTE: Step 1 — run mechanical pre-screening.
    let mechanical_issues = mechanical::mechanical_check(diff, prompt);

    if !mechanical_issues.is_empty() {
        tracing::warn!(count = mechanical_issues.len(), "mechanical issues found");
    }

    // NOTE: Step 2 — classify acceptance criteria.
    let classified = semantic::classify_criteria(&prompt.acceptance_criteria);

    // NOTE: Step 3 — auto-pass mechanical criteria if no issues found.
    let no_mechanical_issues = mechanical_issues.is_empty();

    let mechanical_results: Vec<CriterionResult> = classified
        .iter()
        .filter(|(_, ct)| *ct == CriterionType::Mechanical)
        .map(|(text, ct)| CriterionResult {
            criterion: text.clone(),
            classification: *ct,
            passed: no_mechanical_issues,
            evidence: if no_mechanical_issues {
                "auto-passed: no mechanical issues detected".to_owned()
            } else {
                "failed: mechanical issues detected in pre-screening".to_owned()
            },
        })
        .collect();

    // NOTE: Step 4 — evaluate semantic criteria.
    // WHY: Skip LLM evaluation if there are critical mechanical issues (save cost).
    let semantic_criteria: Vec<(String, CriterionType)> = classified
        .iter()
        .filter(|(_, ct)| *ct == CriterionType::Semantic)
        .cloned()
        .collect();

    let (semantic_results, cost_usd) =
        if verdict::has_critical_mechanical_issues(&mechanical_issues) {
            tracing::info!("skipping LLM evaluation due to critical mechanical issues");
            let results = semantic_criteria
                .iter()
                .map(|(text, ct)| CriterionResult {
                    criterion: text.clone(),
                    classification: *ct,
                    passed: false,
                    evidence: "skipped: critical mechanical issues prevent evaluation".to_owned(),
                })
                .collect();
            (results, 0.0)
        } else if semantic_criteria.is_empty() {
            (Vec::new(), 0.0)
        } else {
            // WHY: hermeneus is temporarily disabled due to compilation errors.
            // Build the evaluation prompt so the infrastructure is ready, but
            // mark criteria as unevaluated until hermeneus is restored.
            let _qa_prompt =
                semantic::build_qa_prompt(&prompt.description, &semantic_criteria, diff);

            tracing::warn!("hermeneus unavailable — semantic criteria marked as unevaluated");

            let results = semantic_criteria
                .iter()
                .map(|(text, ct)| CriterionResult {
                    criterion: text.clone(),
                    classification: *ct,
                    passed: false,
                    evidence: "LLM evaluation unavailable — hermeneus pending compilation fix"
                        .to_owned(),
                })
                .collect();
            (results, 0.0)
        };

    // NOTE: Step 5 — combine results and determine verdict.
    let mut all_results = mechanical_results;
    all_results.extend(semantic_results);

    let qa_verdict = verdict::determine_verdict(&all_results, &mechanical_issues);

    let qa_result = QaResult {
        prompt_number: prompt.prompt_number,
        pr_number,
        verdict: qa_verdict,
        criteria_results: all_results,
        mechanical_issues,
        cost_usd,
        evaluated_at: Timestamp::now(),
    };

    tracing::info!(
        verdict = %qa_result.verdict,
        cost_usd = qa_result.cost_usd,
        "QA evaluation complete"
    );

    qa_result
}

/// Record a QA evaluation result as a lesson in the store.
///
/// Captures the verdict, criteria details, and cost for training data.
/// This is best-effort: failures are logged but do not propagate.
#[cfg(feature = "storage-fjall")]
pub fn record_training_data(
    store: &crate::store::fjall_store::EnergeiaStore,
    qa_result: &QaResult,
    project: &str,
) {
    let passed_count = qa_result
        .criteria_results
        .iter()
        .filter(|r| r.passed)
        .count();
    let total_count = qa_result.criteria_results.len();

    let lesson_text = format!(
        "QA evaluation for prompt #{} PR #{}: {} ({}/{} criteria passed, ${:.2})",
        qa_result.prompt_number,
        qa_result.pr_number,
        qa_result.verdict,
        passed_count,
        total_count,
        qa_result.cost_usd,
    );

    let failed_criteria: Vec<String> = qa_result
        .criteria_results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| format!("  - {}: {}", r.criterion, r.evidence))
        .collect();

    let evidence = if failed_criteria.is_empty() {
        None
    } else {
        Some(format!("Failed criteria:\n{}", failed_criteria.join("\n")))
    };

    let lesson = crate::store::records::NewLesson {
        source: "qa".to_owned(),
        category: "qa-evaluation".to_owned(),
        lesson: lesson_text,
        evidence,
        project: Some(project.to_owned()),
        prompt_number: Some(qa_result.prompt_number),
    };

    if let Err(e) = store.add_lesson(&lesson) {
        tracing::warn!(error = %e, "failed to write QA training data");
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::types::QaVerdict;

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

    #[test]
    fn run_qa_all_mechanical_pass() {
        let prompt = PromptSpec {
            prompt_number: 1,
            description: "add endpoint".to_owned(),
            acceptance_criteria: vec![
                "`cargo test` passes".to_owned(),
                "`cargo clippy` passes".to_owned(),
            ],
            blast_radius: vec![],
        };
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+pub fn foo() {}\n";

        let result = run_qa(diff, &prompt, 42);

        // NOTE: No mechanical issues → mechanical criteria auto-pass.
        // But hermeneus is unavailable, so there should be no semantic criteria
        // to fail (these are all mechanical keywords).
        assert_eq!(result.verdict, QaVerdict::Pass);
        assert!(result.mechanical_issues.is_empty());
    }

    #[test]
    fn run_qa_blast_radius_violation_fails() {
        let prompt = PromptSpec {
            prompt_number: 1,
            description: "scoped change".to_owned(),
            acceptance_criteria: vec!["feature works".to_owned()],
            blast_radius: vec!["src/allowed/".to_owned()],
        };
        let diff = "+++ b/src/outside/file.rs\n@@ -1 +1,2 @@\n+new line\n";

        let result = run_qa(diff, &prompt, 42);

        assert_eq!(result.verdict, QaVerdict::Fail);
        assert!(!result.mechanical_issues.is_empty());
    }

    #[test]
    fn run_qa_captures_anti_patterns() {
        let prompt = PromptSpec {
            prompt_number: 1,
            description: "test".to_owned(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
        };
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+    let x = foo.unwrap();\n";

        let result = run_qa(diff, &prompt, 42);

        assert!(
            result
                .mechanical_issues
                .iter()
                .any(|i| i.message.contains("unwrap()"))
        );
    }

    #[test]
    fn run_qa_skips_llm_on_mechanical_failure() {
        let prompt = PromptSpec {
            prompt_number: 1,
            description: "test".to_owned(),
            acceptance_criteria: vec!["Implements feature correctly".to_owned()],
            blast_radius: vec!["src/allowed/".to_owned()],
        };
        let diff = "+++ b/src/outside/file.rs\n@@ -1 +1,2 @@\n+new line\n";

        let result = run_qa(diff, &prompt, 42);

        // WHY: Blast radius violation → skip LLM → semantic criteria skipped.
        assert_eq!(result.verdict, QaVerdict::Fail);
        assert!(result.cost_usd < f64::EPSILON);
        assert!(result.criteria_results.iter().any(|cr| {
            cr.evidence
                .contains("critical mechanical issues prevent evaluation")
        }));
    }
}
