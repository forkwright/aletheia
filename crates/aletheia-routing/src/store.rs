//! Shared empirical success-rate storage for dispatch and interactive paths.
//!
//! [`AfterActionStore`] maintains a rolling in-memory cache of
//! `(provider_id, task_category) → success_rate` statistics. It has two
//! write paths:
//!
//! - **Dispatch path** ([`AfterActionStore::refresh`]): re-scans the
//!   append-only JSONL log files emitted by the energeia post-processing
//!   stage. Called periodically by the daemon maintenance task.
//!
//! - **Interactive path** ([`AfterActionStore::record_outcome`]): directly
//!   increments the in-memory counters without requiring a JSONL file write.
//!   This is the mechanism by which nous turns feed empirical data into the
//!   same store that dispatch routing queries.
//!
//! Both paths write into the same `RwLock<HashMap>`, so a dispatch routing
//! decision in the next window will incorporate interactive-path outcomes
//! that arrived since the last `refresh`.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use snafu::{ResultExt as _, Snafu};
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::sync::RwLock;

use crate::types::{ProviderId, TaskCategory, TurnOutcome};

const SECS_PER_DAY: u64 = 24 * 60 * 60;

/// Default rolling window for routing success-rate statistics.
pub const DEFAULT_ROUTING_WINDOW: Duration = Duration::from_secs(7 * SECS_PER_DAY);

/// Maximum number of recent interactive outcomes kept for audit.
///
/// WHY: bounded so the in-memory store cannot grow without limit. The log is
/// meant for short-term operational audit, not long-term archival.
const MAX_AUDIT_LOG_SIZE: usize = 1000;

/// Errors produced by [`AfterActionStore`] operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum AfterActionStoreError {
    /// Could not read a JSONL log directory.
    #[snafu(display("I/O error reading after-action log '{}': {source}", path.display()))]
    Io {
        /// Path that triggered the error.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

// ── Wire-format structs (subset of energeia's AfterActionRecord) ──
/// Parsed subset of an after-action JSONL line relevant to routing stats.
#[derive(Debug, serde::Deserialize)]
struct AfterActionLine {
    session_outcomes: Vec<AfterActionSession>,
}

/// Per-session fields used to compute success rates.
#[derive(Debug, serde::Deserialize)]
struct AfterActionSession {
    /// Provider / model that handled the session.
    ///
    /// May be absent for records written before this field was introduced.
    #[serde(default)]
    model: Option<String>,
    /// Terminal status string (e.g. `"success"`, `"failed"`, `"stuck"`).
    status: String,
    /// Category tag. Optional in older records; missing is treated as
    /// `Feature`.
    #[serde(default)]
    category: Option<String>,
}

/// Aggregated success/failure counts for a (provider, category) bucket.
#[derive(Debug, Clone, Default)]
pub struct RollingStats {
    /// Sessions that completed successfully.
    pub successes: u64,
    /// Sessions that ended in any failure state.
    pub failures: u64,
    /// Total sessions (`successes + failures`).
    pub total: u64,
    /// Timestamp of the most recent successful session, if any.
    pub last_success_at: Option<Timestamp>,
}

impl RollingStats {
    /// Empirical success rate in [0, 1], or `None` when `total == 0`.
    pub fn success_rate(&self) -> Option<f64> {
        if self.total == 0 {
            None
        } else {
            // WHY: precision loss at very high session counts (>2^53) is
            // acceptable for routing heuristics.
            #[expect(
                clippy::cast_precision_loss,
                reason = "routing rates are heuristic; precision loss at >2^53 sessions is acceptable"
            )]
            #[expect(
                clippy::as_conversions,
                reason = "u64→f64 for rate computation; loss is acceptable and documented above"
            )]
            Some(self.successes as f64 / self.total as f64)
        }
    }
}

/// Shared read/write cache over empirical provider success-rate statistics.
///
/// Used by both the dispatch path (via [`refresh`](Self::refresh) from JSONL
/// logs) and the interactive path (via [`record_outcome`](Self::record_outcome)
/// for direct counter updates).
///
/// The cache is keyed by `(ProviderId, TaskCategory)` and protected by a
/// `RwLock` so concurrent readers (routing decisions) are never blocked by
/// a periodic `refresh`.
#[derive(Debug)]
pub struct AfterActionStore {
    /// Directory containing per-day JSONL files (`YYYY-MM-DD.jsonl`).
    ///
    /// `None` when the store is used in memory-only mode (interactive path
    /// without a configured log directory).
    dir: Option<PathBuf>,
    /// Latest per-day JSONL files to include during refresh.
    window: Duration,
    /// In-memory cache: `(provider_id, task_category)` → [`RollingStats`].
    cache: RwLock<HashMap<(ProviderId, TaskCategory), RollingStats>>,
    /// Direct interactive writes since the most recent disk refresh.
    interactive: RwLock<HashMap<(ProviderId, TaskCategory), RollingStats>>,
    /// Recent interactive outcomes kept for operational audit.
    ///
    /// WHY: lets operators inspect *why* a turn was counted as a success or
    /// failure without replaying the full JSONL archive.
    audit_log: RwLock<VecDeque<TurnOutcome>>,
}

impl AfterActionStore {
    /// Create a store backed by the given JSONL log directory.
    ///
    /// Call [`refresh`](Self::refresh) to populate the cache from disk.
    pub fn new(dir: PathBuf) -> Self {
        Self::new_with_window(dir, DEFAULT_ROUTING_WINDOW)
    }

    /// Create a store backed by `dir` with a bounded refresh window.
    ///
    /// The window is interpreted as a count of latest per-day JSONL files. A
    /// zero duration keeps all history for callers that explicitly need it.
    pub fn new_with_window(dir: PathBuf, window: Duration) -> Self {
        Self {
            dir: Some(dir),
            window,
            cache: RwLock::new(HashMap::new()),
            interactive: RwLock::new(HashMap::new()),
            audit_log: RwLock::new(VecDeque::new()),
        }
    }

    /// Create an in-memory-only store with no JSONL backing directory.
    ///
    /// Suitable for the interactive path when no log directory is configured.
    /// [`refresh`](Self::refresh) is a no-op on this variant.
    pub fn in_memory() -> Self {
        Self {
            dir: None,
            window: DEFAULT_ROUTING_WINDOW,
            cache: RwLock::new(HashMap::new()),
            interactive: RwLock::new(HashMap::new()),
            audit_log: RwLock::new(VecDeque::new()),
        }
    }

    /// Return rolling stats for a specific (provider, category) pair.
    ///
    /// Returns `Ok(None)` when the pair has no entries in a healthy cache.
    /// Returns `Err` when a requested window requires rebuilding from an
    /// unreadable backing directory.
    pub async fn rolling_stats(
        &self,
        provider: &ProviderId,
        cat: &TaskCategory,
        window: std::time::Duration,
    ) -> Result<Option<RollingStats>, AfterActionStoreError> {
        if window != self.window
            && let Some(dir) = &self.dir
        {
            let mut cache = self.build_cache(dir, window).await?;
            self.merge_interactive(&mut cache).await;
            return Ok(cache.get(&(provider.clone(), *cat)).cloned());
        }

        let cache = self.cache.read().await;
        Ok(cache.get(&(provider.clone(), *cat)).cloned())
    }

    /// Directly record the outcome of an interactive turn.
    ///
    /// Increments the in-memory counter for `(outcome.provider,
    /// outcome.task_category)` without touching the JSONL files. This is the
    /// write path for the nous interactive router.
    ///
    /// WHY: the JSONL files are owned by the energeia post-processing stage.
    /// Rather than having nous write to those files (wrong ownership), nous
    /// increments the shared in-memory counters directly. The next
    /// `refresh()` call from the dispatch path will overwrite the in-memory
    /// state from disk, so interactive-path outcomes live for at most one
    /// refresh window. That is an acceptable trade-off: the store still
    /// captures short-term signal and dispatch routing decisions in the same
    /// window benefit.
    ///
    /// When `outcome.interactive_outcome` is present, the stored success value
    /// is derived from the real outcome dimensions rather than the collapsed
    /// `success` boolean. The outcome is also appended to the bounded audit log
    /// so operators can inspect why a turn was counted as success or failure.
    pub async fn record_outcome(&self, outcome: &TurnOutcome) -> Result<(), AfterActionStoreError> {
        let success = outcome.interactive_outcome.as_ref().map_or(
            outcome.success,
            super::types::InteractiveOutcome::is_success,
        );
        let key = (outcome.provider.clone(), outcome.task_category);
        let mut cache = self.cache.write().await;
        record_stats(cache.entry(key.clone()).or_default(), success);
        drop(cache);

        let mut interactive = self.interactive.write().await;
        record_stats(interactive.entry(key).or_default(), success);
        drop(interactive);

        let mut audit_log = self.audit_log.write().await;
        audit_log.push_back(outcome.clone());
        if audit_log.len() > MAX_AUDIT_LOG_SIZE {
            audit_log.pop_front();
        }
        Ok(())
    }

    /// Return a clone of the most recently recorded interactive outcomes.
    ///
    /// WHY: exposes the audit trail used to derive success-rate statistics so
    /// operators and tests can verify that poor interactive turns were not
    /// counted as successes merely because the provider returned a response.
    #[must_use]
    pub async fn recent_outcomes(&self) -> Vec<TurnOutcome> {
        self.audit_log.read().await.iter().cloned().collect()
    }

    /// Re-scan the JSONL log directory and rebuild the in-memory cache.
    ///
    /// Streams each file line-by-line to avoid loading the whole day's log
    /// into memory. Malformed JSON lines are silently skipped.
    ///
    /// If the store was created with [`in_memory`](Self::in_memory) this is
    /// a no-op (returns `Ok(())`).
    pub async fn refresh(&self) -> Result<(), AfterActionStoreError> {
        self.refresh_window(self.window).await
    }

    /// Re-scan the JSONL log directory using an explicit latest-file window.
    pub async fn refresh_window(&self, window: Duration) -> Result<(), AfterActionStoreError> {
        let Some(ref dir) = self.dir else {
            return Ok(());
        };
        let new_cache = self.build_cache(dir, window).await?;
        let mut cache = self.cache.write().await;
        *cache = new_cache;
        let mut interactive = self.interactive.write().await;
        interactive.clear();
        Ok(())
    }

    /// Build a fresh cache by scanning all JSONL files in `dir`.
    async fn build_cache(
        &self,
        dir: &Path,
        window: Duration,
    ) -> Result<HashMap<(ProviderId, TaskCategory), RollingStats>, AfterActionStoreError> {
        let mut map: HashMap<(ProviderId, TaskCategory), RollingStats> = HashMap::new();

        for path in jsonl_paths_in_window(dir, window).await? {
            self.scan_file(&path, &mut map).await?;
        }

        Ok(map)
    }

    /// Stream a single JSONL file and accumulate stats into `map`.
    async fn scan_file(
        &self,
        path: &Path,
        map: &mut HashMap<(ProviderId, TaskCategory), RollingStats>,
    ) -> Result<(), AfterActionStoreError> {
        // kanon:ignore PERFORMANCE/no-blocking-io-in-async — uses tokio::fs async API, not blocking std::fs
        let file = match tokio::fs::File::open(path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(AfterActionStoreError::Io {
                    path: path.to_owned(),
                    source: e,
                });
            }
        };

        let mut lines = BufReader::new(file).lines();
        while let Some(line) = lines.next_line().await.context(IoSnafu {
            path: path.to_owned(),
        })? {
            let line = line.trim().to_owned();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<AfterActionLine>(&line) {
                Ok(record) => ingest_record(&record, map),
                Err(e) => {
                    // WHY: silently skip malformed lines — partial writes from
                    // process kills produce truncated records; poisoning the cache
                    // for a single bad record degrades all routing decisions.
                    tracing::debug!(
                        path = %path.display(),
                        error = %e,
                        "skipping malformed after-action line"
                    );
                }
            }
        }

        Ok(())
    }

    async fn merge_interactive(&self, map: &mut HashMap<(ProviderId, TaskCategory), RollingStats>) {
        let interactive = self.interactive.read().await;
        for (key, stats) in interactive.iter() {
            merge_stats(map.entry(key.clone()).or_default(), stats);
        }
    }
}

async fn jsonl_paths_in_window(
    dir: &Path,
    window: Duration,
) -> Result<Vec<PathBuf>, AfterActionStoreError> {
    let mut paths = Vec::new();
    let mut read_dir = tokio::fs::read_dir(dir).await.context(IoSnafu {
        path: dir.to_owned(),
    })?;

    while let Some(entry) = read_dir.next_entry().await.context(IoSnafu {
        path: dir.to_owned(),
    })? {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            paths.push(path);
        }
    }

    paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    paths.reverse();

    if let Some(limit) = window_file_limit(window) {
        paths.truncate(limit);
    }

    Ok(paths)
}

fn window_file_limit(window: Duration) -> Option<usize> {
    if window.is_zero() {
        return None;
    }

    let days = window.as_secs().div_ceil(SECS_PER_DAY).max(1);
    usize::try_from(days).ok()
}

/// Ingest one parsed after-action record into the rolling-stats accumulator.
fn ingest_record(
    record: &AfterActionLine,
    map: &mut HashMap<(ProviderId, TaskCategory), RollingStats>,
) {
    for session in &record.session_outcomes {
        let Some(model) = &session.model else {
            continue; // pre-routing records have no model field
        };
        if model.is_empty() {
            continue;
        }

        let provider = ProviderId::new(model.as_str());
        let category = session
            .category
            .as_deref()
            .map_or(TaskCategory::Feature, parse_category);

        let key = (provider, category);
        let stats = map.entry(key).or_default();

        let is_success = session.status == "success";
        record_stats(stats, is_success);
    }
}

fn record_stats(stats: &mut RollingStats, success: bool) {
    if success {
        stats.successes += 1;
        stats.last_success_at = Some(Timestamp::now());
    } else {
        stats.failures += 1;
    }
    stats.total += 1;
}

fn merge_stats(target: &mut RollingStats, source: &RollingStats) {
    target.successes += source.successes;
    target.failures += source.failures;
    target.total += source.total;
    target.last_success_at = match (target.last_success_at, source.last_success_at) {
        (Some(target_ts), Some(source_ts)) => Some(target_ts.max(source_ts)),
        (None, Some(source_ts)) => Some(source_ts),
        (Some(target_ts), None) => Some(target_ts),
        (None, None) => None,
    };
}

/// Parse a category string from a JSONL record.
///
/// Returns [`TaskCategory::Feature`] for unrecognised strings so that new
/// categories added in future PRs degrade gracefully on old store data.
fn parse_category(s: &str) -> TaskCategory {
    match s.parse::<TaskCategory>() {
        Ok(category) => category,
        Err(_) => TaskCategory::Feature,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::io::Write as _;
    use std::time::Duration;

    use super::*;

    fn write_jsonl(dir: &std::path::Path, filename: &str, lines: &[serde_json::Value]) {
        let path = dir.join(filename);
        let mut file = std::fs::File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    fn session_line(model: &str, status: &str, category: &str) -> serde_json::Value {
        serde_json::json!({
            "dispatch_id": "test-dispatch",
            "ts_start": "2026-04-17T00:00:00Z",
            "ts_end": "2026-04-17T00:01:00Z",
            "duration_ms": 60000,
            "session_outcomes": [{"model": model, "status": status, "category": category}],
            "cost_total_cents": 5,
            "turns_total": 10,
            "stage_latencies_ms": {},
            "qa_verdict": "pass",
            "prompt_hash": "sha256:abc"
        })
    }

    // --- Dispatch path (JSONL refresh) ---

    #[tokio::test]
    async fn rolling_stats_counts_match_written_records() {
        let tmp = tempfile::tempdir().unwrap();

        let mut lines = vec![];
        for _ in 0..9 {
            lines.push(session_line("provider-a", "success", "feature"));
        }
        lines.push(session_line("provider-a", "failed", "feature"));
        lines.push(session_line("provider-b", "success", "feature"));
        for _ in 0..9 {
            lines.push(session_line("provider-b", "failed", "feature"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();

        let a = store
            .rolling_stats(
                &ProviderId::new("provider-a"),
                &TaskCategory::Feature,
                Duration::from_hours(168),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a.successes, 9);
        assert_eq!(a.failures, 1);
        assert_eq!(a.total, 10);
        assert!((a.success_rate().unwrap() - 0.9).abs() < 0.001);
    }

    #[tokio::test]
    async fn refresh_handles_malformed_json_gracefully() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("2026-04-17.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "{}", session_line("good", "success", "feature")).unwrap();
        writeln!(file, "{{\"incomplete\":").unwrap();
        writeln!(file, "{}", session_line("good", "success", "feature")).unwrap();

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();

        let stats = store
            .rolling_stats(
                &ProviderId::new("good"),
                &TaskCategory::Feature,
                Duration::from_hours(168),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stats.total, 2);
    }

    #[tokio::test]
    async fn refresh_scans_only_latest_files_in_window() {
        let tmp = tempfile::tempdir().unwrap();

        for idx in 0..1000 {
            let model = if idx >= 990 { "recent" } else { "stale" };
            write_jsonl(
                tmp.path(),
                &format!("2026-04-{idx:04}.jsonl"),
                &[session_line(model, "success", "feature")],
            );
        }

        let store =
            AfterActionStore::new_with_window(tmp.path().to_owned(), Duration::from_hours(240));
        store.refresh().await.unwrap();

        let recent = store
            .rolling_stats(
                &ProviderId::new("recent"),
                &TaskCategory::Feature,
                Duration::from_hours(240),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recent.total, 10);
        assert!(
            store
                .rolling_stats(
                    &ProviderId::new("stale"),
                    &TaskCategory::Feature,
                    Duration::from_hours(240),
                )
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn rolling_stats_uses_requested_window_when_it_differs_from_cache() {
        let tmp = tempfile::tempdir().unwrap();

        for idx in 0..20 {
            let model = if idx >= 10 { "recent" } else { "stale" };
            write_jsonl(
                tmp.path(),
                &format!("2026-04-{idx:04}.jsonl"),
                &[session_line(model, "success", "feature")],
            );
        }

        let store =
            AfterActionStore::new_with_window(tmp.path().to_owned(), Duration::from_hours(480));
        store.refresh().await.unwrap();

        assert!(
            store
                .rolling_stats(
                    &ProviderId::new("stale"),
                    &TaskCategory::Feature,
                    Duration::from_hours(480),
                )
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .rolling_stats(
                    &ProviderId::new("stale"),
                    &TaskCategory::Feature,
                    Duration::from_hours(240),
                )
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn rolling_stats_differing_window_returns_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("not-a-directory");
        std::fs::write(&file_path, "not jsonl").unwrap();

        let store = AfterActionStore::new_with_window(file_path, Duration::from_hours(480));
        let result = store
            .rolling_stats(
                &ProviderId::new("provider-a"),
                &TaskCategory::Feature,
                Duration::from_hours(240),
            )
            .await;

        assert!(
            matches!(result, Err(AfterActionStoreError::Io { .. })),
            "expected I/O error, got {result:?}"
        );
    }

    #[tokio::test]
    async fn missing_dir_returns_error() {
        let store = AfterActionStore::new(std::path::PathBuf::from(
            "/tmp/nonexistent-xyz-routing-test",
        ));
        assert!(store.refresh().await.is_err());
    }

    // --- Interactive path (record_outcome) ---

    /// Empirical router learns from dispatch outcomes via JSONL refresh.
    #[tokio::test]
    async fn empirical_router_learns_from_dispatch_outcome() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = vec![];
        for _ in 0..8 {
            lines.push(session_line("provider-x", "success", "bug"));
        }
        for _ in 0..2 {
            lines.push(session_line("provider-x", "failed", "bug"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();

        let stats = store
            .rolling_stats(
                &ProviderId::new("provider-x"),
                &TaskCategory::Bug,
                Duration::from_hours(168),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stats.total, 10);
        assert_eq!(stats.successes, 8);
        assert!((stats.success_rate().unwrap() - 0.8).abs() < 0.001);
    }

    /// Dispatch JSONL records without a model field are skipped.
    #[tokio::test]
    async fn refresh_skips_records_without_model() {
        let tmp = tempfile::tempdir().unwrap();
        let mut no_model = session_line("provider-y", "success", "feature");
        no_model
            .get_mut("session_outcomes")
            .unwrap()
            .get_mut(0)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("model");
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &[no_model]);

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();

        assert!(
            store
                .rolling_stats(
                    &ProviderId::new("provider-y"),
                    &TaskCategory::Feature,
                    Duration::from_hours(168),
                )
                .await
                .unwrap()
                .is_none()
        );
    }

    /// Interactive-path outcomes are recorded into the same store.
    #[tokio::test]
    async fn empirical_router_learns_from_interactive_outcome() {
        let store = AfterActionStore::in_memory();

        // Simulate 5 interactive turns: 4 success, 1 failure
        let provider = ProviderId::new("claude");
        for i in 0..5u32 {
            let outcome = TurnOutcome::new(provider.clone(), TaskCategory::Feature, i < 4, true);
            store.record_outcome(&outcome).await.unwrap();
        }

        let stats = store
            .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stats.total, 5);
        assert_eq!(stats.successes, 4);
        assert_eq!(stats.failures, 1);
        assert!((stats.success_rate().unwrap() - 0.8).abs() < 0.001);
    }

    /// Dispatch and interactive outcomes land in the same storage backend.
    #[tokio::test]
    async fn dispatch_and_interactive_share_storage() {
        let tmp = tempfile::tempdir().unwrap();

        // Dispatch path: write 3 successes via JSONL
        let lines: Vec<_> = (0..3)
            .map(|_| session_line("shared-provider", "success", "feature"))
            .collect();
        write_jsonl(tmp.path(), "2026-04-22.jsonl", &lines);

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();

        // Interactive path: add 2 more outcomes directly
        let provider = ProviderId::new("shared-provider");
        for i in 0..2u32 {
            let outcome = TurnOutcome::new(
                provider.clone(),
                TaskCategory::Feature,
                i == 0, // 1 success, 1 failure
                true,
            );
            store.record_outcome(&outcome).await.unwrap();
        }

        // Both paths' data should be visible in the same cache
        let stats = store
            .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(1))
            .await
            .unwrap()
            .unwrap();

        // 3 dispatch successes + 1 interactive success + 1 interactive failure = 5 total
        assert_eq!(
            stats.total, 5,
            "dispatch (3) + interactive (2) should sum to 5 total"
        );
        assert_eq!(
            stats.successes, 4,
            "dispatch (3 success) + interactive (1 success) = 4"
        );
        assert_eq!(stats.failures, 1, "interactive contributed 1 failure");
    }

    // --- In-memory mode ---

    #[tokio::test]
    async fn in_memory_store_refresh_is_noop() {
        let store = AfterActionStore::in_memory();
        // Should not error even though there's no directory
        store.refresh().await.unwrap();
    }

    /// When `interactive_outcome` is present, the store must derive success
    /// from the real outcome dimensions rather than the collapsed boolean.
    #[tokio::test]
    async fn record_outcome_derives_success_from_interactive_dimensions() {
        use crate::types::InteractiveOutcome;

        let store = AfterActionStore::in_memory();
        let provider = ProviderId::new("claude");
        // Construct with a stale/wrong collapsed `success = true`; the store
        // must still use the interactive dimensions.
        let outcome = TurnOutcome {
            provider: provider.clone(),
            model: None,
            task_category: TaskCategory::Feature,
            success: true,
            is_interactive: true,
            interactive_outcome: Some(InteractiveOutcome {
                completion: crate::types::CompletionStatus::Completed,
                user_correction: crate::types::CorrectionStatus::Clear,
                tool_error_rate: 1.0,
                loop_guard: crate::types::InterventionStatus::Clear,
                mistake_brake: crate::types::InterventionStatus::Clear,
                budget: crate::types::BudgetStatus::WithinLimit,
                provider: crate::types::ProviderStatus::Available,
                explicit_user_rating: None,
            }),
        };
        store.record_outcome(&outcome).await.unwrap();

        let stats = store
            .rolling_stats(&provider, &TaskCategory::Feature, Duration::from_hours(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.successes, 0);
        assert_eq!(stats.failures, 1);

        let audit = store.recent_outcomes().await;
        assert_eq!(audit.len(), 1);
        assert!(
            audit
                .first()
                .is_some_and(|outcome| outcome.interactive_outcome.is_some())
        );
    }
}
