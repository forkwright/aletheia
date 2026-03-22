//! Graph maintenance cron: periodic knowledge graph cleanup.

use std::sync::Arc;
use std::time::Duration;

use snafu::ResultExt;

use crate::maintenance::KnowledgeMaintenanceExecutor;

/// Configuration for the graph cleanup cron task.
#[derive(Debug, Clone)]
pub struct CronGraphCleanupConfig {
    /// Whether the graph cleanup cron is enabled.
    pub enabled: bool,
    /// Interval between cleanup runs.
    pub interval: Duration,
}

impl Default for CronGraphCleanupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: Duration::from_secs(7 * 24 * 3600),
        }
    }
}

/// Execute the graph cleanup cron: orphan removal and stale entity pruning.
///
/// Delegates to the `KnowledgeMaintenanceExecutor` to:
/// 1. Remove orphaned nodes with no relationships
/// 2. Prune stale entities past their validity window
/// 3. Clean up expired edges
pub(crate) async fn execute_graph_cleanup(
    nous_id: &str,
    knowledge_executor: Option<Arc<dyn KnowledgeMaintenanceExecutor>>,
) -> crate::error::Result<crate::runner::ExecutionResult> {
    let Some(executor) = knowledge_executor else {
        return Ok(crate::runner::ExecutionResult {
            success: false,
            output: Some("no knowledge executor configured".to_owned()),
        });
    };

    let nous_id_owned = nous_id.to_owned();
    let report = tokio::task::spawn_blocking(move || {
        let start = std::time::Instant::now();
        let mut report = executor.garbage_collect(&nous_id_owned)?;
        report.duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        tracing::info!(
            nous_id = %nous_id_owned,
            items_processed = report.items_processed,
            items_modified = report.items_modified,
            duration_ms = report.duration_ms,
            "graph cleanup cron: complete"
        );

        Ok(report)
    })
    .await
    .context(crate::error::BlockingJoinSnafu {
        context: "graph cleanup cron",
    })??;

    Ok(crate::runner::ExecutionResult {
        success: true,
        output: Some(format!(
            "graph cleanup: {} processed, {} removed in {}ms",
            report.items_processed, report.items_modified, report.duration_ms
        )),
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config_disabled() {
        let config = CronGraphCleanupConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval, Duration::from_secs(7 * 24 * 3600));
    }

    #[tokio::test]
    async fn execute_without_executor_returns_failure() {
        let result = execute_graph_cleanup("test-nous", None)
            .await
            .expect("should not error");
        assert!(!result.success);
        assert!(
            result
                .output
                .expect("has output")
                .contains("no knowledge executor")
        );
    }
}
