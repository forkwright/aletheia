// WHY: Corrective prompt generation creates focused fix prompts targeting only
// failed criteria. This prevents full reimplementation when only specific
// aspects of a PR need correction.

use crate::qa::PromptSpec;
use crate::types::{MechanicalIssueKind, QaResult, QaVerdict};

/// Generate a corrective prompt targeting only the failed criteria.
///
/// Returns `None` if the verdict is [`QaVerdict::Pass`] (nothing to fix).
/// The corrective prompt references the original PR, includes evidence from
/// the QA evaluation, and uses a focused blast radius.
#[must_use]
pub fn generate_corrective(qa_result: &QaResult, original: &PromptSpec) -> Option<PromptSpec> {
    if qa_result.verdict == QaVerdict::Pass {
        return None;
    }

    let failed_criteria: Vec<_> = qa_result
        .criteria_results
        .iter()
        .filter(|cr| !cr.passed)
        .collect();

    if failed_criteria.is_empty() {
        return None;
    }

    // NOTE: Build acceptance criteria from only the failed criteria.
    let acceptance_criteria: Vec<String> = failed_criteria
        .iter()
        .map(|cr| cr.criterion.clone())
        .collect();

    let blast_radius = extract_corrective_blast_radius(qa_result, original);

    let description = format!(
        "Fix failing criteria from PR #{} (prompt #{})",
        qa_result.pr_number, qa_result.prompt_number
    );

    Some(PromptSpec {
        prompt_number: 0,
        description,
        acceptance_criteria,
        blast_radius,
    })
}

/// Derive a failure type category from the QA result.
///
/// Groups failures by their primary mechanical issue kind, or "semantic" if
/// no mechanical issues are present. Used for circuit breaker tracking.
#[must_use]
pub fn derive_failure_type(qa_result: &QaResult) -> String {
    if let Some(first_issue) = qa_result.mechanical_issues.first() {
        match first_issue.kind {
            MechanicalIssueKind::BlastRadiusViolation => "blast_radius_violation",
            MechanicalIssueKind::AntiPattern => "anti_pattern",
            MechanicalIssueKind::LintViolation => "lint_violation",
            MechanicalIssueKind::FormatViolation => "format_violation",
        }
        .to_owned()
    } else {
        "semantic".to_owned()
    }
}

/// Determine the blast radius for a corrective prompt.
///
/// Prefers files specifically mentioned in mechanical issues. Falls back to
/// the original prompt's blast radius.
fn extract_corrective_blast_radius(qa_result: &QaResult, original: &PromptSpec) -> Vec<String> {
    let mut files_from_issues: Vec<String> = qa_result
        .mechanical_issues
        .iter()
        .filter_map(|issue| match issue.kind {
            // WHY: AntiPattern details use "file:line" format — extract file
            // before the colon.
            MechanicalIssueKind::AntiPattern => issue
                .details
                .as_ref()
                .and_then(|d| d.split(':').next().map(ToOwned::to_owned)),
            // WHY: BlastRadiusViolation message uses "file modified outside
            // blast radius: {file}" format.
            MechanicalIssueKind::BlastRadiusViolation => issue
                .message
                .strip_prefix("file modified outside blast radius: ")
                .map(ToOwned::to_owned),
            // NOTE: Other kinds fall back to "file:line" in details.
            _ => issue
                .details
                .as_ref()
                .and_then(|d| d.split(':').next().map(ToOwned::to_owned)),
        })
        .collect();

    files_from_issues.sort();
    files_from_issues.dedup();

    if !files_from_issues.is_empty() {
        return files_from_issues;
    }

    original.blast_radius.clone()
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
mod tests {
    use jiff::Timestamp;

    use crate::types::{CriterionResult, CriterionType, MechanicalIssue, MechanicalIssueKind};

    use super::*;

    fn make_qa_result(verdict: QaVerdict, criteria: Vec<CriterionResult>) -> QaResult {
        QaResult {
            prompt_number: 1,
            pr_number: 42,
            verdict,
            criteria_results: criteria,
            mechanical_issues: vec![],
            cost_usd: 0.03,
            evaluated_at: Timestamp::now(),
        }
    }

    fn pass_cr(name: &str) -> CriterionResult {
        CriterionResult {
            criterion: name.to_owned(),
            classification: CriterionType::Semantic,
            passed: true,
            evidence: "ok".to_owned(),
        }
    }

    fn fail_cr(name: &str) -> CriterionResult {
        CriterionResult {
            criterion: name.to_owned(),
            classification: CriterionType::Semantic,
            passed: false,
            evidence: "missing".to_owned(),
        }
    }

    fn original_prompt() -> PromptSpec {
        PromptSpec {
            prompt_number: 1,
            description: "add health endpoint".to_owned(),
            acceptance_criteria: vec!["feature works".to_owned(), "errors handled".to_owned()],
            blast_radius: vec!["crates/pylon/src/".to_owned()],
        }
    }

    // -----------------------------------------------------------------------
    // generate_corrective
    // -----------------------------------------------------------------------

    #[test]
    fn pass_returns_none() {
        let result = make_qa_result(QaVerdict::Pass, vec![pass_cr("a")]);
        assert!(generate_corrective(&result, &original_prompt()).is_none());
    }

    #[test]
    fn fail_returns_corrective() {
        let result = make_qa_result(
            QaVerdict::Fail,
            vec![pass_cr("feature works"), fail_cr("errors handled")],
        );

        let corrective = generate_corrective(&result, &original_prompt());
        assert!(corrective.is_some());

        let spec = corrective.unwrap();
        assert_eq!(spec.acceptance_criteria, vec!["errors handled"]);
        assert!(spec.description.contains("PR #42"));
    }

    #[test]
    fn partial_returns_corrective_with_only_failed() {
        let result = make_qa_result(
            QaVerdict::Partial,
            vec![pass_cr("feature works"), fail_cr("errors handled")],
        );

        let corrective = generate_corrective(&result, &original_prompt());
        assert!(corrective.is_some());

        let spec = corrective.unwrap();
        assert_eq!(spec.acceptance_criteria.len(), 1);
        assert_eq!(spec.acceptance_criteria[0], "errors handled");
    }

    #[test]
    fn corrective_uses_original_blast_radius_when_no_mechanical_issues() {
        let result = make_qa_result(QaVerdict::Fail, vec![fail_cr("a")]);
        let corrective = generate_corrective(&result, &original_prompt()).unwrap();
        assert_eq!(corrective.blast_radius, vec!["crates/pylon/src/"]);
    }

    #[test]
    fn corrective_uses_mechanical_issue_files() {
        let mut result = make_qa_result(QaVerdict::Fail, vec![fail_cr("a")]);
        result.mechanical_issues = vec![MechanicalIssue {
            kind: MechanicalIssueKind::AntiPattern,
            message: "unwrap in library code".to_owned(),
            details: Some("src/lib.rs:42".to_owned()),
        }];

        let corrective = generate_corrective(&result, &original_prompt()).unwrap();
        assert_eq!(corrective.blast_radius, vec!["src/lib.rs"]);
    }

    // -----------------------------------------------------------------------
    // derive_failure_type
    // -----------------------------------------------------------------------

    #[test]
    fn derive_failure_type_semantic() {
        let result = make_qa_result(QaVerdict::Fail, vec![fail_cr("a")]);
        assert_eq!(derive_failure_type(&result), "semantic");
    }

    #[test]
    fn derive_failure_type_mechanical() {
        let mut result = make_qa_result(QaVerdict::Fail, vec![fail_cr("a")]);
        result.mechanical_issues = vec![MechanicalIssue {
            kind: MechanicalIssueKind::BlastRadiusViolation,
            message: "out of scope".to_owned(),
            details: None,
        }];
        assert_eq!(derive_failure_type(&result), "blast_radius_violation");
    }
}
