//! Prosoche (προσοχή): "directed attention." Periodic check-in that monitors
//! calendar, tasks, and system health for a nous.

use std::path::{Path, PathBuf};
#[cfg(feature = "knowledge-store")]
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use serde::{Deserialize, Serialize};

/// Prosoche attention check runner.
pub(crate) struct ProsocheCheck {
    nous_id: String,
    /// Instance data directory to check for disk usage.
    data_dir: Option<PathBuf>,
    /// Database file paths to check sizes.
    db_paths: Vec<PathBuf>,
    /// Optional knowledge store for memory consistency checks.
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<episteme::knowledge_store::KnowledgeStore>>,
    /// Number of recent facts sampled by anomaly detection.
    anomaly_sample_size: usize,
    /// Cancellation token propagated from the task runner.
    ///
    /// WHY: a hung `df` on an NFS/automount path should not hold the in-flight
    /// slot if the runner asks the check to stop.
    cancel: CancellationToken,
}

impl std::fmt::Debug for ProsocheCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("ProsocheCheck");
        s.field("nous_id", &self.nous_id)
            .field("data_dir", &self.data_dir)
            .field("db_paths", &self.db_paths)
            .field("anomaly_sample_size", &self.anomaly_sample_size);
        #[cfg(feature = "knowledge-store")]
        s.field(
            "knowledge_store",
            &self.knowledge_store.as_ref().map(|_| "<KnowledgeStore>"),
        );
        s.finish_non_exhaustive()
    }
}

impl Clone for ProsocheCheck {
    fn clone(&self) -> Self {
        Self {
            nous_id: self.nous_id.clone(),
            data_dir: self.data_dir.clone(),
            db_paths: self.db_paths.clone(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: self.knowledge_store.clone(),
            anomaly_sample_size: self.anomaly_sample_size,
            cancel: self.cancel.clone(),
        }
    }
}

/// Result of a prosoche check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProsocheResult {
    /// Items requiring the nous's attention.
    pub(crate) items: Vec<AttentionItem>,
    /// ISO 8601 timestamp when the check was performed.
    pub(crate) checked_at: String,
}

/// A single item requiring attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AttentionItem {
    /// What kind of attention is needed.
    pub(crate) category: AttentionCategory,
    /// Human-readable description of the item.
    pub(crate) summary: String,
    /// How urgently this needs attention.
    pub(crate) urgency: Urgency,
}

/// Categories of attention items.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum AttentionCategory {
    /// Calendar event or deadline.
    Calendar,
    /// Pending task or overdue item.
    Task,
    /// System health issue (disk, memory, service status).
    SystemHealth,
    /// Knowledge graph consistency anomaly detected via multi-path retrieval.
    MemoryAnomaly,
    /// Application-defined attention category.
    Custom(String),
}

/// Urgency level for attention items.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Urgency {
    /// Informational, no action needed soon.
    Low,
    /// Should be addressed within the current session.
    Medium,
    /// Needs attention soon (within hours).
    High,
    /// Requires immediate action.
    Critical,
}

impl AttentionItem {
    /// Short label for this item's category (used in prompt formatting).
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "attention item label for prosoche prompt formatting"
        )
    )]
    pub(crate) fn category_label(&self) -> &str {
        match &self.category {
            AttentionCategory::Calendar => "calendar",
            AttentionCategory::Task => "task",
            AttentionCategory::SystemHealth => "health",
            AttentionCategory::MemoryAnomaly => "memory-anomaly",
            AttentionCategory::Custom(s) => s,
        }
    }
}

impl ProsocheCheck {
    /// Create a prosoche check for the given nous with default paths.
    pub(crate) fn new(nous_id: &str) -> Self {
        Self {
            nous_id: nous_id.to_owned(),
            data_dir: None,
            db_paths: Vec::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            anomaly_sample_size: DEFAULT_ANOMALY_SAMPLE_SIZE,
            // WHY: a detached token keeps tests and one-off callers working when
            // no runner cancellation source is wired in.
            cancel: CancellationToken::new(),
        }
    }

    /// Apply deployment-tunable daemon behavior.
    #[must_use]
    pub(crate) fn with_daemon_behavior(
        mut self,
        behavior: &taxis::config::DaemonBehaviorConfig,
    ) -> Self {
        self.anomaly_sample_size = behavior.prosoche_anomaly_sample_size;
        self
    }

    /// Set the data directory for disk space checking.
    #[must_use]
    pub(crate) fn with_data_dir(mut self, path: &Path) -> Self {
        self.data_dir = Some(path.to_path_buf());
        self
    }

    /// Set database file paths for size checking.
    #[must_use]
    pub(crate) fn with_db_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.db_paths = paths;
        self
    }

    /// Propagate the runner's cancellation token into long-running checks.
    #[must_use]
    pub(crate) fn with_cancel(mut self, cancel: &CancellationToken) -> Self {
        self.cancel = cancel.clone();
        self
    }

    /// Set the knowledge store for memory consistency checking.
    ///
    /// When set, `run()` will perform multi-path consistency checks on a sample
    /// of recent facts, comparing direct Datalog query results against entity
    /// traversal paths (A-MemGuard consensus approach).
    #[must_use]
    #[cfg(feature = "knowledge-store")]
    pub(crate) fn with_knowledge_store(
        mut self,
        store: Arc<episteme::knowledge_store::KnowledgeStore>,
    ) -> Self {
        self.knowledge_store = Some(store);
        self
    }

    /// Run the attention check. Returns items needing attention.
    ///
    /// Performs real system health checks:
    /// - Disk space on the data directory
    /// - Database file sizes
    /// - Process memory (RSS) via /proc/self/status
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Each health check is independent and partial results
    /// are discarded on cancellation. No state is mutated.
    #[tracing::instrument(skip_all)]
    pub(crate) async fn run(&self) -> crate::error::Result<ProsocheResult> {
        let mut items = Vec::new();

        if let Some(ref data_dir) = self.data_dir {
            items.extend(check_disk_space(data_dir, &self.cancel).await);
        }

        // WHY: database metadata is a blocking syscall; offload it from the
        // Tokio executor so worker threads remain available for actor turns.
        items.extend(
            tokio::task::spawn_blocking({
                let paths = self.db_paths.clone();
                move || check_db_sizes(&paths)
            })
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "database size check task failed");
                Vec::new()
            }),
        );

        // WHY: /proc/self/status reads are synchronous filesystem IO; run them
        // on the blocking pool to avoid stalling the async runtime.
        items.extend(
            tokio::task::spawn_blocking(check_memory)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "memory check task failed");
                    Vec::new()
                }),
        );

        #[cfg(feature = "knowledge-store")]
        if let Some(ref store) = self.knowledge_store {
            let checker = MultiPathConsistencyCheck::new(self.anomaly_sample_size);
            match checker.check(store) {
                Ok(anomalies) => items.extend(anomalies),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "memory consistency check failed, skipping"
                    );
                }
            }
        }

        tracing::info!(
            nous_id = %self.nous_id,
            attention_items = items.len(),
            "prosoche check completed"
        );

        Ok(ProsocheResult {
            items,
            checked_at: jiff::Timestamp::now().to_string(),
        })
    }
}

/// Check a numeric value against two thresholds, returning an [`AttentionItem`] if either is exceeded.
///
/// `warn_threshold` maps to [`Urgency::High`], `critical_threshold` to [`Urgency::Critical`].
/// `format_summary` receives the measured value and should return a human-readable description.
fn check_threshold(
    value: f64,
    warn_threshold: f64,
    critical_threshold: f64,
    format_summary: impl FnOnce(f64) -> String,
) -> Option<AttentionItem> {
    let (urgency, summary) = if value >= critical_threshold {
        (Urgency::Critical, format_summary(value))
    } else if value >= warn_threshold {
        (Urgency::High, format_summary(value))
    } else {
        return None;
    };

    Some(AttentionItem {
        category: AttentionCategory::SystemHealth,
        summary,
        urgency,
    })
}

/// Bound the time we wait for `df` so a hung NFS/automount path cannot occupy
/// the prosoche in-flight slot indefinitely.
const DF_TIMEOUT: Duration = Duration::from_secs(10);

/// Check disk space usage on the filesystem containing `path`.
///
/// Uses `df` command output. WARN at 80% usage, CRITICAL at 95%.
async fn check_disk_space(path: &Path, cancel: &CancellationToken) -> Vec<AttentionItem> {
    match disk_usage_percent(path, cancel).await {
        Ok(Some(percent)) => {
            let label = if percent >= 95.0 {
                "critical"
            } else {
                "warning"
            };
            check_threshold(percent, 80.0, 95.0, |p| {
                format!("Disk space {label}: {p:.1}% used on {}", path.display())
            })
            .into_iter()
            .collect()
        }
        Ok(None) => {
            // WHY: cancellation is not a failure; returning an empty vector lets
            // the runner continue with the remaining health checks.
            Vec::new()
        }
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to check disk space"
            );
            Vec::new()
        }
    }
}

/// Get disk usage percentage for the filesystem containing `path` via `df`.
///
/// Returns `Ok(None)` when the cancellation token fires before `df` completes.
async fn disk_usage_percent(
    path: &Path,
    cancel: &CancellationToken,
) -> std::io::Result<Option<f64>> {
    // NOTE: tokio::process::Child kills the child on Drop, providing the same
    // orphan-prevention guarantee as ProcessGuard. If this future is cancelled,
    // tokio kills the child automatically.
    let output = tokio::select! {
        biased;
        () = cancel.cancelled() => return Ok(None),
        result = tokio::time::timeout(
            DF_TIMEOUT,
            tokio::process::Command::new("df")
                .args(["--output=pcent", "--"])
                .arg(path)
                .output()
        ) => match result {
            Ok(output) => output?,
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "df command did not complete within 10 seconds",
                ));
            }
        }
    };

    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_df_percent(&stdout).map(Some)
}

/// Parse the percentage from `df --output=pcent` output.
fn parse_df_percent(output: &str) -> std::io::Result<f64> {
    // NOTE: df --output=pcent output has a header line then lines like " 42%".
    for line in output.lines().skip(1) {
        let trimmed = line.trim().trim_end_matches('%');
        if let Ok(val) = trimmed.parse::<f64>() {
            return Ok(val);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "failed to parse df output",
    ))
}

/// Check database file sizes. WARN if any file > 1 GB.
fn check_db_sizes(paths: &[PathBuf]) -> Vec<AttentionItem> {
    const ONE_GB: u64 = 1024 * 1024 * 1024;
    let mut items = Vec::new();

    for path in paths {
        match std::fs::metadata(path) {
            Ok(meta) => {
                // WHY: compute GB in integer milli-GB (bytes / (GB/1024)) so the
                // usize→f64 conversion is from u32 (lossless up to ~4 petabyte file
                // sizes in milli-GB), avoiding a u64-as-f64 precision-loss cast.
                let milli_gb = meta.len() / (ONE_GB / 1024);
                let milli_gb_u32 = u32::try_from(milli_gb).unwrap_or(u32::MAX);
                let size_gb = f64::from(milli_gb_u32) / 1024.0_f64;
                // NOTE: single threshold — any file over 1 GB is High urgency.
                items.extend(check_threshold(size_gb, 1.0, f64::INFINITY, |gb| {
                    format!("Database file large: {} is {gb:.1} GB", path.display())
                }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // NOTE: File doesn't exist: not an error for health checks.
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to check database file size"
                );
            }
        }
    }

    items
}

/// Check process memory (RSS) via `/proc/self/status` on Linux.
///
/// Returns an attention item if RSS exceeds reasonable thresholds.
#[cfg(target_os = "linux")]
fn check_memory() -> Vec<AttentionItem> {
    match read_process_rss_kb() {
        Ok(resident_kb) => {
            let rss_mb = resident_kb / 1024;
            // WHY: RSS in MB is bounded by host RAM; clamp to u32::MAX (4TB)
            // then f64::from(u32) is lossless, avoiding u64→f64 precision loss.
            let rss_mb_u32 = u32::try_from(rss_mb).unwrap_or(u32::MAX);
            let rss_f64 = f64::from(rss_mb_u32);
            let label = if rss_mb >= 2048 { "critical" } else { "high" };
            check_threshold(rss_f64, 1024.0, 2048.0, |mb| {
                format!("Process memory {label}: {mb:.0} MB RSS")
            })
            .into_iter()
            .collect()
        }
        Err(e) => {
            tracing::debug!(error = %e, "failed to read process RSS — skipping memory check");
            Vec::new()
        }
    }
}

/// Non-Linux stub: `/proc/self/status` is not available on this platform.
#[cfg(not(target_os = "linux"))]
fn check_memory() -> Vec<AttentionItem> {
    tracing::trace!("process RSS check not supported on this platform");
    Vec::new()
}

/// Read the `VmRSS` value from `/proc/self/status`.
#[cfg(target_os = "linux")]
fn read_process_rss_kb() -> std::io::Result<u64> {
    let contents = std::fs::read_to_string("/proc/self/status")?;
    parse_vmrss(&contents)
}

/// Parse `VmRSS` value in kB from proc status content.
pub(crate) fn parse_vmrss(contents: &str) -> std::io::Result<u64> {
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let value_str = rest.trim().trim_end_matches(" kB").trim();
            return value_str
                .parse::<u64>()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e));
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "VmRSS not found in proc status",
    ))
}

// ── Memory consistency checking (A-MemGuard consensus-based anomaly detection) ──

/// Number of recent facts to sample per consistency check.
///
/// Kept small so the check is lightweight. Each sampled fact triggers
/// one additional Datalog query for entity-path verification.
const DEFAULT_ANOMALY_SAMPLE_SIZE: usize = 15;

/// Trait for pluggable knowledge graph consistency checks.
///
/// Implementations compare retrieval paths through the knowledge graph to
/// detect stale, corrupted, or orphaned facts. Inspired by A-MemGuard
/// (arxiv 2510.02373) consensus-based anomaly detection.
#[cfg(feature = "knowledge-store")]
pub(crate) trait ConsistencyCheck: Send + Sync {
    /// Run the consistency check against the given knowledge store.
    ///
    /// Returns attention items for each anomaly found.
    fn check(
        &self,
        store: &episteme::knowledge_store::KnowledgeStore,
    ) -> Result<Vec<AttentionItem>, episteme::error::Error>;
}

/// Multi-path consistency check: compares direct Datalog fact queries against
/// entity traversal paths (fact -> `fact_entities` -> entity -> `fact_entities` -> facts).
///
/// Two classes of anomaly are detected:
///
/// 1. **Orphaned facts**: facts that exist in the `facts` relation but have no
///    corresponding entry in `fact_entities` -- unreachable via entity traversal.
///
/// 2. **Dangling references**: `fact_entities` entries that reference fact IDs
///    not present in the `facts` relation -- stale after deletion or corruption.
#[cfg(feature = "knowledge-store")]
struct MultiPathConsistencyCheck {
    /// Number of recent facts to sample.
    sample_size: usize,
}

#[cfg(feature = "knowledge-store")]
impl MultiPathConsistencyCheck {
    fn new(sample_size: usize) -> Self {
        Self { sample_size }
    }
}

#[cfg(feature = "knowledge-store")]
impl ConsistencyCheck for MultiPathConsistencyCheck {
    fn check(
        &self,
        store: &episteme::knowledge_store::KnowledgeStore,
    ) -> Result<Vec<AttentionItem>, episteme::error::Error> {
        let mut items = Vec::new();

        // Path 1: Direct Datalog query -- sample recent facts.
        let limit = i64::try_from(self.sample_size).unwrap_or(i64::MAX);
        let facts = store.list_all_facts(limit)?;

        // Path 2: Entity traversal -- for each fact, check if it has a fact_entities link.
        // Facts without entity links are "orphaned" from the entity graph.
        if !facts.is_empty() {
            let fact_ids: Vec<&str> = facts.iter().map(|f| f.id.as_str()).collect();
            let linked_ids = query_entity_linked_fact_ids(store, &fact_ids)?;

            for fact in &facts {
                if !linked_ids.contains(fact.id.as_str()) {
                    items.push(AttentionItem {
                        category: AttentionCategory::MemoryAnomaly,
                        summary: format!(
                            "Orphaned fact (no entity link): {} -- \"{}\"",
                            fact.id,
                            truncate_content(&fact.content, 80),
                        ),
                        urgency: Urgency::Low,
                    });
                }
            }
        }

        // Reverse check: sample fact_entities entries and verify the referenced facts exist.
        let dangling = query_dangling_fact_entity_refs(store, self.sample_size)?;
        for dangling_id in &dangling {
            items.push(AttentionItem {
                category: AttentionCategory::MemoryAnomaly,
                summary: format!(
                    "Dangling fact_entities reference: fact {dangling_id} not found in facts relation"
                ),
                urgency: Urgency::Medium,
            });
        }

        tracing::debug!(
            orphaned = items
                .iter()
                .filter(|i| matches!(i.urgency, Urgency::Low))
                .count(),
            dangling = dangling.len(),
            sampled = facts.len(),
            "memory consistency check completed"
        );

        Ok(items)
    }
}

/// Query which of the given fact IDs have at least one `fact_entities` link.
///
/// Returns the subset of `fact_ids` that are present in the `fact_entities` relation.
#[cfg(feature = "knowledge-store")]
fn query_entity_linked_fact_ids(
    store: &episteme::knowledge_store::KnowledgeStore,
    fact_ids: &[&str],
) -> Result<std::collections::HashSet<String>, episteme::error::Error> {
    use std::collections::BTreeMap;

    if fact_ids.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    // WHY: Build an inline list of IDs for the Datalog `in [...]` operator.
    // Each ID is single-quoted and interior quotes are escaped.
    let id_list: Vec<String> = fact_ids
        .iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect();

    let script = format!(
        "?[fact_id] := *fact_entities{{fact_id, entity_id}}, fact_id in [{}]",
        id_list.join(", ")
    );

    let result = store.run_query(&script, BTreeMap::new())?;

    let mut linked = std::collections::HashSet::new();
    for i in 0..result.row_count() {
        if let Some(s) = result.get_string(i, "fact_id") {
            linked.insert(s);
        }
    }

    Ok(linked)
}

/// Query `fact_entities` for entries whose `fact_id` does not exist in `facts`.
///
/// Samples up to `limit` entries from `fact_entities` and checks each against
/// the `facts` relation. Returns the fact IDs that are dangling references.
#[cfg(feature = "knowledge-store")]
fn query_dangling_fact_entity_refs(
    store: &episteme::knowledge_store::KnowledgeStore,
    limit: usize,
) -> Result<Vec<String>, episteme::error::Error> {
    use std::collections::BTreeMap;

    // WHY: Two-pass in Rust because Datalog negation (`not`) requires all key
    // columns to be bound, but `facts` has composite key `(id, valid_from)`.
    // Query 1: collect distinct fact IDs referenced by `fact_entities`.
    // Query 2: collect distinct fact IDs that exist in `facts`.
    // Subtract in Rust to find dangling references.

    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

    // Step 1: Get a sample of fact IDs from fact_entities.
    let mut params1 = BTreeMap::new();
    params1.insert(
        "limit".to_owned(),
        episteme::engine::DataValue::from(limit_i64),
    );
    let fe_result = store.run_query(
        "?[fact_id] := *fact_entities{fact_id, entity_id} :limit $limit",
        params1,
    )?;

    let fe_ids: std::collections::HashSet<String> = (0..fe_result.row_count())
        .filter_map(|i| fe_result.get_string(i, "fact_id"))
        .collect();

    if fe_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Step 2: Check which of those fact IDs actually exist in facts.
    let id_list: Vec<String> = fe_ids
        .iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect();
    let script = format!(
        "?[id] := *facts{{id, valid_from}}, id in [{}]",
        id_list.join(", ")
    );
    let existing_result = store.run_query(&script, BTreeMap::new())?;

    let existing_ids: std::collections::HashSet<String> = (0..existing_result.row_count())
        .filter_map(|i| existing_result.get_string(i, "id"))
        .collect();

    // Step 3: Difference = dangling references.
    let dangling: Vec<String> = fe_ids
        .into_iter()
        .filter(|id| !existing_ids.contains(id))
        .collect();

    Ok(dangling)
}

/// Truncate a string to at most `max_len` bytes, appending "..." if truncated.
///
/// Splits at the last complete char boundary at or before `max_len`.
#[cfg(feature = "knowledge-store")]
fn truncate_content(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_owned()
    } else {
        // WHY: char_indices yields byte offsets at char boundaries, so the
        // resulting `end` is always a valid split point for str slicing.
        let end = s
            .char_indices()
            .take_while(|(i, _)| *i < max_len)
            .last()
            .map_or(0, |(i, c)| i + c.len_utf8());
        #[expect(
            clippy::string_slice,
            reason = "end is computed from char_indices: guaranteed to be a char boundary"
        )]
        // kanon:ignore RUST/indexing-slicing — end computed from char_indices at position max_len; guaranteed char boundary
        let prefix = &s[..end];
        format!("{prefix}...")
    }
}

#[cfg(test)]
#[path = "prosoche_tests.rs"]
mod prosoche_tests;
