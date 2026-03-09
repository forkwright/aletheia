//! Scenario trait and outcome types.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::client::EvalClient;
use crate::error::{self, Error, Result};

/// Boxed future returned by scenario `run` methods.
pub type ScenarioFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

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
}

/// Result of running a single scenario.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScenarioOutcome {
    Passed { duration: Duration },
    Failed { duration: Duration, error: Error },
    Skipped { reason: String },
}

impl ScenarioOutcome {
    pub fn is_passed(&self) -> bool {
        matches!(self, Self::Passed { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// A named entry in the run report.
#[derive(Debug)]
pub struct ScenarioResult {
    pub meta: ScenarioMeta,
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
pub fn assert_eval(condition: bool, message: impl Into<String>) -> Result<()> {
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
pub fn assert_eq_eval<T: PartialEq + std::fmt::Debug>(
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

#[cfg(test)]
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
}
