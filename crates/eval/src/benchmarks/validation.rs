//! Dataset validation support for memory benchmarks.

use std::io;

use serde::{Deserialize, Serialize};

/// Validation options applied while loading benchmark datasets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkValidationOptions {
    /// Dataset path for diagnostics.
    pub dataset_path: Option<String>,
    /// Downgrade incomplete records and unknown categories to warnings.
    pub allow_best_effort: bool,
    /// Require question-level retrieval evidence references.
    pub require_retrieval_evidence: bool,
}

impl BenchmarkValidationOptions {
    /// Strict validation with no dataset path.
    #[must_use]
    pub fn strict() -> Self {
        Self {
            dataset_path: None,
            allow_best_effort: false,
            require_retrieval_evidence: false,
        }
    }

    /// Strict validation for a dataset path.
    #[must_use]
    pub fn strict_for_path(path: impl Into<String>) -> Self {
        Self {
            dataset_path: Some(path.into()),
            allow_best_effort: false,
            require_retrieval_evidence: false,
        }
    }
}

/// Validation report recorded with benchmark metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkValidationReport {
    /// Dataset name.
    pub dataset: String,
    /// Dataset path used for diagnostics.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dataset_path: Option<String>,
    /// Whether best-effort validation was requested.
    pub best_effort: bool,
    /// Whether retrieval evidence refs were required.
    pub require_retrieval_evidence: bool,
    /// Fatal validation errors.
    #[serde(default)]
    pub errors: Vec<BenchmarkValidationIssue>,
    /// Best-effort validation warnings.
    #[serde(default)]
    pub warnings: Vec<BenchmarkValidationIssue>,
}

impl BenchmarkValidationReport {
    /// Create an empty report for one dataset.
    #[must_use]
    pub fn new(dataset: impl Into<String>, options: &BenchmarkValidationOptions) -> Self {
        Self {
            dataset: dataset.into(),
            dataset_path: options.dataset_path.clone(),
            best_effort: options.allow_best_effort,
            require_retrieval_evidence: options.require_retrieval_evidence,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Record an always-fatal validation issue.
    pub fn error(
        &mut self,
        record_id: Option<String>,
        question_id: Option<String>,
        field: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.errors.push(BenchmarkValidationIssue {
            dataset_path: self.dataset_path.clone(),
            record_id,
            question_id,
            field: field.into(),
            message: message.into(),
        });
    }

    /// Record an issue that best-effort mode may downgrade to a warning.
    pub fn issue(
        &mut self,
        options: &BenchmarkValidationOptions,
        record_id: Option<String>,
        question_id: Option<String>,
        field: impl Into<String>,
        message: impl Into<String>,
    ) {
        let issue = BenchmarkValidationIssue {
            dataset_path: self.dataset_path.clone(),
            record_id,
            question_id,
            field: field.into(),
            message: message.into(),
        };
        if options.allow_best_effort {
            self.warnings.push(issue);
        } else {
            self.errors.push(issue);
        }
    }

    /// Return this report if validation passed.
    ///
    /// # Errors
    ///
    /// Returns `InvalidData` with item-level diagnostics when validation
    /// produced any fatal errors.
    pub fn into_result(self) -> io::Result<Self> {
        if self.errors.is_empty() {
            Ok(self)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                self.error_summary(),
            ))
        }
    }

    /// Human-readable fatal validation summary.
    #[must_use]
    pub fn error_summary(&self) -> String {
        let mut lines = Vec::with_capacity(self.errors.len() + 1);
        lines.push(format!(
            "{} dataset validation failed with {} error(s)",
            self.dataset,
            self.errors.len()
        ));
        lines.extend(self.errors.iter().map(ToString::to_string));
        lines.join("\n")
    }
}

/// One item-level dataset validation issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkValidationIssue {
    /// Dataset path that contained the invalid record.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dataset_path: Option<String>,
    /// Dataset record/conversation identifier when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub record_id: Option<String>,
    /// Question identifier when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub question_id: Option<String>,
    /// Invalid field name.
    pub field: String,
    /// Human-readable diagnostic.
    pub message: String,
}

impl core::fmt::Display for BenchmarkValidationIssue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(path) = &self.dataset_path {
            write!(f, "{path}")?;
        } else {
            write!(f, "<memory>")?;
        }
        if let Some(record_id) = &self.record_id {
            write!(f, " record={record_id}")?;
        }
        if let Some(question_id) = &self.question_id {
            write!(f, " question={question_id}")?;
        }
        write!(f, " field={}: {}", self.field, self.message)
    }
}

/// Filter empty evidence refs and trim surviving refs.
#[must_use]
pub fn clean_refs(refs: &[String]) -> Vec<String> {
    refs.iter()
        .map(|reference| reference.trim())
        .filter(|reference| !reference.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Deserialize evidence refs from absent/null, a string, or a string array.
///
/// # Errors
///
/// Returns a serde error when the field is neither a string nor a string array.
pub fn deserialize_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringList {
        One(String),
        Many(Vec<String>),
    }

    let value = Option::<StringList>::deserialize(deserializer)?;
    Ok(match value {
        Some(StringList::One(item)) => vec![item],
        Some(StringList::Many(items)) => items,
        None => Vec::new(),
    })
}
