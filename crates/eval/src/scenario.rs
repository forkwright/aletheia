//! Scenario trait and outcome types.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::client::EvalClient;
use crate::error::{self, Error, Result};

/// Boxed future returned by scenario `run` methods.
pub type ScenarioFuture<'a> = Pin<Box<dyn Future<Output = ScenarioRunOutcome> + Send + 'a>>;

/// Classification of a scenario's intent for reporting and validation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScenarioClassification {
    /// A semantic assertion with explicit expected criteria.
    #[default]
    Assertive,
    /// A lightweight health/sanity check that may lack explicit criteria.
    Smoke,
    /// An observational probe whose result is recorded but not asserted.
    Informational,
}

/// Metadata describing a scenario for display and filtering.
#[derive(Debug, Clone)]
pub struct ScenarioMeta {
    /// Unique identifier (e.g., "health-returns-ok").
    pub id: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Category for grouping in output (e.g., "health", "session").
    pub category: &'static str,
    /// Whether this scenario requires an auth token.
    pub requires_auth: bool,
    /// Whether this scenario requires at least one configured nous.
    pub requires_nous: bool,
    /// Optional substring that the response text must contain.
    pub expected_contains: Option<&'static str>,
    /// Optional regex pattern that the response text must match.
    pub expected_pattern: Option<&'static str>,
    /// Classification of the scenario's intent.
    pub classification: ScenarioClassification,
}

impl ScenarioMeta {
    /// Human-readable summary of the expected criteria, if any.
    #[must_use]
    pub fn criteria_summary(&self) -> Option<String> {
        match (self.expected_contains, self.expected_pattern) {
            (Some(s), None) => Some(format!("contains: {s:?}")),
            (None, Some(p)) => Some(format!("pattern: {p:?}")),
            (Some(s), Some(p)) => Some(format!("contains: {s:?}; pattern: {p:?}")),
            (None, None) => None,
        }
    }
}

/// Outcome of running a single scenario.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScenarioOutcome {
    /// Scenario completed within timeout without errors.
    Passed {
        /// Wall-clock execution time.
        duration: Duration,
    },
    /// Scenario returned an error or assertion failed.
    Failed {
        /// Wall-clock execution time.
        duration: Duration,
        /// The error that caused failure.
        error: Error,
    },
    /// Scenario was not run (e.g. missing auth token or nous).
    Skipped {
        /// Human-readable reason for skipping.
        reason: String,
    },
}

impl ScenarioOutcome {
    /// Returns `true` if the scenario passed.
    #[must_use]
    pub fn is_passed(&self) -> bool {
        matches!(self, Self::Passed { .. })
    }

    /// Returns `true` if the scenario failed.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Result of a single sub-probe or subcase within a scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSubResult {
    /// Identifier for the sub-probe.
    pub sub_id: String,
    /// Classification of this sub-result.
    pub classification: ScenarioClassification,
    /// Whether the sub-probe passed.
    pub passed: bool,
    /// Human-readable criteria checked by the sub-probe.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub criteria: Option<String>,
    /// Short excerpt or hash of the response evaluated.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub response_excerpt: Option<String>,
    /// Identifiers of any violations detected.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub violation_ids: Vec<String>,
}

/// Rich outcome returned by a scenario's `run` method.
#[derive(Debug)]
pub struct ScenarioRunOutcome {
    /// Overall pass/fail result of the scenario.
    pub result: Result<()>,
    /// Optional structured sub-results for multi-probe scenarios.
    pub sub_results: Vec<ScenarioSubResult>,
}

impl ScenarioRunOutcome {
    /// Create a simple pass outcome with no sub-results.
    #[must_use]
    pub fn pass() -> Self {
        Self {
            result: Ok(()),
            sub_results: Vec::new(),
        }
    }

    /// Create a simple failure outcome with no sub-results.
    #[must_use]
    pub fn fail(error: Error) -> Self {
        Self {
            result: Err(error),
            sub_results: Vec::new(),
        }
    }

    /// Attach sub-results.
    #[must_use]
    pub fn with_sub_results(mut self, sub_results: Vec<ScenarioSubResult>) -> Self {
        self.sub_results = sub_results;
        self
    }
}

impl From<Result<()>> for ScenarioRunOutcome {
    fn from(result: Result<()>) -> Self {
        match result {
            Ok(()) => Self::pass(),
            Err(error) => Self::fail(error),
        }
    }
}

/// A named entry in the run report.
#[derive(Debug)]
pub struct ScenarioResult {
    /// Metadata describing the scenario.
    pub meta: ScenarioMeta,
    /// Outcome of the run.
    pub outcome: ScenarioOutcome,
    /// Structured sub-results, when produced by multi-probe scenarios.
    pub sub_results: Vec<ScenarioSubResult>,
}

/// A behavioral evaluation scenario run against a live instance.
pub trait Scenario: Send + Sync {
    /// Metadata for display and filtering.
    fn meta(&self) -> ScenarioMeta;

    /// Execute the scenario. Return a [`ScenarioRunOutcome`]; `result` is `Ok(())`
    /// for an overall pass and `Err` for an overall failure.
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a>;
}

/// Define a scenario with less boilerplate.
///
/// Generates a unit struct, a [`Scenario`] trait impl with `meta()` returning
/// [`ScenarioMeta`], and a `run()` method wrapped in `Box::pin` with a tracing
/// `instrument` span.
///
/// # Required fields
///
/// - `id`: unique scenario identifier (string literal)
/// - `description`: human-readable description (string literal)
/// - `category`: grouping category (string literal)
///
/// # Optional fields (default to `false` / `None`)
///
/// - `requires_auth`: whether the scenario needs an auth token
/// - `requires_nous`: whether the scenario needs a configured nous
/// - `expected_contains`: substring the response must contain
/// - `expected_pattern`: regex the response must match
/// - `classification`: scenario intent (default `Assertive`)
///
/// # Body
///
/// Assert a condition, returning an assertion error if it fails.
#[tracing::instrument(skip_all)]
pub(crate) fn assert_eval(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        error::AssertionSnafu {
            message: message.into(),
        }
        .fail()
    }
}

/// Assert two values are equal.
#[tracing::instrument(skip_all)]
pub(crate) fn assert_eq_eval<T: PartialEq + std::fmt::Debug>(
    left: &T,
    right: &T,
    context: &str,
) -> Result<()> {
    if left == right {
        Ok(())
    } else {
        error::AssertionSnafu {
            message: format!("{context}: expected {left:?}, got {right:?}"),
        }
        .fail()
    }
}

/// Validate a response string against the scenario's semantic criteria.
///
/// For [`ScenarioClassification::Assertive`] scenarios, at least one of
/// `expected_contains` or `expected_pattern` must be set; otherwise this
/// returns an error rather than accepting any non-empty response. Smoke and
/// informational scenarios may lack explicit criteria and only require a
/// non-empty response.
// kanon:ignore RUST/validate-returns-unit — semantic validation returns Ok/Err; no meaningful value to parse beyond pass/fail
#[tracing::instrument(skip(text), fields(scenario_id = meta.id, text_len = text.len()))]
pub(crate) fn validate_response(meta: &ScenarioMeta, text: &str) -> Result<()> {
    if meta.expected_contains.is_none() && meta.expected_pattern.is_none() {
        match meta.classification {
            ScenarioClassification::Assertive => {
                return error::AssertionSnafu {
                    message: format!(
                        "assertive scenario {} has no validation criteria; \
                         non-empty responses cannot be treated as a semantic pass",
                        meta.id
                    ),
                }
                .fail();
            }
            ScenarioClassification::Smoke | ScenarioClassification::Informational => {
                tracing::warn!(
                    scenario_id = meta.id,
                    "no validation criteria specified; accepting any non-empty response"
                );
                return assert_eval(!text.is_empty(), "response should not be empty");
            }
        }
    }

    if let Some(keyword) = meta.expected_contains {
        assert_eval(
            text.contains(keyword),
            format!("response missing expected text: {keyword:?}"),
        )?;
    }

    if let Some(pattern) = meta.expected_pattern {
        let re = Regex::new(pattern).map_err(|e| {
            error::AssertionSnafu {
                message: format!("invalid expected_pattern {pattern:?}: {e}"),
            }
            .build()
        })?;
        assert_eval(
            re.is_match(text),
            format!("response does not match expected_pattern: {pattern:?}"),
        )?;
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn assert_eval_passes_on_true() {
        let result = assert_eval(true, "ok");
        assert!(result.is_ok());
    }

    #[test]
    fn assert_eval_fails_on_false() {
        let result = assert_eval(false, "fail");
        assert!(result.is_err());
    }

    #[test]
    fn assert_eq_eval_equal_values() {
        let result = assert_eq_eval(&1, &1, "ctx");
        assert!(result.is_ok());
    }

    #[test]
    fn assert_eq_eval_different_values() {
        let result = assert_eq_eval(&1, &2, "ctx");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("ctx"));
    }

    fn meta_with_criteria(
        expected_contains: Option<&'static str>,
        expected_pattern: Option<&'static str>,
    ) -> ScenarioMeta {
        ScenarioMeta {
            id: "test",
            description: "test",
            category: "test",
            requires_auth: false,
            requires_nous: false,
            expected_contains,
            expected_pattern,
            classification: ScenarioClassification::Assertive,
        }
    }

    #[test]
    fn validate_response_no_criteria_assertive_fails() {
        let meta = meta_with_criteria(None, None);
        assert!(validate_response(&meta, "some text").is_err());
    }

    #[test]
    fn validate_response_no_criteria_smoke_accepts_nonempty() {
        let mut meta = meta_with_criteria(None, None);
        meta.classification = ScenarioClassification::Smoke;
        assert!(validate_response(&meta, "some text").is_ok());
    }

    #[test]
    fn validate_response_no_criteria_smoke_rejects_empty() {
        let mut meta = meta_with_criteria(None, None);
        meta.classification = ScenarioClassification::Smoke;
        assert!(validate_response(&meta, "").is_err());
    }

    #[test]
    fn validate_response_no_criteria_informational_accepts_nonempty() {
        let mut meta = meta_with_criteria(None, None);
        meta.classification = ScenarioClassification::Informational;
        assert!(validate_response(&meta, "some text").is_ok());
    }

    #[test]
    fn validate_response_expected_contains_passes() {
        let meta = meta_with_criteria(Some("hello"), None);
        assert!(validate_response(&meta, "say hello world").is_ok());
    }

    #[test]
    fn validate_response_expected_contains_fails() {
        let meta = meta_with_criteria(Some("hello"), None);
        assert!(validate_response(&meta, "goodbye world").is_err());
    }

    #[test]
    fn validate_response_expected_pattern_passes() {
        let meta = meta_with_criteria(None, Some(r"\d+"));
        assert!(validate_response(&meta, "answer is 42").is_ok());
    }

    #[test]
    fn validate_response_expected_pattern_fails() {
        let meta = meta_with_criteria(None, Some(r"\d+"));
        assert!(validate_response(&meta, "no digits here").is_err());
    }

    #[test]
    fn validate_response_invalid_pattern_returns_error() {
        let meta = meta_with_criteria(None, Some(r"[invalid"));
        assert!(validate_response(&meta, "text").is_err());
    }

    #[test]
    fn scenario_run_outcome_pass_has_ok_result() {
        let outcome = ScenarioRunOutcome::pass();
        assert!(outcome.result.is_ok());
        assert!(outcome.sub_results.is_empty());
    }

    #[test]
    fn scenario_run_outcome_fail_has_err_result() {
        let err = error::AssertionSnafu {
            message: "boom".to_owned(),
        }
        .build();
        let outcome = ScenarioRunOutcome::fail(err);
        assert!(outcome.result.is_err());
    }

    #[test]
    fn scenario_run_outcome_with_sub_results() {
        let sub = ScenarioSubResult {
            sub_id: "p1".to_owned(),
            classification: ScenarioClassification::Assertive,
            passed: true,
            criteria: None,
            response_excerpt: None,
            violation_ids: vec![],
        };
        let outcome = ScenarioRunOutcome::pass().with_sub_results(vec![sub]);
        assert_eq!(outcome.sub_results.len(), 1);
    }

    #[test]
    fn meta_criteria_summary_contains() {
        let meta = meta_with_criteria(Some("hello"), None);
        assert_eq!(
            meta.criteria_summary(),
            Some("contains: \"hello\"".to_owned())
        );
    }

    #[test]
    fn meta_criteria_summary_pattern() {
        let meta = meta_with_criteria(None, Some(r"\d+"));
        assert_eq!(
            meta.criteria_summary(),
            Some("pattern: \"\\\\d+\"".to_owned())
        );
    }
}
