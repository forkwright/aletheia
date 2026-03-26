//! Scenario trait and outcome types.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use regex::Regex;

use crate::client::EvalClient;
use crate::error::{self, Error, Result};

/// Boxed future returned by scenario `run` methods.
pub(crate) type ScenarioFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

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
}

/// Result of running a single scenario.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScenarioOutcome {
    /// Scenario completed within timeout without errors.
    Passed { duration: Duration },
    /// Scenario returned an error or assertion failed.
    Failed { duration: Duration, error: Error },
    /// Scenario was not run (e.g. missing auth token or nous).
    Skipped { reason: String },
}

impl ScenarioOutcome {
    /// Returns `true` if the scenario passed.
    #[must_use]
    pub(crate) fn is_passed(&self) -> bool {
        matches!(self, Self::Passed { .. })
    }

    /// Returns `true` if the scenario failed.
    #[must_use]
    pub(crate) fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// A named entry in the run report.
#[derive(Debug)]
pub struct ScenarioResult {
    /// Metadata describing the scenario.
    pub meta: ScenarioMeta,
    /// Outcome of the run.
    pub outcome: ScenarioOutcome,
}

/// A behavioral evaluation scenario run against a live instance.
pub trait Scenario: Send + Sync {
    /// Metadata for display and filtering.
    fn meta(&self) -> ScenarioMeta;

    /// Execute the scenario. Return `Ok(())` for pass, `Err` for failure.
    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a>;
}

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
/// When neither `expected_contains` nor `expected_pattern` is set, accepts any
/// non-empty response and logs a warning about missing validation criteria.
#[tracing::instrument(skip(text), fields(scenario_id = meta.id, text_len = text.len()))]
pub(crate) fn validate_response(meta: &ScenarioMeta, text: &str) -> Result<()> {
    if meta.expected_contains.is_none() && meta.expected_pattern.is_none() {
        tracing::warn!(
            scenario_id = meta.id,
            "no validation criteria specified; accepting any non-empty response"
        );
        return assert_eval(!text.is_empty(), "response should not be empty");
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
        }
    }

    #[test]
    fn validate_response_no_criteria_accepts_nonempty() {
        let meta = meta_with_criteria(None, None);
        assert!(validate_response(&meta, "some text").is_ok());
    }

    #[test]
    fn validate_response_no_criteria_rejects_empty() {
        let meta = meta_with_criteria(None, None);
        assert!(validate_response(&meta, "").is_err());
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
}
