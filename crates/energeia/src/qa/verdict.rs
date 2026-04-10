// WHY: Verdict determination is pure logic that combines mechanical issues
// and criterion results into a single QaVerdict. Separated from the
// orchestrator so the rules are testable in isolation.

use crate::types::{CriterionResult, MechanicalIssue, QaVerdict};

/// Determine the aggregate verdict from mechanical issues and criterion results.
///
/// Rules:
/// - Any mechanical issue → [`QaVerdict::Fail`] (hard blocker)
/// - All criteria pass → [`QaVerdict::Pass`]
/// - Some fail + some pass → [`QaVerdict::Partial`]
/// - All fail → [`QaVerdict::Fail`]
/// - No results and no issues → [`QaVerdict::Pass`] (vacuously true)
#[must_use]
pub fn determine_verdict(
    criteria: &[CriterionResult],
    mechanical_issues: &[MechanicalIssue],
) -> QaVerdict {
    // WHY: Mechanical issues are a hard blocker — any mechanical failure means
    // the PR cannot be accepted regardless of semantic results.
    if !mechanical_issues.is_empty() {
        return QaVerdict::Fail;
    }

    if criteria.is_empty() {
        return QaVerdict::Pass;
    }

    let fail_count = criteria.iter().filter(|r| !r.passed).count();
    let pass_count = criteria.iter().filter(|r| r.passed).count();

    if fail_count > 0 && pass_count > 0 {
        QaVerdict::Partial
    } else if fail_count > 0 {
        QaVerdict::Fail
    } else {
        QaVerdict::Pass
    }
}

/// Returns `true` if any mechanical issue is present.
///
/// When this returns `true`, LLM evaluation should be skipped to save cost.
#[must_use]
pub fn has_critical_mechanical_issues(issues: &[MechanicalIssue]) -> bool {
    !issues.is_empty()
}

#[cfg(test)]
mod tests {
    use crate::types::{CriterionType, MechanicalIssueKind};

    use super::*;

    fn pass_criterion(name: &str) -> CriterionResult {
        CriterionResult {
            criterion: name.to_owned(),
            classification: CriterionType::Semantic,
            passed: true,
            evidence: "ok".to_owned(),
        }
    }

    fn fail_criterion(name: &str) -> CriterionResult {
        CriterionResult {
            criterion: name.to_owned(),
            classification: CriterionType::Semantic,
            passed: false,
            evidence: "missing".to_owned(),
        }
    }

    fn mechanical_issue() -> MechanicalIssue {
        MechanicalIssue {
            kind: MechanicalIssueKind::BlastRadiusViolation,
            message: "out of scope".to_owned(),
            details: None,
        }
    }

    #[test]
    fn verdict_all_pass() {
        let results = vec![pass_criterion("a"), pass_criterion("b")];
        assert_eq!(determine_verdict(&results, &[]), QaVerdict::Pass);
    }

    #[test]
    fn verdict_all_fail() {
        let results = vec![fail_criterion("a"), fail_criterion("b")];
        assert_eq!(determine_verdict(&results, &[]), QaVerdict::Fail);
    }

    #[test]
    fn verdict_partial() {
        let results = vec![pass_criterion("a"), fail_criterion("b")];
        assert_eq!(determine_verdict(&results, &[]), QaVerdict::Partial);
    }

    #[test]
    fn verdict_fail_on_mechanical_issues() {
        // WHY: Even with all criteria passing, mechanical issues force FAIL.
        let results = vec![pass_criterion("a")];
        let issues = vec![mechanical_issue()];
        assert_eq!(determine_verdict(&results, &issues), QaVerdict::Fail);
    }

    #[test]
    fn verdict_empty_results_is_pass() {
        assert_eq!(determine_verdict(&[], &[]), QaVerdict::Pass);
    }

    #[test]
    fn verdict_mechanical_only_no_criteria() {
        let issues = vec![mechanical_issue()];
        assert_eq!(determine_verdict(&[], &issues), QaVerdict::Fail);
    }

    #[test]
    fn verdict_single_pass() {
        let results = vec![pass_criterion("a")];
        assert_eq!(determine_verdict(&results, &[]), QaVerdict::Pass);
    }

    #[test]
    fn verdict_single_fail() {
        let results = vec![fail_criterion("a")];
        assert_eq!(determine_verdict(&results, &[]), QaVerdict::Fail);
    }

    #[test]
    fn has_critical_mechanical_issues_empty() {
        assert!(!has_critical_mechanical_issues(&[]));
    }

    #[test]
    fn has_critical_mechanical_issues_present() {
        assert!(has_critical_mechanical_issues(&[mechanical_issue()]));
    }
}
