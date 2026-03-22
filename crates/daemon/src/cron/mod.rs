//! Cron task definitions for periodic agent maintenance operations.

/// Periodic configuration variant search: mutate and benchmark agent pipeline configs.
pub(crate) mod evolution;
/// Periodic knowledge graph cleanup: orphan removal, stale entity pruning.
pub(crate) mod graph_cleanup;
/// Periodic self-reflection: agent evaluates its own recent performance.
pub(crate) mod reflection;

pub use evolution::CronEvolutionConfig;
pub use graph_cleanup::CronGraphCleanupConfig;
pub use reflection::CronReflectionConfig;

/// Aggregated cron task configuration. All tasks disabled by default.
#[derive(Debug, Clone, Default)]
pub struct CronConfig {
    /// Evolution: periodic configuration variant search.
    pub evolution: CronEvolutionConfig,
    /// Reflection: periodic self-reflection prompt.
    pub reflection: CronReflectionConfig,
    /// Graph cleanup: periodic knowledge graph orphan and stale entity removal.
    pub graph_cleanup: CronGraphCleanupConfig,
}
