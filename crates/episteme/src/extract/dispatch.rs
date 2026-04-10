//! Dispatch pattern detection and scoring for the knowledge pipeline.
//!
//! Extracts learning patterns from dispatch history, QA results, and steward
//! cycles. Patterns are represented as facts that feed into episteme's
//! existing extraction pipeline.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// A detected pattern from dispatch/QA/steward history.
///
/// WHY: Recurring patterns (e.g., "prompt N always needs 2 resumes",
/// "crate X CI fails on clippy") should be captured as knowledge facts
/// so the system can learn from operational history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DispatchPattern {
    /// What kind of pattern was detected.
    pub pattern_type: PatternType,
    /// Human-readable description of the pattern.
    pub description: String,
    /// How severe/important this pattern is.
    pub severity: PatternSeverity,
    /// Number of occurrences that triggered detection.
    pub occurrence_count: u32,
    /// Project this pattern was detected in.
    pub project: String,
    /// Optional crate or module scope.
    pub scope: Option<String>,
}

/// Classification of detected patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PatternType {
    /// Same CI failure recurring across multiple dispatches.
    RecurringCiFailure,
    /// A prompt consistently needs multiple resumes.
    HighResumeRate,
    /// A crate consistently produces lint/format issues.
    CrateQualityDrift,
    /// Blast radius violations in a specific area.
    BlastRadiusHotspot,
    /// Cost anomaly (significantly above average).
    CostAnomaly,
    /// Merge conflicts recurring in the same files.
    ConflictHotspot,
}

impl fmt::Display for PatternType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecurringCiFailure => f.write_str("recurring-ci-failure"),
            Self::HighResumeRate => f.write_str("high-resume-rate"),
            Self::CrateQualityDrift => f.write_str("crate-quality-drift"),
            Self::BlastRadiusHotspot => f.write_str("blast-radius-hotspot"),
            Self::CostAnomaly => f.write_str("cost-anomaly"),
            Self::ConflictHotspot => f.write_str("conflict-hotspot"),
        }
    }
}

/// Severity level for detected patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PatternSeverity {
    /// Informational — worth tracking but no action needed.
    Info,
    /// Warning — may indicate a developing problem.
    Warning,
    /// Critical — requires attention or intervention.
    Critical,
}

impl fmt::Display for PatternSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => f.write_str("info"),
            Self::Warning => f.write_str("warning"),
            Self::Critical => f.write_str("critical"),
        }
    }
}

/// Quality score for a single prompt execution.
///
/// WHY: Tracking per-prompt quality metrics enables trend detection
/// and calibration of dispatch parameters (budget, resume policy).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PromptScore {
    /// The prompt number that was scored.
    pub prompt_number: u32,
    /// Whether the prompt completed without any resumes.
    pub one_shot: bool,
    /// Number of resumes needed.
    pub resume_count: u32,
    /// Whether CI passed on first push.
    pub ci_first_try: bool,
    /// Whether QA passed without corrective prompts.
    pub qa_pass: bool,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Total duration in milliseconds.
    pub duration_ms: u64,
    /// Overall quality grade.
    pub quality_grade: Grade,
}

/// Quality grade for a prompt execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Grade {
    /// One-shot success with CI pass on first try.
    A,
    /// One resume or one CI fix needed.
    B,
    /// Multiple resumes or fixes needed.
    C,
    /// Stuck or failed entirely.
    F,
}

impl fmt::Display for Grade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::A => f.write_str("A"),
            Self::B => f.write_str("B"),
            Self::C => f.write_str("C"),
            Self::F => f.write_str("F"),
        }
    }
}

/// Aggregate quality scores for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProjectScores {
    /// Total number of prompts scored.
    pub total_prompts: usize,
    /// Percentage of one-shot completions (0.0 to 1.0).
    pub one_shot_rate: f64,
    /// Percentage of CI first-try passes (0.0 to 1.0).
    pub ci_first_try_rate: f64,
    /// Percentage of QA passes (0.0 to 1.0).
    pub qa_pass_rate: f64,
    /// Average cost in USD per prompt.
    pub avg_cost_usd: f64,
    /// Average duration in milliseconds per prompt.
    pub avg_duration_ms: u64,
    /// Distribution of quality grades.
    pub grade_distribution: HashMap<Grade, usize>,
}

/// Inputs to the crate-private `compute_grade` scoring helper.
///
/// Bundles the boolean and counter flags into a single named record so the
/// call sites are self-documenting and so the function signature stays
/// under the workspace's "more than 3 bools" lint threshold.
///
/// The four boolean fields are **independent quality observations**, not a
/// state machine — `one_shot`, `ci_first_try`, `qa_pass`, and `has_failure`
/// each measure a different dimension of the run, and any combination of
/// them is valid. A state-machine refactor would obscure that independence.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent quality dimensions of a single dispatch run; not a state machine"
)]
#[derive(Debug, Clone, Copy)]
pub struct GradeInputs {
    /// Session completed in one shot, no resume needed.
    pub one_shot: bool,
    /// CI passed on the first attempt, no fix needed.
    pub ci_first_try: bool,
    /// QA gate passed.
    pub qa_pass: bool,
    /// Number of resume attempts during the session.
    pub resume_count: u32,
    /// Session was aborted, errored, or rolled back.
    pub has_failure: bool,
}

/// Compute a quality grade based on execution metrics.
///
/// - A: one-shot + CI pass on first try + QA pass
/// - B: one resume or one CI fix
/// - C: multiple resumes or fixes
/// - F: stuck or failed
#[must_use]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "scoring helper exercised from tests; steward wiring lands with dispatch reporter")
)]
pub(crate) fn compute_grade(inputs: GradeInputs) -> Grade {
    if inputs.has_failure {
        return Grade::F;
    }
    if inputs.one_shot && inputs.ci_first_try && inputs.qa_pass {
        return Grade::A;
    }
    if inputs.resume_count <= 1 && (inputs.ci_first_try || inputs.qa_pass) {
        return Grade::B;
    }
    Grade::C
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn grade_display() {
        assert_eq!(Grade::A.to_string(), "A");
        assert_eq!(Grade::B.to_string(), "B");
        assert_eq!(Grade::C.to_string(), "C");
        assert_eq!(Grade::F.to_string(), "F");
    }

    #[test]
    fn compute_grade_a_for_perfect_run() {
        let grade = compute_grade(GradeInputs {
            one_shot: true,
            ci_first_try: true,
            qa_pass: true,
            resume_count: 0,
            has_failure: false,
        });
        assert_eq!(grade, Grade::A);
    }

    #[test]
    fn compute_grade_b_for_one_resume() {
        let grade = compute_grade(GradeInputs {
            one_shot: false,
            ci_first_try: true,
            qa_pass: true,
            resume_count: 1,
            has_failure: false,
        });
        assert_eq!(grade, Grade::B);
    }

    #[test]
    fn compute_grade_c_for_multiple_resumes() {
        let grade = compute_grade(GradeInputs {
            one_shot: false,
            ci_first_try: false,
            qa_pass: false,
            resume_count: 3,
            has_failure: false,
        });
        assert_eq!(grade, Grade::C);
    }

    #[test]
    fn compute_grade_f_for_failure() {
        let grade = compute_grade(GradeInputs {
            one_shot: false,
            ci_first_try: false,
            qa_pass: false,
            resume_count: 0,
            has_failure: true,
        });
        assert_eq!(grade, Grade::F);
    }

    #[test]
    fn pattern_type_display() {
        assert_eq!(
            PatternType::RecurringCiFailure.to_string(),
            "recurring-ci-failure"
        );
        assert_eq!(PatternType::CostAnomaly.to_string(), "cost-anomaly");
    }

    #[test]
    fn pattern_severity_display() {
        assert_eq!(PatternSeverity::Info.to_string(), "info");
        assert_eq!(PatternSeverity::Critical.to_string(), "critical");
    }

    #[test]
    fn dispatch_pattern_fields() {
        let pattern = DispatchPattern {
            pattern_type: PatternType::RecurringCiFailure,
            description: "clippy failures in energeia".to_owned(),
            severity: PatternSeverity::Warning,
            occurrence_count: 3,
            project: "acme/repo".to_owned(),
            scope: Some("energeia".to_owned()),
        };
        assert_eq!(pattern.occurrence_count, 3);
        assert_eq!(pattern.pattern_type, PatternType::RecurringCiFailure);
    }

    #[test]
    fn prompt_score_roundtrip() {
        let score = PromptScore {
            prompt_number: 42,
            one_shot: true,
            resume_count: 0,
            ci_first_try: true,
            qa_pass: true,
            cost_usd: 1.50,
            duration_ms: 60_000,
            quality_grade: Grade::A,
        };
        let json = serde_json::to_string(&score).expect("serialization should succeed");
        let deserialized: PromptScore =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(deserialized.prompt_number, 42);
        assert_eq!(deserialized.quality_grade, Grade::A);
    }

    #[test]
    fn project_scores_empty() {
        let scores = ProjectScores {
            total_prompts: 0,
            one_shot_rate: 0.0,
            ci_first_try_rate: 0.0,
            qa_pass_rate: 0.0,
            avg_cost_usd: 0.0,
            avg_duration_ms: 0,
            grade_distribution: HashMap::new(),
        };
        assert_eq!(scores.total_prompts, 0);
    }
}
