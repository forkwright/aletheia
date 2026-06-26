//! Knowledge maintenance bridge trait and report types.
//!
//! The daemon crate defines this interface; the binary crate implements it
//! where concrete types (`KnowledgeStore`) are available. All methods are
//! blocking: the runner wraps calls in `spawn_blocking`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Discrete outcome for a knowledge maintenance task.
///
/// Maps [`MaintenanceReport::errors`] to the task-policy outcome expected by
/// the runner: complete success, completed with non-fatal errors (degraded),
/// or an unrecoverable failure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MaintenanceOutcome {
    /// No errors were reported; the task completed cleanly.
    #[default]
    Success,
    /// The task finished but reported non-fatal errors; operators should
    /// review the detail string but the task is not retried as a hard failure.
    Degraded,
    /// The task could not complete.
    Failure,
}

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

impl MaintenanceReport {
    /// Classify this report into a discrete task outcome.
    ///
    /// Any non-fatal error count degrades the result; callers should map
    /// unhandled `Err` returns to [`MaintenanceOutcome::Failure`].
    #[must_use]
    pub fn outcome(&self) -> MaintenanceOutcome {
        if self.errors == 0 {
            MaintenanceOutcome::Success
        } else {
            MaintenanceOutcome::Degraded
        }
    }
}

/// Bridge trait for knowledge graph maintenance operations.
///
/// Daemon crate defines it, binary crate implements it where `KnowledgeStore`
/// is available. All methods are blocking: the runner wraps in `spawn_blocking`.
pub trait KnowledgeMaintenanceExecutor: Send + Sync {
    /// Insert a single fact into the durable knowledge store.
    fn insert_fact(&self, fact: &episteme::knowledge::Fact) -> crate::error::Result<()>;

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

    /// Materialize derived Datalog rules into the `derived_facts` relation.
    ///
    /// Runs all three rule families (ontological IS-A closure, transitive causal
    /// chains, defeasible defaults) in sequence and persists results. Returns the
    /// total number of derived facts written.
    fn materialize_derived_facts(&self) -> crate::error::Result<MaintenanceReport>;

    /// Run the serendipity discovery engine over recently active entities.
    fn discover_serendipitous_facts(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<MaintenanceReport>;

    /// Consolidate overflowing facts into summarized, higher-quality facts.
    ///
    /// The default implementation returns an empty success report so callers
    /// that do not configure a consolidation provider can still satisfy the
    /// trait without wiring LLM infrastructure.
    fn consolidate_knowledge(&self, _nous_id: &str) -> crate::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
}

/// Policy for derived-rule materialization (#4662).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum DerivedMaterializationPolicy {
    /// Eagerly refresh derived facts as part of the scheduled daemon task.
    /// This is the production default: derived results are materialized in
    /// the background and query surfaces only need to check the watermark.
    #[default]
    Scheduled,
    /// Refresh derived facts on demand when a freshness check finds stale
    /// rows. This trades query latency for lower background write volume.
    OnDemand,
}

/// Configuration for derived-rule maintenance (#4662).
#[derive(Debug, Clone)]
pub struct DerivedRulesConfig {
    /// How derived facts are refreshed.
    pub policy: DerivedMaterializationPolicy,
    /// Cadence for the scheduled materialization task when policy is
    /// [`DerivedMaterializationPolicy::Scheduled`].
    pub materialization_interval: Duration,
}

impl Default for DerivedRulesConfig {
    fn default() -> Self {
        // WHY: every 6 hours balances freshness of IS-A closure / causal
        // chains against the cost of a full Datalog fixpoint pass.
        Self {
            policy: DerivedMaterializationPolicy::Scheduled,
            materialization_interval: Duration::from_hours(6),
        }
    }
}

/// Configuration for knowledge maintenance task scheduling.
#[derive(Debug, Clone)]
pub struct KnowledgeMaintenanceConfig {
    /// Whether knowledge maintenance tasks are enabled.
    pub enabled: bool,
    /// Auto-dream consolidation settings.
    pub auto_dream: AutoDreamConfig,
    /// Serendipity discovery idle-maintenance settings.
    pub serendipity: SerendipityMaintenanceConfig,
    /// Derived Datalog rule maintenance settings.
    pub derived_rules: DerivedRulesConfig,
    /// Cadence for the gnosis code-graph index rebuild task.
    ///
    /// WHY: issue #5963 asks for an automatic gnosis rebuild trigger; this
    /// interval drives the daemon `index-maintenance` task that re-indexes
    /// the workspace source tree.
    pub index_maintenance_interval: Duration,
}

impl Default for KnowledgeMaintenanceConfig {
    fn default() -> Self {
        // WHY: every hour keeps the code-graph index reasonably fresh without
        // monopolizing I/O during active development. Operators can tune it.
        Self {
            enabled: false,
            auto_dream: AutoDreamConfig::default(),
            serendipity: SerendipityMaintenanceConfig::default(),
            derived_rules: DerivedRulesConfig::default(),
            index_maintenance_interval: Duration::from_secs(60 * 60),
        }
    }
}

/// Configuration for auto-dream memory consolidation.
///
/// Controls the background consolidation process that periodically merges
/// session transcripts into the knowledge graph.
#[derive(Debug, Clone)]
pub struct AutoDreamConfig {
    /// Whether auto-dream consolidation is enabled.
    pub enabled: bool,
    /// Minimum hours between consolidation runs.
    pub min_hours: u64,
    /// Minimum sessions required to trigger consolidation.
    pub min_sessions: usize,
    /// Session scan throttle interval in seconds.
    pub scan_interval_secs: i64,
    /// Stale lock threshold in seconds.
    pub stale_threshold_secs: i64,
}

impl Default for AutoDreamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_hours: 24,
            min_sessions: 5,
            scan_interval_secs: 600,
            stale_threshold_secs: 3_600,
        }
    }
}

/// Configuration for serendipity discovery maintenance.
#[derive(Debug, Clone)]
pub struct SerendipityMaintenanceConfig {
    /// Whether serendipity discovery is enabled.
    pub enabled: bool,
    /// Cron cadence for the scheduled discovery task.
    pub cadence: String,
}

impl Default for SerendipityMaintenanceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cadence: "0 0 7 * * *".to_owned(),
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    struct MockExecutor;

    impl KnowledgeMaintenanceExecutor for MockExecutor {
        fn insert_fact(&self, _fact: &episteme::knowledge::Fact) -> crate::error::Result<()> {
            Ok(())
        }

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

        fn materialize_derived_facts(&self) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport {
                items_processed: 0,
                items_modified: 0,
                detail: Some("Derived facts materialized: 0".to_owned()),
                ..Default::default()
            })
        }

        fn discover_serendipitous_facts(
            &self,
            _nous_id: &str,
        ) -> crate::error::Result<MaintenanceReport> {
            Ok(MaintenanceReport {
                items_processed: 0,
                items_modified: 0,
                detail: Some("Serendipity discovery: 0 discoveries surfaced".to_owned()),
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
        assert!(!config.serendipity.enabled);
        assert_eq!(config.serendipity.cadence, "0 0 7 * * *");
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
