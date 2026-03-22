//! Evolution cron: periodic configuration variant search.

use std::time::Duration;

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
            interval: Duration::from_secs(24 * 3600),
        }
    }
}

/// Execute the evolution cron: dispatch a config variant search prompt via the bridge.
///
/// The agent receives a prompt instructing it to:
/// 1. Mutate its current pipeline configuration
/// 2. Evaluate variant performance against benchmarks
/// 3. Promote variants that show measurable improvement
pub(crate) async fn execute_evolution(
    nous_id: &str,
    bridge: Option<&dyn crate::bridge::DaemonBridge>,
) -> crate::error::Result<crate::runner::ExecutionResult> {
    let Some(bridge) = bridge else {
        return Ok(crate::runner::ExecutionResult {
            success: false,
            output: Some("no bridge configured".to_owned()),
        });
    };

    let prompt = concat!(
        "Run evolution cycle: review your current configuration, ",
        "generate a variant with adjusted model parameters, ",
        "and evaluate the variant against recent session outcomes. ",
        "If the variant shows improvement, record it for promotion."
    );

    match bridge
        .send_prompt(nous_id, "daemon:evolution", prompt)
        .await
    {
        Ok(result) => {
            tracing::info!(
                nous_id = %nous_id,
                success = result.success,
                "evolution cron: dispatch succeeded"
            );
            Ok(crate::runner::ExecutionResult {
                success: true,
                output: Some("evolution cycle dispatched".to_owned()),
            })
        }
        Err(e) => {
            tracing::warn!(
                nous_id = %nous_id,
                error = %e,
                "evolution cron: dispatch failed"
            );
            Ok(crate::runner::ExecutionResult {
                success: false,
                output: Some(format!("evolution dispatch failed: {e}")),
            })
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config_disabled() {
        let config = CronEvolutionConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval, Duration::from_secs(24 * 3600));
    }

    #[tokio::test]
    async fn execute_without_bridge_returns_failure() {
        let result = execute_evolution("test-nous", None)
            .await
            .expect("should not error");
        assert!(!result.success);
        assert!(result.output.expect("has output").contains("no bridge"));
    }
}
