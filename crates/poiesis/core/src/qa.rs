//! QA report types for document validation.
//!
//! Every render path gates on [`QaReport::has_issues`] being `false`.

use serde::{Deserialize, Serialize};

use crate::error::FactbaseError;

/// Kind of QA issue found during document validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum QaIssueKind {
    /// A citation could not be resolved to a fact.
    CitationUnresolvable,
    /// A claim does not match the fact it cites.
    ClaimMismatch,
    /// Prose violated a style or structural rule.
    ProseViolation,
    /// A required section is absent from the document.
    MissingSection,
}

/// A single QA issue with kind, optional source location, and human message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaIssue {
    /// The classification of the issue.
    pub kind: QaIssueKind,
    /// Optional source location (e.g. a JSON pointer or line reference).
    pub location: Option<String>,
    /// Human-readable description of the issue.
    pub message: String,
}

/// Aggregate QA result; every render path gates on `has_issues == false`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaReport {
    /// Whether any issues were found.
    pub has_issues: bool,
    /// Number of issues in this report.
    pub issue_count: usize,
    /// The individual issues comprising this report.
    pub issues: Vec<QaIssue>,
}

impl QaReport {
    /// Returns a passing report with no issues.
    #[must_use]
    pub fn pass() -> Self {
        Self {
            has_issues: false,
            issue_count: 0,
            issues: Vec::new(),
        }
    }

    /// Creates a report from a vector of issues.
    ///
    /// `has_issues` is set to `true` when the vector is non-empty.
    #[must_use]
    pub fn new(issues: Vec<QaIssue>) -> Self {
        let has_issues = !issues.is_empty();
        let issue_count = issues.len();
        Self {
            has_issues,
            issue_count,
            issues,
        }
    }

    /// Merges multiple reports into a single report.
    ///
    /// All issues are flattened and `has_issues` is recomputed.
    #[must_use]
    pub fn merge(reports: impl IntoIterator<Item = QaReport>) -> Self {
        let mut issues = Vec::new();
        for report in reports {
            issues.extend(report.issues);
        }
        Self::new(issues)
    }

    /// Returns `true` if the report contains no issues.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        !self.has_issues
    }

    /// Serialises the report to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] if serialisation fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

impl From<&FactbaseError> for QaIssue {
    fn from(error: &FactbaseError) -> Self {
        Self {
            kind: QaIssueKind::CitationUnresolvable,
            location: None,
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FactbaseError;

    #[test]
    fn pass_report_has_no_issues() {
        let report = QaReport::pass();
        assert!(!report.has_issues);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn new_empty_equals_pass() {
        let report = QaReport::new(Vec::new());
        assert!(!report.has_issues);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn new_nonempty_sets_has_issues_true() {
        let issue = QaIssue {
            kind: QaIssueKind::ProseViolation,
            location: None,
            message: "bad prose".to_owned(),
        };
        let report = QaReport::new(vec![issue]);
        assert!(report.has_issues);
        assert_eq!(report.issues.len(), 1);
    }

    #[test]
    fn merge_combines_issues() {
        let r1 = QaReport::new(vec![QaIssue {
            kind: QaIssueKind::MissingSection,
            location: Some("/body".to_owned()),
            message: "missing intro".to_owned(),
        }]);
        let r2 = QaReport::new(vec![QaIssue {
            kind: QaIssueKind::ClaimMismatch,
            location: None,
            message: "claim mismatch".to_owned(),
        }]);
        let merged = QaReport::merge([r1, r2]);
        assert!(merged.has_issues);
        assert_eq!(merged.issues.len(), 2);
    }

    #[test]
    fn is_clean_reflects_has_issues() {
        let clean = QaReport::pass();
        assert!(clean.is_clean());

        let dirty = QaReport::new(vec![QaIssue {
            kind: QaIssueKind::CitationUnresolvable,
            location: None,
            message: "oops".to_owned(),
        }]);
        assert!(!dirty.is_clean());
    }

    #[test]
    fn from_factbase_error_maps_kind() {
        let error = FactbaseError::UnknownFact {
            id: "f-1".to_owned(),
            referenced_by: "claim-a".to_owned(),
        };
        let issue: QaIssue = (&error).into();
        assert!(matches!(issue.kind, QaIssueKind::CitationUnresolvable));
        assert_eq!(issue.location, None);
        assert_eq!(issue.message, error.to_string());
    }
}
