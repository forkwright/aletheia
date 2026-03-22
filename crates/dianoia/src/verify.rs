//! Verification workflow: goal-backward tracing against phase success criteria.
//!
//! Verifies that a phase's success criteria are met by checking each criterion
//! against provided evidence. Works backward from goals: for each goal, trace
//! which criteria are met and which have gaps.

use serde::{Deserialize, Serialize};

use crate::phase::Phase;

/// Overall verification status for a phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum VerificationStatus {
    /// All criteria are met.
    Met,
    /// Some criteria are met, some are not.
    PartiallyMet,
    /// No criteria are met (or critical criteria failed).
    NotMet,
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Met => f.write_str("met"),
            Self::PartiallyMet => f.write_str("partially-met"),
            Self::NotMet => f.write_str("not-met"),
        }
    }
}

/// Status of an individual criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CriterionStatus {
    /// Criterion is satisfied.
    Met,
    /// Criterion is partially satisfied.
    PartiallyMet,
    /// Criterion is not satisfied.
    NotMet,
}

impl std::fmt::Display for CriterionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Met => f.write_str("met"),
            Self::PartiallyMet => f.write_str("partially-met"),
            Self::NotMet => f.write_str("not-met"),
        }
    }
}

/// Evidence supporting a criterion evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// Type of evidence (e.g., "file", "test-result", "output").
    pub kind: String,
    /// The evidence content or reference (file path, test output, etc.).
    pub content: String,
}

/// A gap found during verification: an unmet or partially-met criterion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationGap {
    /// The criterion text that was evaluated.
    pub criterion: String,
    /// Current status of this criterion.
    pub status: CriterionStatus,
    /// Detailed explanation of why the criterion is not fully met.
    pub detail: String,
    /// Concrete next step to close the gap.
    pub proposed_fix: String,
}

/// Evaluation of a single criterion, with status and supporting evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionResult {
    /// The criterion text being evaluated.
    pub criterion: String,
    /// Whether it passed, partially passed, or failed.
    pub status: CriterionStatus,
    /// Evidence supporting this evaluation.
    pub evidence: Vec<Evidence>,
    /// Explanation of the evaluation outcome.
    pub detail: String,
}

/// Full verification result for a phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Overall phase verification status.
    pub status: VerificationStatus,
    /// Human-readable summary of the verification outcome.
    pub summary: String,
    /// Per-criterion results.
    pub criteria: Vec<CriterionResult>,
    /// Gaps that need to be addressed.
    pub gaps: Vec<VerificationGap>,
    /// When the verification was performed.
    pub verified_at: jiff::Timestamp,
    /// Whether this result was manually overridden.
    pub overridden: bool,
    /// Override justification, if any.
    pub override_note: Option<String>,
}

/// Input for verifying a single criterion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionInput {
    /// The criterion text to evaluate.
    pub criterion: String,
    /// Whether the criterion is met.
    pub status: CriterionStatus,
    /// Evidence supporting the evaluation.
    pub evidence: Vec<Evidence>,
    /// Explanation of the evaluation.
    pub detail: String,
    /// If not met, a proposed fix.
    pub proposed_fix: Option<String>,
}

/// A goal with its traced criteria mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalTrace {
    /// The goal being traced.
    pub goal: String,
    /// Criteria that map to this goal, with their statuses.
    pub criteria: Vec<CriterionResult>,
    /// Whether this goal is fully satisfied.
    pub satisfied: bool,
}

/// Verify a phase's success criteria against provided evidence.
///
/// Takes a phase and a set of criterion evaluations, produces a structured
/// verification result. The agent provides criterion evaluations, and this
/// function aggregates them into an overall result.
#[must_use]
pub fn verify_phase(phase: &Phase, inputs: &[CriterionInput]) -> VerificationResult {
    let mut criteria = Vec::with_capacity(inputs.len());
    let mut gaps = Vec::new();

    for input in inputs {
        let result = CriterionResult {
            criterion: input.criterion.clone(),
            status: input.status,
            evidence: input.evidence.clone(),
            detail: input.detail.clone(),
        };
        criteria.push(result);

        if input.status != CriterionStatus::Met {
            gaps.push(VerificationGap {
                criterion: input.criterion.clone(),
                status: input.status,
                detail: input.detail.clone(),
                proposed_fix: input
                    .proposed_fix
                    .clone()
                    .unwrap_or_else(|| "no fix proposed".to_owned()),
            });
        }
    }

    let status = compute_overall_status(&criteria);
    let summary = build_summary(phase, &criteria, &gaps);

    VerificationResult {
        status,
        summary,
        criteria,
        gaps,
        verified_at: jiff::Timestamp::now(),
        overridden: false,
        override_note: None,
    }
}

/// Trace goals backward: for each goal, find which criteria contribute to it
/// and whether the goal is satisfied.
///
/// Goals are the phase goal plus any phase requirements. Each criterion is
/// matched to the goal it most directly supports (by substring containment
/// or shared significant words).
#[must_use]
pub fn trace_goals(phase: &Phase, criteria: &[CriterionResult]) -> Vec<GoalTrace> {
    let mut goals: Vec<String> = vec![phase.goal.clone()];
    goals.extend(phase.requirements.iter().cloned());

    goals
        .into_iter()
        .map(|goal| {
            let matching: Vec<CriterionResult> = criteria
                .iter()
                .filter(|c| {
                    let goal_lower = goal.to_lowercase();
                    let criterion_lower = c.criterion.to_lowercase();
                    criterion_lower.contains(&goal_lower)
                        || goal_lower.contains(&criterion_lower)
                        || shares_significant_words(&goal_lower, &criterion_lower)
                })
                .cloned()
                .collect();

            let satisfied =
                !matching.is_empty() && matching.iter().all(|c| c.status == CriterionStatus::Met);

            GoalTrace {
                goal,
                criteria: matching,
                satisfied,
            }
        })
        .collect()
}

/// Override a verification result with a manual justification.
#[must_use]
pub fn override_result(mut result: VerificationResult, note: String) -> VerificationResult {
    result.overridden = true;
    result.override_note = Some(note);
    result
}

/// Compute overall verification status from per-criterion results.
fn compute_overall_status(criteria: &[CriterionResult]) -> VerificationStatus {
    if criteria.is_empty() {
        return VerificationStatus::NotMet;
    }

    let all_met = criteria.iter().all(|c| c.status == CriterionStatus::Met);
    let any_met = criteria
        .iter()
        .any(|c| c.status == CriterionStatus::Met || c.status == CriterionStatus::PartiallyMet);

    if all_met {
        VerificationStatus::Met
    } else if any_met {
        VerificationStatus::PartiallyMet
    } else {
        VerificationStatus::NotMet
    }
}

/// Build a human-readable summary of the verification.
fn build_summary(phase: &Phase, criteria: &[CriterionResult], gaps: &[VerificationGap]) -> String {
    let met_count = criteria
        .iter()
        .filter(|c| c.status == CriterionStatus::Met)
        .count();

    if gaps.is_empty() {
        format!(
            "Phase '{}': all {} criteria met.",
            phase.name,
            criteria.len()
        )
    } else {
        format!(
            "Phase '{}': {}/{} criteria met, {} gaps remaining.",
            phase.name,
            met_count,
            criteria.len(),
            gaps.len()
        )
    }
}

/// Check whether two strings share significant words (3+ chars).
fn shares_significant_words(a: &str, b: &str) -> bool {
    let a_words: Vec<&str> = a.split_whitespace().filter(|w| w.len() >= 3).collect();
    let b_words: Vec<&str> = b.split_whitespace().filter(|w| w.len() >= 3).collect();

    a_words.iter().any(|w| b_words.contains(w))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
    use super::*;

    fn make_phase() -> Phase {
        let mut phase = Phase::new(
            "Foundation".to_owned(),
            "Build the foundation layer".to_owned(),
            1,
        );
        phase.requirements = vec![
            "All tests pass".to_owned(),
            "API endpoints documented".to_owned(),
        ];
        phase
    }

    fn met_input(criterion: &str) -> CriterionInput {
        CriterionInput {
            criterion: criterion.to_owned(),
            status: CriterionStatus::Met,
            evidence: vec![Evidence {
                kind: "test_result".to_owned(),
                content: "all tests passed".to_owned(),
            }],
            detail: "criterion satisfied".to_owned(),
            proposed_fix: None,
        }
    }

    fn failed_input(criterion: &str, fix: &str) -> CriterionInput {
        CriterionInput {
            criterion: criterion.to_owned(),
            status: CriterionStatus::NotMet,
            evidence: Vec::new(),
            detail: "criterion not satisfied".to_owned(),
            proposed_fix: Some(fix.to_owned()),
        }
    }

    fn partial_input(criterion: &str) -> CriterionInput {
        CriterionInput {
            criterion: criterion.to_owned(),
            status: CriterionStatus::PartiallyMet,
            evidence: vec![Evidence {
                kind: "file".to_owned(),
                content: "src/lib.rs".to_owned(),
            }],
            detail: "partially done".to_owned(),
            proposed_fix: Some("complete the remaining work".to_owned()),
        }
    }

    #[test]
    fn all_criteria_met_returns_met() {
        let phase = make_phase();
        let inputs = vec![met_input("tests pass"), met_input("docs complete")];

        let result = verify_phase(&phase, &inputs);

        assert_eq!(result.status, VerificationStatus::Met);
        assert!(result.gaps.is_empty());
        assert_eq!(result.criteria.len(), 2);
        assert!(!result.overridden);
    }

    #[test]
    fn no_criteria_returns_not_met() {
        let phase = make_phase();
        let result = verify_phase(&phase, &[]);

        assert_eq!(result.status, VerificationStatus::NotMet);
    }

    #[test]
    fn mixed_criteria_returns_partially_met() {
        let phase = make_phase();
        let inputs = vec![
            met_input("tests pass"),
            failed_input("docs complete", "write the docs"),
        ];

        let result = verify_phase(&phase, &inputs);

        assert_eq!(result.status, VerificationStatus::PartiallyMet);
        assert_eq!(result.gaps.len(), 1);
        assert_eq!(result.gaps[0].criterion, "docs complete");
        assert_eq!(result.gaps[0].proposed_fix, "write the docs");
    }

    #[test]
    fn all_criteria_failed_returns_not_met() {
        let phase = make_phase();
        let inputs = vec![
            failed_input("tests pass", "fix tests"),
            failed_input("docs complete", "write docs"),
        ];

        let result = verify_phase(&phase, &inputs);

        assert_eq!(result.status, VerificationStatus::NotMet);
        assert_eq!(result.gaps.len(), 2);
    }

    #[test]
    fn partially_met_criterion_creates_gap() {
        let phase = make_phase();
        let inputs = vec![partial_input("API coverage")];

        let result = verify_phase(&phase, &inputs);

        assert_eq!(result.status, VerificationStatus::PartiallyMet);
        assert_eq!(result.gaps.len(), 1);
        assert_eq!(result.gaps[0].status, CriterionStatus::PartiallyMet);
    }

    #[test]
    fn failed_without_fix_gets_default_message() {
        let phase = make_phase();
        let inputs = vec![CriterionInput {
            criterion: "no fix provided".to_owned(),
            status: CriterionStatus::NotMet,
            evidence: Vec::new(),
            detail: "failed".to_owned(),
            proposed_fix: None,
        }];

        let result = verify_phase(&phase, &inputs);

        assert_eq!(result.gaps[0].proposed_fix, "no fix proposed");
    }

    #[test]
    fn summary_includes_phase_name_and_counts() {
        let phase = make_phase();
        let inputs = vec![met_input("tests pass"), failed_input("docs", "write them")];

        let result = verify_phase(&phase, &inputs);

        assert!(
            result.summary.contains("Foundation"),
            "summary should contain phase name: {}",
            result.summary
        );
        assert!(
            result.summary.contains("1/2"),
            "summary should contain met/total: {}",
            result.summary
        );
    }

    #[test]
    fn all_met_summary_says_all() {
        let phase = make_phase();
        let inputs = vec![met_input("a"), met_input("b")];

        let result = verify_phase(&phase, &inputs);

        assert!(
            result.summary.contains("all 2 criteria met"),
            "summary: {}",
            result.summary
        );
    }

    #[test]
    fn override_sets_flag_and_note() {
        let phase = make_phase();
        let inputs = vec![failed_input("criterion", "fix it")];
        let result = verify_phase(&phase, &inputs);

        let overridden = override_result(result, "accepted risk".to_owned());

        assert!(overridden.overridden);
        assert_eq!(overridden.override_note.as_deref(), Some("accepted risk"));
        assert_eq!(overridden.status, VerificationStatus::NotMet);
    }

    #[test]
    fn trace_goals_matches_criteria_to_goals() {
        let phase = make_phase();
        let criteria = vec![
            CriterionResult {
                criterion: "All tests pass in CI".to_owned(),
                status: CriterionStatus::Met,
                evidence: Vec::new(),
                detail: "passed".to_owned(),
            },
            CriterionResult {
                criterion: "API endpoints documented in OpenAPI".to_owned(),
                status: CriterionStatus::NotMet,
                evidence: Vec::new(),
                detail: "missing".to_owned(),
            },
        ];

        let traces = trace_goals(&phase, &criteria);

        assert_eq!(traces.len(), 3, "phase goal + 2 requirements = 3 traces");

        let tests_goal = traces.iter().find(|t| t.goal == "All tests pass").unwrap();
        assert!(tests_goal.satisfied, "tests goal should be satisfied");

        let docs_goal = traces
            .iter()
            .find(|t| t.goal == "API endpoints documented")
            .unwrap();
        assert!(!docs_goal.satisfied, "docs goal should not be satisfied");
    }

    #[test]
    fn trace_goals_empty_criteria_unsatisfied() {
        let phase = make_phase();
        let traces = trace_goals(&phase, &[]);

        for trace in &traces {
            assert!(
                !trace.satisfied,
                "empty criteria means no goal is satisfied"
            );
        }
    }

    #[test]
    fn evidence_preserved_in_result() {
        let phase = make_phase();
        let inputs = vec![CriterionInput {
            criterion: "test".to_owned(),
            status: CriterionStatus::Met,
            evidence: vec![
                Evidence {
                    kind: "file".to_owned(),
                    content: "src/main.rs".to_owned(),
                },
                Evidence {
                    kind: "test_result".to_owned(),
                    content: "42 tests passed".to_owned(),
                },
            ],
            detail: "ok".to_owned(),
            proposed_fix: None,
        }];

        let result = verify_phase(&phase, &inputs);

        assert_eq!(result.criteria[0].evidence.len(), 2);
        assert_eq!(result.criteria[0].evidence[0].kind, "file");
        assert_eq!(result.criteria[0].evidence[1].content, "42 tests passed");
    }

    #[test]
    fn verification_result_serde_roundtrip() {
        let phase = make_phase();
        let inputs = vec![
            met_input("criterion-a"),
            failed_input("criterion-b", "fix b"),
        ];

        let result = verify_phase(&phase, &inputs);
        let json = serde_json::to_string(&result).unwrap();
        let back: VerificationResult = serde_json::from_str(&json).unwrap();

        assert_eq!(back.status, result.status);
        assert_eq!(back.criteria.len(), result.criteria.len());
        assert_eq!(back.gaps.len(), result.gaps.len());
    }

    #[test]
    fn verification_status_display() {
        assert_eq!(VerificationStatus::Met.to_string(), "met");
        assert_eq!(
            VerificationStatus::PartiallyMet.to_string(),
            "partially-met"
        );
        assert_eq!(VerificationStatus::NotMet.to_string(), "not-met");
    }

    #[test]
    fn criterion_status_display() {
        assert_eq!(CriterionStatus::Met.to_string(), "met");
        assert_eq!(CriterionStatus::PartiallyMet.to_string(), "partially-met");
        assert_eq!(CriterionStatus::NotMet.to_string(), "not-met");
    }
}
