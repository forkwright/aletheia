//! Core dispatch types ported from kanon's phronesis crate.
//!
//! These types define the vocabulary for dispatch orchestration: what to
//! dispatch, how sessions terminate, budget tracking, resume policies, and
//! quality assurance results.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Dispatch specification and results
// ---------------------------------------------------------------------------

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
}

/// Aggregate result of a dispatch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DispatchResult {
    /// Unique identifier for this dispatch run.
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

// ---------------------------------------------------------------------------
// Session outcome
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Budget tracking
// ---------------------------------------------------------------------------

/// Cost, turn, and duration limits for a dispatch run.
///
/// Uses atomic operations for thread-safe concurrent recording from multiple
/// sessions. Cost is stored as hundredths of a cent (10,000 per USD) to avoid
/// floating-point accumulation drift.
pub struct Budget {
    /// Maximum cost in USD. `None` means unlimited.
    pub max_cost_usd: Option<f64>,
    /// Maximum total LLM turns. `None` means unlimited.
    pub max_turns: Option<u32>,
    /// Maximum wall-clock duration in milliseconds. `None` means unlimited.
    pub max_duration_ms: Option<u64>,
    // WHY: atomic operations allow lock-free recording from concurrent sessions
    // without requiring a mutex around budget updates.
    current_cost_hundredths: AtomicU64,
    current_turns: AtomicU32,
    start_time: Instant,
}

impl Budget {
    /// Create a new budget with the given limits.
    #[must_use]
    pub fn new(
        max_cost_usd: Option<f64>,
        max_turns: Option<u32>,
        max_duration_ms: Option<u64>,
    ) -> Self {
        Self {
            max_cost_usd,
            max_turns,
            max_duration_ms,
            current_cost_hundredths: AtomicU64::new(0),
            current_turns: AtomicU32::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record cost and turns consumed by a session. Thread-safe.
    pub fn record(&self, cost_usd: f64, turns: u32) {
        // WHY: 10,000 hundredths per USD gives 0.01-cent precision without
        // floating-point accumulation drift across many sessions.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "cost values are small positive numbers; truncation at u64 max is unreachable"
        )]
        let hundredths = (cost_usd * 10_000.0) as u64;
        self.current_cost_hundredths
            .fetch_add(hundredths, Ordering::Relaxed);
        self.current_turns.fetch_add(turns, Ordering::Relaxed);
    }

    /// Check whether any budget limit has been exceeded.
    #[must_use]
    pub fn check(&self) -> BudgetStatus {
        if let Some(max_cost) = self.max_cost_usd {
            let current = self.current_cost_usd();
            if current >= max_cost {
                return BudgetStatus::Exceeded(format!(
                    "cost ${current:.2} >= limit ${max_cost:.2}"
                ));
            }
            if current >= max_cost * 0.8 {
                return BudgetStatus::Warning(format!(
                    "cost ${current:.2} approaching limit ${max_cost:.2}"
                ));
            }
        }

        if let Some(max_turns) = self.max_turns {
            let current = self.current_turns();
            if current >= max_turns {
                return BudgetStatus::Exceeded(format!("turns {current} >= limit {max_turns}"));
            }
            if current >= max_turns * 4 / 5 {
                return BudgetStatus::Warning(format!(
                    "turns {current} approaching limit {max_turns}"
                ));
            }
        }

        if let Some(max_ms) = self.max_duration_ms {
            let elapsed = self.elapsed_ms();
            if elapsed >= max_ms {
                return BudgetStatus::Exceeded(format!("duration {elapsed}ms >= limit {max_ms}ms"));
            }
        }

        BudgetStatus::Ok
    }

    /// Current total cost in USD.
    #[must_use]
    pub fn current_cost_usd(&self) -> f64 {
        let hundredths = self.current_cost_hundredths.load(Ordering::Relaxed);
        #[expect(
            clippy::cast_precision_loss,
            reason = "hundredths values are small enough that f64 precision is sufficient"
        )]
        let cost = hundredths as f64 / 10_000.0;
        cost
    }

    /// Current total turns consumed.
    #[must_use]
    pub fn current_turns(&self) -> u32 {
        self.current_turns.load(Ordering::Relaxed)
    }

    /// Elapsed wall-clock time in milliseconds since budget creation.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "elapsed time in ms fits u64 for any realistic dispatch duration"
        )]
        let ms = self.start_time.elapsed().as_millis() as u64;
        ms
    }

    /// Fraction of cost budget consumed (0.0 to 1.0+). Returns 0.0 if no cost limit.
    #[must_use]
    pub fn cost_fraction(&self) -> f64 {
        self.max_cost_usd
            .map_or(0.0, |max| self.current_cost_usd() / max)
    }

    /// Fraction of turn budget consumed (0.0 to 1.0+). Returns 0.0 if no turn limit.
    #[must_use]
    pub fn turn_fraction(&self) -> f64 {
        self.max_turns.map_or(0.0, |max| {
            #[expect(
                clippy::cast_lossless,
                reason = "u32 -> f64 is always lossless but clippy wants the annotation"
            )]
            let fraction = f64::from(self.current_turns()) / f64::from(max);
            fraction
        })
    }
}

impl std::fmt::Debug for Budget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Budget")
            .field("max_cost_usd", &self.max_cost_usd)
            .field("max_turns", &self.max_turns)
            .field("max_duration_ms", &self.max_duration_ms)
            .field("current_cost_usd", &self.current_cost_usd())
            .field("current_turns", &self.current_turns())
            .field("elapsed_ms", &self.elapsed_ms())
            .finish()
    }
}

/// Result of a budget check.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BudgetStatus {
    /// All limits within acceptable range.
    Ok,
    /// Approaching a limit (80%+ consumed).
    Warning(String),
    /// A limit has been exceeded.
    Exceeded(String),
}

// ---------------------------------------------------------------------------
// Resume policy
// ---------------------------------------------------------------------------

/// Multi-stage resume policy for stuck or stalled sessions.
///
/// Each stage has a turn budget and an escalating urgency message injected
/// into the session to redirect the agent's behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResumePolicy {
    /// Ordered stages of escalating intervention.
    pub stages: Vec<ResumeStage>,
}

/// A single stage in a resume escalation sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResumeStage {
    /// Maximum turns allowed in this stage before escalating.
    pub turn_budget: u32,
    /// Message injected into the session at this escalation level.
    pub message: String,
}

impl Default for ResumePolicy {
    fn default() -> Self {
        Self {
            stages: vec![
                ResumeStage {
                    turn_budget: 5,
                    message: "You seem stuck. Try a different approach.".to_owned(),
                },
                ResumeStage {
                    turn_budget: 3,
                    message: "Focus on the core requirement only. Skip anything non-essential."
                        .to_owned(),
                },
                ResumeStage {
                    turn_budget: 2,
                    message: "Final attempt. Commit what you have and report status.".to_owned(),
                },
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// QA types
// ---------------------------------------------------------------------------

/// Result of a QA evaluation against a pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Cost in USD for the LLM evaluation.
    pub cost_usd: f64,
    /// Timestamp when the evaluation completed.
    pub evaluated_at: Timestamp,
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
    fn budget_new_no_limits() {
        let budget = Budget::new(None, None, None);
        assert_eq!(budget.current_cost_usd(), 0.0);
        assert_eq!(budget.current_turns(), 0);
        assert_eq!(budget.check(), BudgetStatus::Ok);
    }

    #[test]
    fn budget_record_and_check() {
        let budget = Budget::new(Some(1.0), Some(10), None);
        budget.record(0.5, 3);
        assert!((budget.current_cost_usd() - 0.5).abs() < 0.001);
        assert_eq!(budget.current_turns(), 3);
        assert_eq!(budget.check(), BudgetStatus::Ok);
    }

    #[test]
    fn budget_warning_at_80_percent() {
        let budget = Budget::new(Some(1.0), None, None);
        budget.record(0.85, 0);
        assert!(matches!(budget.check(), BudgetStatus::Warning(_)));
    }

    #[test]
    fn budget_exceeded() {
        let budget = Budget::new(Some(1.0), None, None);
        budget.record(1.5, 0);
        assert!(matches!(budget.check(), BudgetStatus::Exceeded(_)));
    }

    #[test]
    fn budget_turn_exceeded() {
        let budget = Budget::new(None, Some(5), None);
        budget.record(0.0, 6);
        assert!(matches!(budget.check(), BudgetStatus::Exceeded(_)));
    }

    #[test]
    fn budget_cost_fraction() {
        let budget = Budget::new(Some(10.0), None, None);
        budget.record(3.0, 0);
        assert!((budget.cost_fraction() - 0.3).abs() < 0.01);
    }

    #[test]
    fn budget_turn_fraction() {
        let budget = Budget::new(None, Some(20), None);
        budget.record(0.0, 5);
        assert!((budget.turn_fraction() - 0.25).abs() < 0.01);
    }

    #[test]
    fn budget_fractions_zero_without_limits() {
        let budget = Budget::new(None, None, None);
        assert_eq!(budget.cost_fraction(), 0.0);
        assert_eq!(budget.turn_fraction(), 0.0);
    }

    #[test]
    fn budget_concurrent_recording() {
        let budget = Budget::new(Some(100.0), Some(1000), None);
        budget.record(1.0, 10);
        budget.record(2.0, 20);
        budget.record(0.5, 5);
        assert!((budget.current_cost_usd() - 3.5).abs() < 0.001);
        assert_eq!(budget.current_turns(), 35);
    }

    #[test]
    fn budget_debug_format() {
        let budget = Budget::new(Some(5.0), Some(100), None);
        let debug = format!("{budget:?}");
        assert!(debug.contains("Budget"));
        assert!(debug.contains("max_cost_usd"));
    }

    #[test]
    fn resume_policy_default_has_three_stages() {
        let policy = ResumePolicy::default();
        assert_eq!(policy.stages.len(), 3);
        assert!(policy.stages[0].turn_budget > policy.stages[2].turn_budget);
    }

    #[test]
    fn dispatch_spec_roundtrip() {
        let spec = DispatchSpec {
            prompt_numbers: vec![1, 2, 3],
            project: "test-project".to_owned(),
            dag_ref: None,
            max_parallel: Some(2),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: DispatchSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt_numbers, vec![1, 2, 3]);
        assert_eq!(deserialized.max_parallel, Some(2));
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
        };
        let json = serde_json::to_string(&outcome).unwrap();
        let deserialized: SessionOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt_number, 1);
        assert_eq!(deserialized.status, SessionStatus::Success);
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
            cost_usd: 0.03,
            evaluated_at: Timestamp::now(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: QaResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.verdict, QaVerdict::Pass);
        assert_eq!(deserialized.criteria_results.len(), 1);
    }

    #[test]
    fn session_status_equality() {
        assert_eq!(SessionStatus::Success, SessionStatus::Success);
        assert_ne!(SessionStatus::Success, SessionStatus::Failed);
    }
}
