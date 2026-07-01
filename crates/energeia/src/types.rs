//! Core dispatch types for orchestration vocabulary.
//!
//! Defines what to dispatch, how sessions terminate, and quality assurance
//! results. Budget tracking and resume policies are in their dedicated modules
//! and re-exported here for convenience.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

pub use crate::budget::{Budget, BudgetStatus};
pub use crate::resume::{ResumePolicy, ResumeStage};

// ── Dispatch specification and results ──

/// What to dispatch: a set of prompt numbers with optional DAG constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DispatchSpec {
    /// Prompt numbers to execute (may be a subset of the full DAG).
    pub prompt_numbers: Vec<u32>,
    /// Project identifier this dispatch belongs to.
    pub project: String,
    /// Optional reference to a prompt DAG for dependency ordering.
    pub dag_ref: Option<String>,
    /// Maximum parallelism (simultaneous sessions). `None` means unlimited.
    pub max_parallel: Option<u32>,
    /// Maximum turns per initial session. `None` delegates to engine defaults.
    #[serde(default)]
    pub max_turns: Option<u32>,
    /// Maximum total cost in USD for this dispatch. `None` uses orchestrator defaults.
    #[serde(default)]
    pub budget_usd: Option<f64>,
}

impl DispatchSpec {
    /// Create a dispatch spec for a set of prompts in a project.
    ///
    /// All optional fields default to `None` (no DAG ref, no parallelism limit).
    #[must_use]
    pub fn new(project: String, prompt_numbers: Vec<u32>) -> Self {
        Self {
            prompt_numbers,
            project,
            dag_ref: None,
            max_parallel: None,
            max_turns: None,
            budget_usd: None,
        }
    }

    /// Create a dispatch spec with all fields specified.
    #[must_use]
    pub fn with_options(
        project: String,
        prompt_numbers: Vec<u32>,
        dag_ref: Option<String>,
        max_parallel: Option<u32>,
    ) -> Self {
        Self {
            prompt_numbers,
            project,
            dag_ref,
            max_parallel,
            max_turns: None,
            budget_usd: None,
        }
    }

    /// Set the per-session turn limit.
    #[must_use]
    pub fn with_max_turns(mut self, max_turns: Option<u32>) -> Self {
        self.max_turns = max_turns;
        self
    }

    /// Set the total cost budget in USD.
    #[must_use]
    pub fn with_budget_usd(mut self, budget_usd: Option<f64>) -> Self {
        self.budget_usd = budget_usd;
        self
    }
}

/// Aggregate result of a dispatch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DispatchResult {
    /// Unique identifier for this dispatch run.
    // kanon:ignore RUST/primitive-for-domain-id — public result type; changing to newtype would be a breaking API change across crates
    pub dispatch_id: String,
    /// Per-prompt outcomes in execution order.
    pub outcomes: Vec<SessionOutcome>,
    /// Total cost across all sessions in USD.
    pub total_cost_usd: f64,
    /// Wall-clock duration of the entire dispatch.
    pub duration_ms: u64,
    /// Whether the dispatch was aborted before completing all prompts.
    pub aborted: bool,
    /// Timestamp when the dispatch completed.
    pub completed_at: Timestamp,
}

// ── Session outcome ──

/// Result of executing a single prompt in a dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionOutcome {
    /// The prompt number that was executed.
    pub prompt_number: u32,
    /// Terminal status of the session.
    pub status: SessionStatus,
    /// Agent SDK session identifier, if one was created.
    pub session_id: Option<String>,
    /// Total cost in USD for this session (including resumes).
    pub cost_usd: f64,
    /// Total LLM turns consumed.
    pub num_turns: u32,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Number of times the session was resumed via health checks.
    pub resume_count: u32,
    /// Pull request URL if the session produced one.
    pub pr_url: Option<String>,
    /// Error message if the session failed.
    pub error: Option<String>,
    /// Typed reason bucket for failed sessions.
    ///
    /// `None` means the session was successful or the terminal status already
    /// carries the non-provider reason, such as an operator abort.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<FailureClass>,
    /// LLM model used for this session (e.g., "claude-3-5-sonnet").
    ///
    /// This is `None` if the model could not be determined from the session.
    pub model: Option<String>,
    /// Blast radius paths from the prompt spec.
    ///
    /// Used for cost attribution to specific modules/features.
    pub blast_radius: Vec<String>,
    /// Number of QA-driven corrective attempts made for this prompt before
    /// this outcome. `0` means this is the original execution.
    #[serde(default)]
    pub corrective_attempts: u32,
    /// Tokens read from the prompt cache on this session.
    #[serde(default)]
    pub cache_hit_tokens: u64,
    /// Tokens written to the prompt cache on this session.
    #[serde(default)]
    pub cache_miss_tokens: u64,
    /// Parsed structured output from this session, when the prompt declared
    /// an output format and the final result was valid JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<serde_json::Value>,
}

/// Failure bucket preserved for routing and health telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FailureClass {
    /// The provider/model returned a normal completion that did not satisfy the task.
    Provider,
    /// Authentication or authorization failed.
    Auth,
    /// Network, DNS, connection, or transport failure.
    Network,
    /// Session or backend timed out.
    Timeout,
    /// Provider-side rate limit or quota exhaustion.
    RateLimit,
    /// Worker process, task join, or runtime/protocol failure.
    WorkerRuntime,
}

impl FailureClass {
    /// Whether this failure should be excluded from model/provider quality scores.
    #[must_use]
    pub fn is_infrastructure(self) -> bool {
        matches!(
            self,
            Self::Auth | Self::Network | Self::Timeout | Self::RateLimit | Self::WorkerRuntime
        )
    }
}

impl std::fmt::Display for FailureClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider => write!(f, "provider"),
            Self::Auth => write!(f, "auth"),
            Self::Network => write!(f, "network"),
            Self::Timeout => write!(f, "timeout"),
            Self::RateLimit => write!(f, "rate_limit"),
            Self::WorkerRuntime => write!(f, "worker_runtime"),
        }
    }
}

/// Terminal status of a dispatched session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionStatus {
    /// Session completed its task successfully.
    Success,
    /// Session failed to complete its task.
    Failed,
    /// Session became stuck (health escalation reached terminal level).
    Stuck,
    /// Session was aborted via cancellation token.
    Aborted,
    /// Session exceeded its budget allocation.
    BudgetExceeded,
    /// Session was skipped (dependency failed or dispatch aborted).
    Skipped,
    /// Infrastructure failure (zero turns, short duration — auth/network issues).
    InfraFailure,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::Failed => write!(f, "failed"),
            Self::Stuck => write!(f, "stuck"),
            Self::Aborted => write!(f, "aborted"),
            Self::BudgetExceeded => write!(f, "budget_exceeded"),
            Self::Skipped => write!(f, "skipped"),
            Self::InfraFailure => write!(f, "infra_failure"),
        }
    }
}

// ── QA types ──

/// Result of a QA evaluation against a pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "QaResultRaw")]
#[non_exhaustive]
pub struct QaResult {
    /// The prompt number that produced the PR.
    pub prompt_number: u32,
    /// Pull request number evaluated.
    pub pr_number: u64,
    /// Overall verdict.
    pub verdict: QaVerdict,
    /// Per-criterion evaluation results.
    pub criteria_results: Vec<CriterionResult>,
    /// Mechanical issues found in the diff.
    pub mechanical_issues: Vec<MechanicalIssue>,
    /// Human-readable reasons for the verdict, derived from failed criteria
    /// and mechanical issues.
    pub reasons: Vec<String>,
    /// Cost in USD for the LLM evaluation.
    pub cost_usd: f64,
    /// Timestamp when the evaluation completed.
    pub evaluated_at: Timestamp,
    /// Whether semantic (LLM-based) evaluation was included in this result.
    ///
    /// When `false`, the verdict reflects mechanical checks only and the
    /// operator should be aware that semantic criteria were not evaluated.
    pub semantic_evaluated: bool,
}

/// Raw deserialization type for [`QaResult`].
///
/// The `semantic_evaluated` field defaults to `false` for backward
/// compatibility with serialized results that predate this field.
#[derive(Debug, Clone, Deserialize)]
struct QaResultRaw {
    prompt_number: u32,
    pr_number: u64,
    verdict: QaVerdict,
    criteria_results: Vec<CriterionResult>,
    mechanical_issues: Vec<MechanicalIssue>,
    #[serde(default)]
    reasons: Vec<String>,
    cost_usd: f64,
    evaluated_at: Timestamp,
    #[serde(default)]
    semantic_evaluated: bool,
}

impl From<QaResultRaw> for QaResult {
    fn from(raw: QaResultRaw) -> Self {
        Self {
            prompt_number: raw.prompt_number,
            pr_number: raw.pr_number,
            verdict: raw.verdict,
            criteria_results: raw.criteria_results,
            mechanical_issues: raw.mechanical_issues,
            reasons: raw.reasons,
            cost_usd: raw.cost_usd,
            evaluated_at: raw.evaluated_at,
            semantic_evaluated: raw.semantic_evaluated,
        }
    }
}

impl QaResult {
    /// Create a QA result.
    ///
    /// Intended for test harnesses and mock QA gates that need to produce
    /// results without running a full evaluation pipeline.
    #[must_use]
    pub fn new(
        prompt_number: u32,
        pr_number: u64,
        verdict: QaVerdict,
        criteria_results: Vec<CriterionResult>,
        mechanical_issues: Vec<MechanicalIssue>,
        reasons: Vec<String>,
        cost_usd: f64,
        evaluated_at: Timestamp,
        semantic_evaluated: bool,
    ) -> Self {
        Self {
            prompt_number,
            pr_number,
            verdict,
            criteria_results,
            mechanical_issues,
            reasons,
            cost_usd,
            evaluated_at,
            semantic_evaluated,
        }
    }
}

/// Overall quality verdict for a PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum QaVerdict {
    /// All criteria pass, no blocking mechanical issues.
    Pass,
    /// Some criteria fail but the PR is partially acceptable.
    Partial,
    /// Critical criteria fail or blocking mechanical issues found.
    Fail,
}

impl std::fmt::Display for QaVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => write!(f, "pass"),
            Self::Partial => write!(f, "partial"),
            Self::Fail => write!(f, "fail"),
        }
    }
}

/// Evaluation result for a single acceptance criterion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CriterionResult {
    /// The acceptance criterion text.
    pub criterion: String,
    /// Whether this criterion was mechanically or semantically evaluated.
    pub classification: CriterionType,
    /// Whether the criterion passed.
    pub passed: bool,
    /// Supporting evidence from the diff or evaluation.
    pub evidence: String,
}

/// How a criterion was evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CriterionType {
    /// Checkable by static analysis (lint, format, blast radius).
    Mechanical,
    /// Requires LLM evaluation of intent and correctness.
    Semantic,
}

/// A mechanical issue found in a diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MechanicalIssue {
    /// Category of the issue.
    pub kind: MechanicalIssueKind,
    /// Human-readable description.
    pub message: String,
    /// Optional additional details (file paths, line numbers, etc.).
    pub details: Option<String>,
}

/// Categories of mechanical issues detectable without LLM evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MechanicalIssueKind {
    /// Changes touch files outside the declared blast radius.
    BlastRadiusViolation,
    /// Known anti-pattern detected in the diff.
    AntiPattern,
    /// Lint check failure.
    LintViolation,
    /// Code formatting violation.
    FormatViolation,
}

impl std::fmt::Display for MechanicalIssueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlastRadiusViolation => write!(f, "blast_radius_violation"),
            Self::AntiPattern => write!(f, "anti_pattern"),
            Self::LintViolation => write!(f, "lint_violation"),
            Self::FormatViolation => write!(f, "format_violation"),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn session_status_display() {
        assert_eq!(SessionStatus::Success.to_string(), "success");
        assert_eq!(SessionStatus::Failed.to_string(), "failed");
        assert_eq!(SessionStatus::Stuck.to_string(), "stuck");
        assert_eq!(SessionStatus::Aborted.to_string(), "aborted");
        assert_eq!(SessionStatus::BudgetExceeded.to_string(), "budget_exceeded");
        assert_eq!(SessionStatus::Skipped.to_string(), "skipped");
        assert_eq!(SessionStatus::InfraFailure.to_string(), "infra_failure");
    }

    #[test]
    fn failure_class_display_and_infra_flag() {
        assert_eq!(FailureClass::Provider.to_string(), "provider");
        assert_eq!(FailureClass::Auth.to_string(), "auth");
        assert_eq!(FailureClass::Network.to_string(), "network");
        assert_eq!(FailureClass::Timeout.to_string(), "timeout");
        assert_eq!(FailureClass::RateLimit.to_string(), "rate_limit");
        assert_eq!(FailureClass::WorkerRuntime.to_string(), "worker_runtime");
        assert!(!FailureClass::Provider.is_infrastructure());
        assert!(FailureClass::Auth.is_infrastructure());
    }

    #[test]
    fn qa_verdict_display() {
        assert_eq!(QaVerdict::Pass.to_string(), "pass");
        assert_eq!(QaVerdict::Partial.to_string(), "partial");
        assert_eq!(QaVerdict::Fail.to_string(), "fail");
    }

    #[test]
    fn mechanical_issue_kind_display() {
        assert_eq!(
            MechanicalIssueKind::BlastRadiusViolation.to_string(),
            "blast_radius_violation"
        );
        assert_eq!(MechanicalIssueKind::AntiPattern.to_string(), "anti_pattern");
    }

    #[test]
    fn dispatch_spec_roundtrip() {
        let spec = DispatchSpec {
            prompt_numbers: vec![1, 2, 3],
            project: "test-project".to_owned(),
            dag_ref: None,
            max_parallel: Some(2),
            max_turns: Some(7),
            budget_usd: Some(12.5),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: DispatchSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt_numbers, vec![1, 2, 3]);
        assert_eq!(deserialized.max_parallel, Some(2));
        assert_eq!(deserialized.max_turns, Some(7));
        assert_eq!(deserialized.budget_usd, Some(12.5));
    }

    #[test]
    fn session_outcome_roundtrip() {
        let outcome = SessionOutcome {
            prompt_number: 1,
            status: SessionStatus::Success,
            session_id: Some("sess-123".to_owned()),
            cost_usd: 0.42,
            num_turns: 15,
            duration_ms: 30_000,
            resume_count: 0,
            pr_url: Some("https://github.com/acme/repo/pull/42".to_owned()),
            error: None,
            failure_class: None,
            model: Some("claude-3-5-sonnet".to_owned()),
            blast_radius: vec!["crates/foo/".to_owned()],
            corrective_attempts: 0,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            structured_output: None,
        };
        let json = serde_json::to_string(&outcome).unwrap();
        let deserialized: SessionOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt_number, 1);
        assert_eq!(deserialized.status, SessionStatus::Success);
        assert_eq!(deserialized.model.as_deref(), Some("claude-3-5-sonnet"));
        assert_eq!(deserialized.blast_radius, vec!["crates/foo/"]);
    }

    #[test]
    fn qa_result_roundtrip() {
        let result = QaResult {
            prompt_number: 1,
            pr_number: 42,
            verdict: QaVerdict::Pass,
            criteria_results: vec![CriterionResult {
                criterion: "tests pass".to_owned(),
                classification: CriterionType::Mechanical,
                passed: true,
                evidence: "CI green".to_owned(),
            }],
            mechanical_issues: vec![],
            reasons: vec![],
            cost_usd: 0.03,
            evaluated_at: Timestamp::now(),
            semantic_evaluated: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: QaResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.verdict, QaVerdict::Pass);
        assert!(deserialized.semantic_evaluated);
        assert_eq!(deserialized.criteria_results.len(), 1);
    }

    #[test]
    fn session_status_equality() {
        let parsed: SessionStatus = serde_json::from_str("\"Success\"").unwrap();

        assert_eq!(parsed, SessionStatus::Success);
        assert_ne!(parsed, SessionStatus::Failed);
    }
}
