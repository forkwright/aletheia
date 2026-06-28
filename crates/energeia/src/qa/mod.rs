//! Quality assurance evaluation engine for dispatch output.
//!
//! Orchestrates the QA flow:
//! 1. Run mechanical pre-screening (blast radius, anti-patterns)
//! 2. Classify acceptance criteria (mechanical vs semantic)
//! 3. Auto-pass mechanical criteria if no mechanical issues found
//! 4. Run LLM evaluation on semantic criteria via hermeneus (when provider available)
//! 5. Determine verdict: Pass / Partial / Fail
//! 6. Capture training data as a lesson
//! 7. If Partial/Fail, generate corrective prompt
//!
//! The [`QaGate`] trait separates mechanical pre-screening (fast, no LLM cost)
//! from semantic evaluation (uses hermeneus for LLM-based assessment).
//!
//! When no LLM provider is available, [`run_qa`] degrades gracefully to
//! mechanical-only evaluation and sets `semantic_evaluated = false` on the
//! result so the operator knows the gate is incomplete.

use std::future::Future;
use std::pin::Pin;

use hermeneus::provider::LlmProvider;
use hermeneus::types::{CompletionRequest, Content, ContentBlock, Message, Role};
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

/// Abstraction over PR diff fetching.
///
/// Implementations fetch the unified diff for a pull request from a forge
/// (`GitHub`, `GitLab`, etc.) so the QA gate can evaluate real changes.
pub trait DiffProvider: Send + Sync {
    /// Fetch the unified diff for the given PR URL.
    fn fetch_diff<'a>(
        &'a self,
        pr_url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;
}

/// Specification of a prompt for QA evaluation.
///
/// Contains the acceptance criteria and blast radius constraints that the
/// QA gate evaluates against.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "PromptSpecRaw")]
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

/// Raw deserialization type for [`PromptSpec`].
#[derive(Debug, Clone, Deserialize)]
struct PromptSpecRaw {
    prompt_number: u32,
    description: String,
    acceptance_criteria: Vec<String>,
    blast_radius: Vec<String>,
}

impl From<PromptSpecRaw> for PromptSpec {
    fn from(raw: PromptSpecRaw) -> Self {
        Self {
            prompt_number: raw.prompt_number,
            description: raw.description,
            acceptance_criteria: raw.acceptance_criteria,
            blast_radius: raw.blast_radius,
        }
    }
}

impl PromptSpec {
    /// Create a minimal prompt spec for QA evaluation.
    ///
    /// All optional fields (acceptance criteria, blast radius) default to empty.
    #[must_use]
    pub fn new(prompt_number: u32, description: String) -> Self {
        Self {
            prompt_number,
            description,
            acceptance_criteria: Vec::new(),
            blast_radius: Vec::new(),
        }
    }
}

fn waiver_provenance(criterion: &str) -> Option<&str> {
    let trimmed = criterion.trim();
    let lower = trimmed.to_ascii_lowercase();
    for prefix in ["qa-waiver:", "waiver:"] {
        if lower.starts_with(prefix) {
            return trimmed.get(prefix.len()..).map(str::trim);
        }
    }
    None
}

fn split_waivers(criteria: &[String]) -> (Vec<String>, Vec<CriterionResult>, bool) {
    let mut evaluable = Vec::new();
    let mut waiver_results = Vec::new();
    let mut has_valid_waiver = false;

    for criterion in criteria {
        if let Some(provenance) = waiver_provenance(criterion) {
            if provenance.is_empty() {
                waiver_results.push(CriterionResult {
                    criterion: "QA waiver".to_owned(),
                    classification: CriterionType::Mechanical,
                    passed: false,
                    evidence: "qa-waiver requires non-empty provenance".to_owned(),
                });
            } else {
                has_valid_waiver = true;
                waiver_results.push(CriterionResult {
                    criterion: "QA waiver".to_owned(),
                    classification: CriterionType::Mechanical,
                    passed: true,
                    evidence: format!("waived with provenance: {provenance}"),
                });
            }
        } else if !criterion.trim().is_empty() {
            evaluable.push(criterion.clone());
        }
    }

    if evaluable.is_empty() && !has_valid_waiver {
        waiver_results.push(CriterionResult {
            criterion: "acceptance criteria".to_owned(),
            classification: CriterionType::Semantic,
            passed: false,
            evidence: "missing acceptance criteria; attach qa-waiver: <provenance> to waive"
                .to_owned(),
        });
    }

    (evaluable, waiver_results, has_valid_waiver)
}

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
/// * `llm` - Optional LLM provider for semantic evaluation. When `None`,
///   semantic criteria are skipped with a warning and the verdict reflects
///   mechanical checks only.
///
/// # Semantic evaluation
///
/// When an LLM provider is supplied, semantic criteria are evaluated via
/// hermeneus. When unavailable (`None`), the verdict indicates that
/// semantic evaluation was not included so the operator knows the gate
/// is mechanical-only.
pub async fn run_qa(
    diff: &str,
    prompt: &PromptSpec,
    pr_number: u64,
    llm: Option<&dyn LlmProvider>,
) -> QaResult {
    tracing::info!(
        prompt_number = prompt.prompt_number,
        pr_number,
        semantic_eval = llm.is_some(),
        "starting QA evaluation"
    );

    let mechanical_issues = mechanical::mechanical_check(diff, prompt);

    if !mechanical_issues.is_empty() {
        tracing::warn!(count = mechanical_issues.len(), "mechanical issues found");
    }

    let (criteria_to_evaluate, waiver_results, has_valid_waiver) =
        split_waivers(&prompt.acceptance_criteria);
    let missing_acceptance_gate = criteria_to_evaluate.is_empty() && !has_valid_waiver;
    let classified = semantic::classify_criteria(&criteria_to_evaluate);

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

    // WHY: Skip LLM evaluation if there are critical mechanical issues (save cost).
    let semantic_criteria: Vec<(String, CriterionType)> = classified
        .iter()
        .filter(|(_, ct)| *ct == CriterionType::Semantic)
        .cloned()
        .collect();

    let (semantic_results, cost_usd, semantic_evaluated) = evaluate_semantic_criteria(
        &semantic_criteria,
        &mechanical_issues,
        &prompt.description,
        diff,
        llm,
    )
    .await;

    let mut all_results = waiver_results;
    all_results.extend(mechanical_results);
    all_results.extend(semantic_results);

    let mut qa_verdict = verdict::determine_verdict(&all_results, &mechanical_issues);
    if missing_acceptance_gate && mechanical_issues.is_empty() {
        qa_verdict = crate::types::QaVerdict::Partial;
    }

    let reasons = build_reasons(&all_results, &mechanical_issues);

    let qa_result = QaResult {
        prompt_number: prompt.prompt_number,
        pr_number,
        verdict: qa_verdict,
        criteria_results: all_results,
        mechanical_issues,
        reasons,
        cost_usd,
        evaluated_at: Timestamp::now(),
        semantic_evaluated,
    };

    tracing::info!(
        verdict = %qa_result.verdict,
        cost_usd = qa_result.cost_usd,
        semantic_evaluated = qa_result.semantic_evaluated,
        "QA evaluation complete"
    );

    qa_result
}

/// Evaluate semantic criteria via LLM or degrade gracefully.
///
/// Returns `(results, cost_usd, semantic_evaluated)`.
async fn evaluate_semantic_criteria(
    criteria: &[(String, CriterionType)],
    mechanical_issues: &[MechanicalIssue],
    description: &str,
    diff: &str,
    llm: Option<&dyn LlmProvider>,
) -> (Vec<CriterionResult>, f64, bool) {
    if verdict::has_critical_mechanical_issues(mechanical_issues) {
        tracing::info!("skipping LLM evaluation due to critical mechanical issues");
        let results = criteria
            .iter()
            .map(|(text, ct)| CriterionResult {
                criterion: text.clone(),
                classification: *ct,
                passed: false,
                evidence:
                    "skipped: critical mechanical issues prevent evaluation (provenance: mechanical pre-screen)"
                        .to_owned(),
            })
            .collect();
        return (results, 0.0, false);
    }

    if criteria.is_empty() {
        return (Vec::new(), 0.0, true);
    }

    let Some(provider) = llm else {
        tracing::warn!("no LLM provider — semantic criteria skipped (mechanical-only gate)");
        let results = criteria
            .iter()
            .map(|(text, ct)| CriterionResult {
                criterion: text.clone(),
                classification: *ct,
                passed: false,
                evidence:
                    "no LLM provider available; semantic evaluation skipped (provenance: qa/no-llm-provider)"
                        .to_owned(),
            })
            .collect();
        return (results, 0.0, false);
    };

    let qa_prompt_text = semantic::build_qa_prompt(description, criteria, diff);

    let request = CompletionRequest {
        model: String::new(), // WHY: empty string lets the provider use its default model
        messages: vec![Message {
            role: Role::User,
            content: Content::Text(qa_prompt_text),
            cache_breakpoint: false,
        }],
        ..CompletionRequest::default()
    };

    match provider.complete(&request).await {
        Ok(response) => {
            let response_text: String = response
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            let results = semantic::parse_qa_response(&response_text, criteria);
            // WHY: Cost estimation requires model pricing data which lives in
            // hermeneus::models. The provider returns raw token counts; callers
            // with pricing context can compute cost from response.usage. We
            // report 0.0 here and let the orchestrator handle cost attribution.
            (results, 0.0, true)
        }
        Err(e) => {
            tracing::warn!(error = %e, "LLM evaluation failed — semantic criteria marked as unevaluated");
            let results = criteria
                .iter()
                .map(|(text, ct)| CriterionResult {
                    criterion: text.clone(),
                    classification: *ct,
                    passed: false,
                    evidence: format!("LLM evaluation failed (provenance: qa/llm-error): {e}"),
                })
                .collect();
            (results, 0.0, false)
        }
    }
}

/// Build human-readable reasons from failed criteria and mechanical issues.
#[must_use]
fn build_reasons(
    criteria: &[CriterionResult],
    mechanical_issues: &[MechanicalIssue],
) -> Vec<String> {
    let mut reasons = Vec::new();

    for issue in mechanical_issues {
        let mut reason = issue.message.clone();
        if let Some(ref details) = issue.details {
            reason.push_str(": ");
            reason.push_str(details);
        }
        reasons.push(reason);
    }

    for cr in criteria.iter().filter(|c| !c.passed) {
        reasons.push(format!("{}: {}", cr.criterion, cr.evidence));
    }

    reasons
}

/// Record a QA evaluation result as a lesson in the store.
///
/// Captures the verdict, criteria details, and cost for training data.
/// This is best-effort: failures are logged but do not propagate.
#[cfg(feature = "storage-fjall")]
pub fn record_training_data(
    store: &crate::store::EnergeiaStore,
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

    #[tokio::test]
    async fn run_qa_all_mechanical_pass() {
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

        let result = run_qa(diff, &prompt, 42, None).await;

        // NOTE: No mechanical issues, all criteria are mechanical keywords
        // so they auto-pass. semantic_evaluated is true because there are no
        // semantic criteria to evaluate (vacuously complete).
        assert_eq!(result.verdict, QaVerdict::Pass);
        assert!(result.mechanical_issues.is_empty());
        assert!(result.reasons.is_empty());
        assert!(result.semantic_evaluated);
    }

    #[tokio::test]
    async fn run_qa_blast_radius_violation_fails() {
        let prompt = PromptSpec {
            prompt_number: 1,
            description: "scoped change".to_owned(),
            acceptance_criteria: vec!["feature works".to_owned()],
            blast_radius: vec!["src/allowed/".to_owned()],
        };
        let diff = "+++ b/src/outside/file.rs\n@@ -1 +1,2 @@\n+new line\n";

        let result = run_qa(diff, &prompt, 42, None).await;

        assert_eq!(result.verdict, QaVerdict::Fail);
        assert!(!result.mechanical_issues.is_empty());
    }

    #[tokio::test]
    async fn run_qa_captures_anti_patterns() {
        let prompt = PromptSpec {
            prompt_number: 1,
            description: "test".to_owned(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
        };
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+    let x = foo.unwrap();\n";

        let result = run_qa(diff, &prompt, 42, None).await;

        assert!(
            result
                .mechanical_issues
                .iter()
                .any(|i| i.message.contains("unwrap()"))
        );
    }

    #[tokio::test]
    async fn run_qa_empty_acceptance_criteria_needs_review() {
        let prompt = PromptSpec::new(1, "test".to_owned());
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+pub fn foo() {}\n";

        let result = run_qa(diff, &prompt, 42, None).await;

        assert_eq!(result.verdict, QaVerdict::Partial);
        assert!(result.reasons.iter().any(|reason| {
            reason.contains("missing acceptance criteria")
                && reason.contains("qa-waiver: <provenance>")
        }));
    }

    #[tokio::test]
    async fn run_qa_valid_criteria_waiver_passes_with_provenance() {
        let mut prompt = PromptSpec::new(1, "test".to_owned());
        prompt.acceptance_criteria = vec!["qa-waiver: approved by alice in issue #123".to_owned()];
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+pub fn foo() {}\n";

        let result = run_qa(diff, &prompt, 42, None).await;

        assert_eq!(result.verdict, QaVerdict::Pass);
        assert!(result.criteria_results.iter().any(|criterion| {
            criterion.passed
                && criterion
                    .evidence
                    .contains("approved by alice in issue #123")
        }));
    }

    #[tokio::test]
    async fn run_qa_waiver_without_provenance_needs_review() {
        let mut prompt = PromptSpec::new(1, "test".to_owned());
        prompt.acceptance_criteria = vec!["qa-waiver:".to_owned()];
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+pub fn foo() {}\n";

        let result = run_qa(diff, &prompt, 42, None).await;

        assert_eq!(result.verdict, QaVerdict::Partial);
        assert!(
            result
                .reasons
                .iter()
                .any(|reason| reason.contains("requires non-empty provenance"))
        );
    }

    #[tokio::test]
    async fn run_qa_skips_llm_on_mechanical_failure() {
        let prompt = PromptSpec {
            prompt_number: 1,
            description: "test".to_owned(),
            acceptance_criteria: vec!["Implements feature correctly".to_owned()],
            blast_radius: vec!["src/allowed/".to_owned()],
        };
        let diff = "+++ b/src/outside/file.rs\n@@ -1 +1,2 @@\n+new line\n";

        let result = run_qa(diff, &prompt, 42, None).await;

        // WHY: Blast radius violation -> skip LLM -> semantic criteria skipped.
        assert_eq!(result.verdict, QaVerdict::Fail);
        assert!(result.cost_usd < f64::EPSILON);
        assert!(!result.semantic_evaluated);
        assert!(result.criteria_results.iter().any(|cr| {
            cr.evidence
                .contains("critical mechanical issues prevent evaluation")
                && cr.evidence.contains("provenance: mechanical pre-screen")
        }));
    }

    #[tokio::test]
    async fn run_qa_indicates_no_semantic_eval_without_llm() {
        let prompt = PromptSpec {
            prompt_number: 1,
            description: "test".to_owned(),
            acceptance_criteria: vec!["Implements feature correctly".to_owned()],
            blast_radius: vec![],
        };
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+pub fn foo() {}\n";

        let result = run_qa(diff, &prompt, 42, None).await;

        // WHY: No LLM provider -> semantic criteria fail with clear evidence.
        assert!(!result.semantic_evaluated);
        assert!(result.criteria_results.iter().any(|cr| {
            cr.evidence.contains("no LLM provider available")
                && cr.evidence.contains("provenance: qa/no-llm-provider")
        }));
    }
}
