//! Reflection cron: periodic self-reflection prompt.

use std::time::Duration;

/// Configuration for the reflection cron task.
#[derive(Debug, Clone)]
pub struct CronReflectionConfig {
    /// Whether the reflection cron is enabled.
    pub enabled: bool,
    /// Interval between reflection runs.
    pub interval: Duration,
}

impl Default for CronReflectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: Duration::from_secs(24 * 3600),
        }
    }
}

/// Execute the reflection cron: dispatch a self-reflection prompt via the bridge.
///
/// The agent receives a prompt instructing it to:
/// 1. Review recent session performance
/// 2. Identify patterns, contradictions, and corrections
/// 3. Consolidate learnings into memory
pub(crate) async fn execute_reflection(
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
        "Run reflection cycle: review your recent sessions and performance. ",
        "Identify patterns, recurring corrections, contradictions in your knowledge, ",
        "and areas where you could improve. Consolidate any insights into your ",
        "long-term memory. Evaluate your strengths and weaknesses by domain."
    );

    match bridge
        .send_prompt(nous_id, "daemon:reflection", prompt)
        .await
    {
        Ok(result) => {
            tracing::info!(
                nous_id = %nous_id,
                success = result.success,
                "reflection cron: dispatch succeeded"
            );
            Ok(crate::runner::ExecutionResult {
                success: true,
                output: Some("reflection cycle dispatched".to_owned()),
            })
        }
        Err(e) => {
            tracing::warn!(
                nous_id = %nous_id,
                error = %e,
                "reflection cron: dispatch failed"
            );
            Ok(crate::runner::ExecutionResult {
                success: false,
                output: Some(format!("reflection dispatch failed: {e}")),
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
        let config = CronReflectionConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval, Duration::from_secs(24 * 3600));
    }

    #[tokio::test]
    async fn execute_without_bridge_returns_failure() {
        let result = execute_reflection("test-nous", None)
            .await
            .expect("should not error");
        assert!(!result.success);
        assert!(result.output.expect("has output").contains("no bridge"));
    }
}
