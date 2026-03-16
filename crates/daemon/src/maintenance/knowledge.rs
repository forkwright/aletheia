//! Knowledge maintenance bridge trait and report types.
//!
//! The daemon crate defines this interface; the binary crate implements it
//! where concrete types (`KnowledgeStore`) are available. All methods are
//! blocking (CozoDB is sync): the runner wraps calls in `spawn_blocking`.

use serde::{Deserialize, Serialize};

/// Outcome of a single knowledge maintenance operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaintenanceReport {
    /// Total items examined during the operation.
    pub items_processed: u64,
    /// Items that were actually changed.
    pub items_modified: u64,
    /// Number of non-fatal errors encountered.
    pub errors: u32,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Optional human-readable detail string.
    pub detail: Option<String>,
}

/// Bridge trait for knowledge graph maintenance operations.
///
/// Daemon crate defines it, binary crate implements it where `KnowledgeStore`
/// is available. All methods are blocking: the runner wraps in `spawn_blocking`.
pub trait KnowledgeMaintenanceExecutor: Send + Sync {
    /// Refresh temporal decay scores for all entities/edges.
    fn refresh_decay_scores(&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;

    /// Find and merge duplicate entities.
    fn deduplicate_entities(&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;

    /// Recompute graph-wide scores (`PageRank`, centrality, etc.).
    fn recompute_graph_scores(&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;

    /// Re-embed entities whose embeddings are stale or missing.
    fn refresh_embeddings(&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;

    /// Remove orphaned nodes, expired edges, and other detritus.
    fn garbage_collect(&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;

    /// Rebuild or optimize graph indexes.
    fn maintain_indexes(&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;

    /// Run a diagnostic health check on the knowledge graph.
    fn health_check(&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;

    /// Compute decay scores for skills and retire stale ones.
    fn run_skill_decay(&self, nous_id: &str) -> crate::error::Result<MaintenanceReport>;
}

/// Configuration for knowledge maintenance task scheduling.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeMaintenanceConfig {
    /// Whether knowledge maintenance tasks are enabled.
    pub enabled: bool,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    struct MockExecutor;

    impl KnowledgeMaintenanceExecutor for MockExecutor {
        fn refresh_decay_scores(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport {
                items_processed: 10,
                items_modified: 3,
                ..Default::default()
            })
        }

        fn deduplicate_entities(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn recompute_graph_scores(
            &self,
            _nous_id: &str,
        ) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn refresh_embeddings(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn garbage_collect(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn maintain_indexes(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn health_check(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport::default())
        }

        fn run_skill_decay(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport {
                items_processed: 5,
                items_modified: 1,
                detail: Some("Skill decay: 4 active, 0 needs_review, 1 retired".to_owned()),
                ..Default::default()
            })
        }
    }

    #[test]
    fn mock_executor_returns_report() {
        let executor = MockExecutor;
        let report = executor
            .refresh_decay_scores("test-nous")
            .expect("should succeed");
        assert_eq!(report.items_processed, 10);
        assert_eq!(report.items_modified, 3);
    }

    #[test]
    fn maintenance_report_default() {
        let report = MaintenanceReport::default();
        assert_eq!(report.items_processed, 0);
        assert_eq!(report.items_modified, 0);
        assert_eq!(report.errors, 0);
        assert_eq!(report.duration_ms, 0);
        assert!(report.detail.is_none());
    }

    #[test]
    fn default_config_is_disabled() {
        let config = KnowledgeMaintenanceConfig::default();
        assert!(!config.enabled);
    }

    #[test]
    fn maintenance_report_serialization_roundtrip() {
        let report = MaintenanceReport {
            items_processed: 42,
            items_modified: 7,
            errors: 1,
            duration_ms: 1234,
            detail: Some("test detail".to_owned()),
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let back: MaintenanceReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.items_processed, 42);
        assert_eq!(back.items_modified, 7);
        assert_eq!(back.errors, 1);
        assert_eq!(back.duration_ms, 1234);
        assert_eq!(back.detail.as_deref(), Some("test detail"));
    }
}
