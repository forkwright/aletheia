//! Prosoche (προσοχή): "directed attention." Periodic check-in that monitors
//! calendar, tasks, and system health for a nous.

use std::path::{Path, PathBuf};
#[cfg(feature = "knowledge-store")]
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Prosoche attention check runner.
pub struct ProsocheCheck {
    nous_id: String,
    /// Instance data directory to check for disk usage.
    data_dir: Option<PathBuf>,
    /// Database file paths to check sizes.
    db_paths: Vec<PathBuf>,
    /// Optional knowledge store for memory consistency checks.
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<aletheia_episteme::knowledge_store::KnowledgeStore>>,
}

impl std::fmt::Debug for ProsocheCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("ProsocheCheck");
        s.field("nous_id", &self.nous_id)
            .field("data_dir", &self.data_dir)
            .field("db_paths", &self.db_paths);
        #[cfg(feature = "knowledge-store")]
        s.field("knowledge_store", &self.knowledge_store.as_ref().map(|_| "<KnowledgeStore>"));
        s.finish()
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
        }
    }
}

/// Result of a prosoche check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProsocheResult {
    /// Items requiring the nous's attention.
    pub items: Vec<AttentionItem>,
    /// ISO 8601 timestamp when the check was performed.
    pub checked_at: String,
}

/// A single item requiring attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionItem {
    /// What kind of attention is needed.
    pub category: AttentionCategory,
    /// Human-readable description of the item.
    pub summary: String,
    /// How urgently this needs attention.
    pub urgency: Urgency,
}

/// Categories of attention items.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttentionCategory {
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
pub enum Urgency {
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "prosoche attention checks, tested and awaiting integration"
        )
    )]
    pub(crate) fn new(nous_id: &str) -> Self {
        Self {
            nous_id: nous_id.to_owned(),
            data_dir: None,
            db_paths: Vec::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
        }
    }

    /// Set the data directory for disk space checking.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "prosoche attention checks, tested and awaiting integration"
        )
    )]
    pub(crate) fn with_data_dir(mut self, path: &Path) -> Self {
        self.data_dir = Some(path.to_path_buf());
        self
    }

    /// Set database file paths for size checking.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "prosoche attention checks, tested and awaiting integration"
        )
    )]
    pub(crate) fn with_db_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.db_paths = paths;
        self
    }

    /// Set the knowledge store for memory consistency checking.
    ///
    /// When set, `run()` will perform multi-path consistency checks on a sample
    /// of recent facts, comparing direct Datalog query results against entity
    /// traversal paths (A-MemGuard consensus approach).
    #[must_use]
    #[cfg(feature = "knowledge-store")]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "prosoche attention checks, tested and awaiting integration"
        )
    )]
    pub(crate) fn with_knowledge_store(
        mut self,
        store: Arc<aletheia_episteme::knowledge_store::KnowledgeStore>,
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
    pub async fn run(&self) -> crate::error::Result<ProsocheResult> {
        let mut items = Vec::new();

        if let Some(ref data_dir) = self.data_dir {
            items.extend(check_disk_space(data_dir).await);
        }

        items.extend(check_db_sizes(&self.db_paths));

        items.extend(check_memory());

        #[cfg(feature = "knowledge-store")]
        if let Some(ref store) = self.knowledge_store {
            let checker = MultiPathConsistencyCheck::new(ANOMALY_SAMPLE_SIZE);
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

/// Check disk space usage on the filesystem containing `path`.
///
/// Uses `df` command output. WARN at 80% usage, CRITICAL at 95%.
async fn check_disk_space(path: &Path) -> Vec<AttentionItem> {
    match disk_usage_percent(path).await {
        Ok(percent) => {
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
async fn disk_usage_percent(path: &Path) -> std::io::Result<f64> {
    // NOTE: tokio::process::Child kills the child on Drop, providing the same
    // orphan-prevention guarantee as ProcessGuard. If this future is cancelled,
    // tokio kills the child automatically.
    let output = tokio::process::Command::new("df")
        .args(["--output=pcent", "--"])
        .arg(path)
        .output()
        .await?;

    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_df_percent(&stdout)
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
                #[expect(
                    clippy::cast_precision_loss,
                    clippy::as_conversions,
                    reason = "u64→f64: file sizes on supported storage are well below f64 mantissa 2^53 ≈ 9 exabytes; display precision need is a single decimal place"
                )]
                let size_gb = meta.len() as f64 / ONE_GB as f64;
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
fn check_memory() -> Vec<AttentionItem> {
    match read_process_rss_kb() {
        Ok(resident_kb) => {
            let rss_mb = resident_kb / 1024;
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "u64→f64: process RSS in MB is bounded by host RAM (under 2^53 MB); cast is exact at practical scale"
            )]
            let rss_f64 = rss_mb as f64;
            let label = if rss_mb >= 2048 {
                "critical"
            } else {
                "high"
            };
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

/// Read the `VmRSS` value from `/proc/self/status`.
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

// ---------------------------------------------------------------------------
// Memory consistency checking (A-MemGuard consensus-based anomaly detection)
// ---------------------------------------------------------------------------

/// Number of recent facts to sample per consistency check.
///
/// Kept small to ensure the check is lightweight. Each sampled fact triggers
/// one additional Datalog query for entity-path verification.
#[cfg(feature = "knowledge-store")]
const ANOMALY_SAMPLE_SIZE: usize = 15;

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
        store: &aletheia_episteme::knowledge_store::KnowledgeStore,
    ) -> Result<Vec<AttentionItem>, aletheia_episteme::error::Error>;
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
        store: &aletheia_episteme::knowledge_store::KnowledgeStore,
    ) -> Result<Vec<AttentionItem>, aletheia_episteme::error::Error> {
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
            orphaned = items.iter().filter(|i| matches!(i.urgency, Urgency::Low)).count(),
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
    store: &aletheia_episteme::knowledge_store::KnowledgeStore,
    fact_ids: &[&str],
) -> Result<std::collections::HashSet<String>, aletheia_episteme::error::Error> {
    use std::collections::BTreeMap;

    if fact_ids.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    // WHY: Build an inline list of IDs for CozoDB's `in [...]` operator.
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
    for row in &result.rows {
        if let Some(val) = row.first()
            && let Some(s) = val.get_str()
        {
            linked.insert(s.to_owned());
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
    store: &aletheia_episteme::knowledge_store::KnowledgeStore,
    limit: usize,
) -> Result<Vec<String>, aletheia_episteme::error::Error> {
    use std::collections::BTreeMap;

    // WHY: Two-pass in Rust because CozoDB negation (`not`) requires all key
    // columns to be bound, but `facts` has composite key `(id, valid_from)`.
    // Query 1: collect distinct fact IDs referenced by `fact_entities`.
    // Query 2: collect distinct fact IDs that exist in `facts`.
    // Subtract in Rust to find dangling references.

    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

    // Step 1: Get a sample of fact IDs from fact_entities.
    let mut params1 = BTreeMap::new();
    params1.insert("limit".to_owned(), aletheia_episteme::engine::DataValue::from(limit_i64));
    let fe_result = store.run_query(
        "?[fact_id] := *fact_entities{fact_id, entity_id} :limit $limit",
        params1,
    )?;

    let fe_ids: std::collections::HashSet<String> = fe_result
        .rows
        .iter()
        .filter_map(|row| row.first().and_then(|v| v.get_str()).map(str::to_owned))
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

    let existing_ids: std::collections::HashSet<String> = existing_result
        .rows
        .iter()
        .filter_map(|row| row.first().and_then(|v| v.get_str()).map(str::to_owned))
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
        let prefix = &s[..end];
        format!("{prefix}...")
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prosoche_returns_items_for_default() {
        let check = ProsocheCheck::new("test-nous");
        let result = check.run().await.expect("should succeed");
        assert!(!result.checked_at.is_empty());
    }

    #[test]
    fn prosoche_check_new() {
        let check = ProsocheCheck::new("alice-nous");
        let debug = format!("{check:?}");
        assert!(
            debug.contains("alice-nous"),
            "ProsocheCheck should store the nous_id"
        );
    }

    #[test]
    fn attention_item_category_label_calendar() {
        let item = AttentionItem {
            category: AttentionCategory::Calendar,
            summary: "meeting".to_owned(),
            urgency: Urgency::Medium,
        };
        assert_eq!(item.category_label(), "calendar");
    }

    #[test]
    fn attention_item_category_label_task() {
        let item = AttentionItem {
            category: AttentionCategory::Task,
            summary: "review PR".to_owned(),
            urgency: Urgency::Low,
        };
        assert_eq!(item.category_label(), "task");
    }

    #[test]
    fn attention_item_category_label_health() {
        let item = AttentionItem {
            category: AttentionCategory::SystemHealth,
            summary: "disk full".to_owned(),
            urgency: Urgency::Critical,
        };
        assert_eq!(item.category_label(), "health");
    }

    #[test]
    fn attention_item_category_label_custom() {
        let item = AttentionItem {
            category: AttentionCategory::Custom("foo".to_owned()),
            summary: "custom item".to_owned(),
            urgency: Urgency::Low,
        };
        assert_eq!(item.category_label(), "foo");
    }

    #[test]
    fn urgency_ordering() {
        assert!(Urgency::Low < Urgency::Medium);
        assert!(Urgency::Medium < Urgency::High);
        assert!(Urgency::High < Urgency::Critical);
    }

    #[test]
    fn prosoche_result_serialization() {
        let result = ProsocheResult {
            items: vec![AttentionItem {
                category: AttentionCategory::Task,
                summary: "test".to_owned(),
                urgency: Urgency::High,
            }],
            checked_at: "2026-01-01T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let back: ProsocheResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.checked_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn attention_item_serialization() {
        let item = AttentionItem {
            category: AttentionCategory::Calendar,
            summary: "standup".to_owned(),
            urgency: Urgency::Medium,
        };
        let json = serde_json::to_string(&item).expect("serialize");
        assert!(json.contains("Calendar"));
        assert!(json.contains("standup"));
        assert!(json.contains("Medium"));
    }

    #[test]
    fn parse_vmrss_extracts_value() {
        let content = "\
Name:   aletheia
VmPeak:   500000 kB
VmRSS:   123456 kB
Threads:  8
";
        let rss = parse_vmrss(content).expect("should parse");
        assert_eq!(rss, 123_456);
    }

    #[test]
    fn parse_vmrss_missing_returns_error() {
        let content = "Name: aletheia\nThreads: 8\n";
        assert!(parse_vmrss(content).is_err());
    }

    #[test]
    fn check_memory_runs_without_panic() {
        let items = check_memory();
        assert!(
            items.len() <= 1,
            "test process should not exceed memory thresholds"
        );
    }

    #[test]
    fn check_db_sizes_empty_paths() {
        let items = check_db_sizes(&[]);
        assert!(items.is_empty());
    }

    #[test]
    fn check_db_sizes_nonexistent_file() {
        let items = check_db_sizes(&[PathBuf::from("/tmp/nonexistent-db-file-for-test.db")]);
        assert!(
            items.is_empty(),
            "nonexistent file should not produce items"
        );
    }

    #[test]
    fn check_db_sizes_small_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db_path = dir.path().join("test.db");
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        std::fs::write(&db_path, b"small content").expect("write test file");

        let items = check_db_sizes(&[db_path]);
        assert!(items.is_empty(), "small file should not trigger warning");
    }

    #[tokio::test]
    async fn prosoche_with_data_dir_runs_disk_check() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let check = ProsocheCheck::new("test-nous").with_data_dir(dir.path());
        let result = check.run().await.expect("should succeed");
        assert!(!result.checked_at.is_empty());
    }

    #[tokio::test]
    async fn prosoche_with_db_paths_runs_size_check() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db_path = dir.path().join("test.db");
        #[expect(
            clippy::disallowed_methods,
            reason = "maintenance tasks run outside the async runtime and require synchronous filesystem access"
        )]
        std::fs::write(&db_path, b"data").expect("write");
        let check = ProsocheCheck::new("test-nous").with_db_paths(vec![db_path]);
        let result = check.run().await.expect("should succeed");
        assert!(!result.checked_at.is_empty());
    }

    #[test]
    fn parse_df_percent_valid() {
        let output = "Use%\n 42%\n";
        let percent = parse_df_percent(output).expect("should parse");
        assert!((percent - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_df_percent_no_data() {
        let output = "Use%\n";
        assert!(parse_df_percent(output).is_err());
    }

    #[test]
    fn attention_item_category_label_memory_anomaly() {
        let item = AttentionItem {
            category: AttentionCategory::MemoryAnomaly,
            summary: "orphaned fact".to_owned(),
            urgency: Urgency::Low,
        };
        assert_eq!(item.category_label(), "memory-anomaly");
    }

    #[test]
    fn memory_anomaly_serialization_roundtrip() {
        let item = AttentionItem {
            category: AttentionCategory::MemoryAnomaly,
            summary: "dangling reference".to_owned(),
            urgency: Urgency::Medium,
        };
        let json = serde_json::to_string(&item).expect("serialize");
        assert!(json.contains("MemoryAnomaly"));
        let back: AttentionItem = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.category_label(), "memory-anomaly");
    }

    // --- Knowledge-store-gated tests ---

    #[cfg(feature = "knowledge-store")]
    #[expect(clippy::indexing_slicing, reason = "test assertions on known-length vectors")]
    mod knowledge_store_tests {
        use super::*;

        /// Helper: create a test fact with the given id string.
        fn make_fact(id: &str, content: &str) -> aletheia_episteme::knowledge::Fact {
            aletheia_episteme::knowledge::Fact {
                id: aletheia_episteme::id::FactId::new(id).expect("valid id"),
                nous_id: "test-nous".to_owned(),
                fact_type: "observation".to_owned(),
                content: content.to_owned(),
                scope: None,
                temporal: aletheia_episteme::knowledge::FactTemporal {
                    valid_from: jiff::Timestamp::now(),
                    valid_to: jiff::Timestamp::from_second(253_402_207_200)
                        .expect("far future"),
                    recorded_at: jiff::Timestamp::now(),
                },
                provenance: aletheia_episteme::knowledge::FactProvenance {
                    confidence: 0.9,
                    tier: aletheia_episteme::knowledge::EpistemicTier::Verified,
                    source_session_id: None,
                    stability_hours: 720.0,
                },
                lifecycle: aletheia_episteme::knowledge::FactLifecycle {
                    superseded_by: None,
                    is_forgotten: false,
                    forgotten_at: None,
                    forget_reason: None,
                },
                access: aletheia_episteme::knowledge::FactAccess {
                    access_count: 0,
                    last_accessed_at: None,
                },
            }
        }

        /// Helper: insert a `fact_entities` row via raw Datalog.
        fn insert_fact_entity_raw(
            store: &aletheia_episteme::knowledge_store::KnowledgeStore,
            fact_id: &str,
            entity_id: &str,
        ) {
            use std::collections::BTreeMap;
            use aletheia_episteme::engine::DataValue;

            let now = aletheia_episteme::knowledge::format_timestamp(&jiff::Timestamp::now());
            let mut params = BTreeMap::new();
            params.insert("fact_id".to_owned(), DataValue::Str(fact_id.into()));
            params.insert("entity_id".to_owned(), DataValue::Str(entity_id.into()));
            params.insert("created_at".to_owned(), DataValue::Str(now.into()));
            store
                .run_mut_query(
                    r"?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]
                      :put fact_entities { fact_id, entity_id => created_at }",
                    params,
                )
                .expect("insert fact_entity");
        }

        #[test]
        fn truncate_content_short() {
            assert_eq!(truncate_content("hello", 10), "hello");
        }

        #[test]
        fn truncate_content_exact_boundary() {
            assert_eq!(truncate_content("hello", 5), "hello");
        }

        #[test]
        fn truncate_content_long() {
            let long = "a".repeat(100);
            let result = truncate_content(&long, 10);
            assert_eq!(result.len(), 13); // 10 chars + "..."
            assert!(result.ends_with("..."));
        }

        #[test]
        fn truncate_content_multibyte() {
            // 3-byte UTF-8 chars: should not split in the middle.
            let s = "\u{2603}\u{2603}\u{2603}\u{2603}"; // 4 snowmen, 12 bytes
            let result = truncate_content(s, 7);
            assert!(result.ends_with("..."));
            // Should include at most 2 snowmen (6 bytes < 7) + "..."
            assert!(result.starts_with("\u{2603}\u{2603}"));
        }

        #[test]
        fn consistency_check_empty_store() {
            let store =
                aletheia_episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");
            let checker = MultiPathConsistencyCheck::new(15);
            let items = checker.check(&store).expect("check");
            assert!(items.is_empty(), "empty store should produce no anomalies");
        }

        #[test]
        fn consistency_check_orphaned_facts() {
            // Insert facts without entity links — they should be flagged as orphaned.
            let store =
                aletheia_episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");

            let fact = make_fact("fact-orphan-001", "Rust is a systems programming language");
            store.insert_fact(&fact).expect("insert fact");

            let checker = MultiPathConsistencyCheck::new(15);
            let items = checker.check(&store).expect("check");

            assert_eq!(items.len(), 1, "should detect one orphaned fact");
            assert!(
                matches!(items[0].category, AttentionCategory::MemoryAnomaly),
                "category should be MemoryAnomaly"
            );
            assert!(
                items[0].summary.contains("Orphaned fact"),
                "summary should mention orphaned: {}",
                items[0].summary
            );
            assert_eq!(items[0].urgency, Urgency::Low);
        }

        #[test]
        fn consistency_check_linked_facts_no_anomaly() {
            // Insert a fact WITH entity links — no anomaly should be flagged.
            let store =
                aletheia_episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");

            let fact = make_fact("fact-linked-001", "Cody prefers Rust over Go");
            store.insert_fact(&fact).expect("insert fact");

            let entity = aletheia_episteme::knowledge::Entity {
                id: aletheia_episteme::id::EntityId::new("entity-cody-001").expect("valid"),
                name: "Cody".to_owned(),
                entity_type: "person".to_owned(),
                aliases: Vec::new(),
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            };
            store.insert_entity(&entity).expect("insert entity");
            insert_fact_entity_raw(&store, fact.id.as_str(), entity.id.as_str());

            let checker = MultiPathConsistencyCheck::new(15);
            let items = checker.check(&store).expect("check");

            assert!(
                items.is_empty(),
                "linked facts should produce no anomalies, got: {items:?}"
            );
        }

        #[test]
        fn consistency_check_dangling_reference() {
            // Insert a fact_entities entry pointing to a non-existent fact.
            let store =
                aletheia_episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");

            let entity = aletheia_episteme::knowledge::Entity {
                id: aletheia_episteme::id::EntityId::new("entity-ghost-001").expect("valid"),
                name: "Ghost".to_owned(),
                entity_type: "concept".to_owned(),
                aliases: Vec::new(),
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            };
            store.insert_entity(&entity).expect("insert entity");

            // Insert a fact_entities row with a fact_id that doesn't exist in facts.
            insert_fact_entity_raw(&store, "phantom-fact-001", entity.id.as_str());

            let checker = MultiPathConsistencyCheck::new(15);
            let items = checker.check(&store).expect("check");

            assert_eq!(items.len(), 1, "should detect one dangling reference");
            assert!(
                matches!(items[0].category, AttentionCategory::MemoryAnomaly),
                "category should be MemoryAnomaly"
            );
            assert!(
                items[0].summary.contains("Dangling"),
                "summary should mention dangling: {}",
                items[0].summary
            );
            assert_eq!(items[0].urgency, Urgency::Medium);
        }

        #[tokio::test]
        async fn prosoche_with_knowledge_store_runs_consistency_check() {
            let store =
                aletheia_episteme::knowledge_store::KnowledgeStore::open_mem().expect("open_mem");

            // Insert an orphaned fact (no entity link).
            let fact = make_fact("fact-prosoche-001", "test content");
            store.insert_fact(&fact).expect("insert fact");

            let check = ProsocheCheck::new("test-nous").with_knowledge_store(store);
            let result = check.run().await.expect("should succeed");

            let anomalies: Vec<_> = result
                .items
                .iter()
                .filter(|i| matches!(i.category, AttentionCategory::MemoryAnomaly))
                .collect();
            assert_eq!(anomalies.len(), 1, "should detect the orphaned fact");
        }
    }
}
