//! Integration test for the graphe retention path exposed through oikonomos.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;

use oikonomos::maintenance::{MaintenanceConfig, RetentionExecutor, RetentionSummary};
use oikonomos::runner::TaskRunner;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct MockRetentionExecutor;

impl RetentionExecutor for MockRetentionExecutor {
    fn execute_retention(&self) -> oikonomos::error::Result<RetentionSummary> {
        Ok(RetentionSummary::default())
    }
}

#[test]
fn daemon_registers_retention_executor_when_enabled() {
    let token = CancellationToken::new();
    let mut config = MaintenanceConfig::default();
    config.retention.enabled = true;

    let executor: Arc<dyn RetentionExecutor> = Arc::new(MockRetentionExecutor);
    let mut runner = TaskRunner::new("system", token)
        .with_maintenance(config)
        .with_retention(executor);
    runner.register_maintenance_tasks();

    let statuses = runner.status();
    let retention = statuses
        .iter()
        .find(|status| status.id == "retention-execution")
        .expect("retention executor task should be registered");

    assert_eq!(retention.name, "Data retention cleanup");
    assert!(retention.enabled);
}
