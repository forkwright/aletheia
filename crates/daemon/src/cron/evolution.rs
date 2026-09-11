//! Evolution cron: periodic configuration variant search.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

/// Configuration for the evolution cron task.
#[derive(Debug, Clone)]
pub struct CronEvolutionConfig {
    /// Whether the evolution cron is enabled.
    pub enabled: bool,
    /// Interval between evolution runs.
    pub interval: Duration,
}

impl Default for CronEvolutionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: Duration::from_hours(24),
        }
    }
}

/// Execute the evolution cron: dispatch a config variant search prompt via the bridge.
///
/// The agent receives a prompt instructing it to:
/// 1. Mutate its current pipeline configuration
/// 2. Evaluate variant performance against benchmarks
/// 3. Promote variants that show measurable improvement
#[tracing::instrument(skip_all)]
pub(crate) async fn execute_evolution(
    nous_id: &str,
    bridge: Option<&dyn crate::bridge::DaemonBridge>,
    cancel: CancellationToken,
) -> crate::error::Result<crate::runner::ExecutionResult> {
    let Some(bridge) = bridge else {
        return Ok(crate::runner::ExecutionResult::skipped(Some(
            "no bridge configured".to_owned(),
        )));
    };

    let prompt = concat!(
        "Run evolution cycle: review your current configuration, ",
        "generate a variant with adjusted model parameters, ",
        "and evaluate the variant against recent session outcomes. ",
        "If the variant shows improvement, record it for promotion."
    );

    match bridge
        .send_prompt_with_cancel(nous_id, "daemon:evolution", prompt, cancel)
        .await
    {
        // WHY(#7252): return the bridge's own classification unchanged — a
        // failed turn must record a failed task run, not a completed one.
        Ok(result) => {
            tracing::info!(
                nous_id = %nous_id,
                outcome = ?result.outcome,
                "evolution cron: dispatch returned"
            );
            Ok(result)
        }
        Err(e) => {
            tracing::warn!(
                nous_id = %nous_id,
                error = %e,
                "evolution cron: dispatch failed"
            );
            Ok(crate::runner::ExecutionResult::failed(Some(format!(
                "evolution dispatch failed: {e}"
            ))))
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    /// Bridge returning a canned `ExecutionResult`, for asserting the cron
    /// arm propagates the turn's own outcome (#7252).
    struct FixedOutcomeBridge(crate::runner::ExecutionResult);

    impl crate::bridge::DaemonBridge for FixedOutcomeBridge {
        fn send_prompt(
            &self,
            _nous_id: &str,
            _session_key: &str,
            _prompt: &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::error::Result<crate::runner::ExecutionResult>,
                    > + Send
                    + '_,
            >,
        > {
            let result = self.0.clone();
            Box::pin(async move { Ok(result) })
        }
    }

    #[test]
    fn default_config_disabled() {
        let config = CronEvolutionConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval, Duration::from_hours(24));
    }

    #[tokio::test]
    async fn execute_without_bridge_returns_failure() {
        let result = execute_evolution("test-nous", None, CancellationToken::new())
            .await
            .expect("should not error");
        assert!(!result.is_success());
        assert!(result.output.expect("has output").contains("no bridge"));
    }

    /// WHY(#7252): a failed turn must record a failed task run — the old
    /// code logged the bridge's success flag and then reported success.
    #[tokio::test]
    async fn failed_turn_propagates_as_failed_outcome() {
        let bridge = FixedOutcomeBridge(crate::runner::ExecutionResult::failed(Some(
            "turn failed: history load_failed".to_owned(),
        )));
        let result = execute_evolution("test-nous", Some(&bridge), CancellationToken::new())
            .await
            .expect("bridge failure arrives as Ok(failed), not Err");
        assert_eq!(
            result.outcome,
            crate::runner::TaskOutcome::Failed,
            "a failed turn must propagate to the task outcome"
        );
        assert_eq!(
            result.output.as_deref(),
            Some("turn failed: history load_failed"),
            "the turn's error must surface as the task output"
        );
    }

    #[tokio::test]
    async fn successful_turn_propagates_as_success_outcome() {
        let bridge = FixedOutcomeBridge(crate::runner::ExecutionResult::success(Some(
            "evolution report".to_owned(),
        )));
        let result = execute_evolution("test-nous", Some(&bridge), CancellationToken::new())
            .await
            .expect("should not error");
        assert!(result.is_success());
        assert_eq!(result.output.as_deref(), Some("evolution report"));
    }
}
