//! Shared empirical success-rate storage for dispatch and interactive paths.
//!
//! [`AfterActionStore`] maintains a rolling in-memory cache of
//! `(provider_id, task_category) → success_rate` statistics. It has two
//! write paths:
//!
//! - **Dispatch path** ([`AfterActionStore::refresh`]): re-scans the
//!   append-only JSONL log files emitted by the energeia post-processing
//!   stage under `dir`. Called periodically by the daemon maintenance task.
//!
//! - **Interactive path** ([`AfterActionStore::record_outcome`]): increments
//!   the in-memory cache immediately (so a routing decision made right
//!   after sees it), AND (#4519, when the store was constructed with a
//!   backing `dir`) best-effort appends the outcome to a durable
//!   day-partitioned JSONL log under `dir/interactive/` — a directory nous
//!   owns, kept separate from energeia's `dir` so ownership of the two
//!   write paths never collides. [`refresh`](Self::refresh) rebuilds the
//!   cache from BOTH directories, so interactive-path outcomes survive a
//!   refresh instead of being silently discarded (the defect #4519 fixed:
//!   refresh used to replace the cache from `dir` alone, then
//!   unconditionally clear the interactive scratch map with nothing having
//!   read it first). A durable-write failure does not lose data on the next
//!   refresh either: an outcome only stays in the in-memory scratch map
//!   when its durable append failed (a successful append is already
//!   findable on disk, so keeping it in the scratch map too would make
//!   refresh double-count it) — refresh merges that scratch map on top of
//!   the disk scan, so a failed-to-persist entry survives one more refresh
//!   cycle before the scratch map is cleared. A disk failure that persists
//!   across multiple refresh cycles still eventually loses the entry; the
//!   bounded protection is the trade-off, not an unbounded retry queue.
//!
//! [`AfterActionStore::skipped_malformed_lines`] and
//! [`AfterActionStore::failed_interactive_persists`] expose counters for
//! operational visibility into both silent-degradation paths.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use jiff::Timestamp;
use snafu::{ResultExt as _, Snafu};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::sync::RwLock;

use crate::types::{InteractiveOutcome, ProviderId, TaskCategory, TurnOutcome};

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
    /// Directory containing per-day dispatch JSONL files (`YYYY-MM-DD.jsonl`),
    /// owned and written by energeia's post-processing stage.
    ///
    /// `None` when the store is used in memory-only mode (interactive path
    /// without a configured log directory).
    dir: Option<PathBuf>,
    /// Latest per-day JSONL files to include during refresh.
    window: Duration,
    /// In-memory cache: `(provider_id, task_category)` → [`RollingStats`].
    cache: RwLock<HashMap<(ProviderId, TaskCategory), RollingStats>>,
    /// Interactive writes not yet reflected on disk under `dir/interactive/`.
    ///
    /// WHY(#4519): this used to be the *only* record of interactive-path
    /// outcomes, discarded by every `refresh`. `record_outcome` now durably
    /// appends to `dir/interactive/` first (when `dir` is configured); an
    /// entry lands HERE only when that append fails, or when no `dir` is
    /// configured at all (`in_memory()`, where `refresh` never runs). A
    /// successfully-persisted entry is deliberately NOT also added here —
    /// `refresh` would then read it twice (once from disk, once from this
    /// map) and double-count it. `refresh` merges this map on top of the
    /// disk scan before clearing it, so a failed-to-persist entry survives
    /// one more refresh cycle instead of vanishing immediately.
    interactive: RwLock<HashMap<(ProviderId, TaskCategory), RollingStats>>,
    /// Recent interactive outcomes kept for operational audit.
    ///
    /// WHY: lets operators inspect *why* a turn was counted as a success or
    /// failure without replaying the full JSONL archive.
    audit_log: RwLock<VecDeque<TurnOutcome>>,
    /// Interactive outcomes whose durable append to `dir/interactive/`
    /// failed, since store creation (#4519). Non-zero means the operator
    /// should check disk space / permissions on `dir`; the data is not
    /// lost (it stays in `interactive` until the next refresh merges it),
    /// but repeated failures shrink the durability window to "since last
    /// refresh" instead of "forever."
    failed_interactive_persists: AtomicU64,
    /// Malformed JSONL lines skipped across both the dispatch and
    /// interactive logs during the most recent refresh (#4519). Reset to 0
    /// at the start of each `refresh`/`refresh_window` call.
    skipped_malformed_lines: AtomicU64,
}

impl AfterActionStore {
    /// Create a store backed by the given JSONL log directory.
    ///
    /// Call [`refresh`](Self::refresh) to populate the cache from disk.
    /// Interactive-path outcomes recorded via [`record_outcome`](Self::record_outcome)
    /// are durably logged under `dir/interactive/` (#4519).
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
            failed_interactive_persists: AtomicU64::new(0),
            skipped_malformed_lines: AtomicU64::new(0),
        }
    }

    /// Create an in-memory-only store with no JSONL backing directory.
    ///
    /// Suitable for the interactive path when no log directory is configured.
    /// [`refresh`](Self::refresh) is a no-op on this variant, and
    /// interactive outcomes recorded via
    /// [`record_outcome`](Self::record_outcome) are never durably persisted
    /// (there is no `dir` to persist under) — they live only until the
    /// store is dropped, which is the accepted trade-off this variant's
    /// name states.
    pub fn in_memory() -> Self {
        Self {
            dir: None,
            window: DEFAULT_ROUTING_WINDOW,
            cache: RwLock::new(HashMap::new()),
            interactive: RwLock::new(HashMap::new()),
            audit_log: RwLock::new(VecDeque::new()),
            failed_interactive_persists: AtomicU64::new(0),
            skipped_malformed_lines: AtomicU64::new(0),
        }
    }

    /// Directory for durable interactive-path outcome logs, if a backing
    /// `dir` is configured (#4519).
    ///
    /// WHY a subdirectory of `dir` rather than a sibling path: it derives
    /// from the one directory callers already configure (no new
    /// constructor parameter, so every existing call site gets durability
    /// for free), while staying a directory `tokio::fs::read_dir(dir)`
    /// never descends into — `jsonl_paths_in_window` is non-recursive — so
    /// energeia's dispatch scan and this interactive scan can never read
    /// each other's files.
    fn interactive_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join("interactive"))
    }

    /// Interactive outcomes whose durable append has failed since store
    /// creation (#4519). See the field doc on `failed_interactive_persists`.
    #[must_use]
    pub fn failed_interactive_persists(&self) -> u64 {
        self.failed_interactive_persists.load(Ordering::Relaxed)
    }

    /// Malformed JSONL lines skipped during the most recent refresh,
    /// across both the dispatch and interactive logs (#4519).
    #[must_use]
    pub fn skipped_malformed_lines(&self) -> u64 {
        self.skipped_malformed_lines.load(Ordering::Relaxed)
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
            // WHY(#4519): consistency with `refresh_window` — a one-off
            // differently-windowed query must see the same durably-logged
            // interactive data a refresh would fold in, not just the
            // dispatch JSONL files.
            if let Some(interactive_dir) = self.interactive_dir() {
                self.merge_interactive_log(&interactive_dir, window, &mut cache)
                    .await?;
            }
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
    ///
    /// WHY(#5740) this returns nothing: the write path touches only the
    /// in-memory maps and a best-effort durable append, none of which the
    /// caller can act on. It previously returned `Result<(), AfterActionStoreError>`
    /// that was always `Ok`, which gave every caller an error arm no input
    /// could reach — an error handler that cannot fire is indistinguishable
    /// from one that is broken. The fallible operations on this type are the
    /// ones that touch `dir`: [`refresh`](Self::refresh),
    /// [`refresh_window`](Self::refresh_window) and the rebuild branch of
    /// [`rolling_stats`](Self::rolling_stats). Durable-append failures are
    /// instead exposed via [`failed_interactive_persists`](Self::failed_interactive_persists).
    pub async fn record_outcome(&self, outcome: &TurnOutcome) {
        let success = outcome.interactive_outcome.as_ref().map_or(
            outcome.success,
            super::types::InteractiveOutcome::is_success,
        );
        let key = (outcome.provider.clone(), outcome.task_category);

        // WHY: `cache` reflects this outcome immediately regardless of
        // durability, so a routing decision made right after this call sees
        // it — `refresh` rebuilding `cache` from disk is a periodic
        // reconciliation, not the only path that populates it.
        let mut cache = self.cache.write().await;
        record_stats(cache.entry(key.clone()).or_default(), success);
        drop(cache);

        let mut audit_log = self.audit_log.write().await;
        audit_log.push_back(outcome.clone());
        if audit_log.len() > MAX_AUDIT_LOG_SIZE {
            audit_log.pop_front();
        }
        drop(audit_log);

        // WHY(#4519): best-effort durable persistence, then only add to the
        // `interactive` scratch map on FAILURE. A successful durable append
        // will be picked up from disk by the next refresh's
        // `merge_interactive_log` scan; adding it to `interactive` too would
        // make refresh double-count it (both the disk-scanned copy and the
        // scratch-map copy). An entry only belongs in `interactive` when
        // disk does not yet have it — i.e. no `dir` is configured at all
        // (`in_memory()` — refresh never touches this map in that mode, by
        // design) or the durable append just failed, in which case refresh's
        // belt-and-suspenders merge is what keeps it alive for one more
        // cycle rather than dropping it on the spot.
        let mut persisted = false;
        if let Some(interactive_dir) = self.interactive_dir() {
            match append_interactive_outcome(&interactive_dir, outcome, success).await {
                Ok(()) => persisted = true,
                Err(e) => {
                    self.failed_interactive_persists
                        .fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        dir = %interactive_dir.display(),
                        error = %e,
                        "failed to durably persist interactive routing outcome; \
                         kept in memory until next refresh"
                    );
                }
            }
        }
        if !persisted {
            let mut interactive = self.interactive.write().await;
            record_stats(interactive.entry(key).or_default(), success);
        }
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
    ///
    /// WHY(#4519): rebuilds from BOTH the dispatch JSONL files under `dir`
    /// and the durable interactive log under `dir/interactive/`, then
    /// merges the in-memory `interactive` scratch map on top (covering any
    /// outcome whose durable append failed) before clearing it. Earlier,
    /// this replaced the cache from `dir` alone and unconditionally cleared
    /// `interactive` with nothing having read it — interactive-path
    /// outcomes were silently discarded on every refresh.
    pub async fn refresh_window(&self, window: Duration) -> Result<(), AfterActionStoreError> {
        let Some(ref dir) = self.dir else {
            return Ok(());
        };
        self.skipped_malformed_lines.store(0, Ordering::Relaxed);

        let mut new_cache = self.build_cache(dir, window).await?;
        if let Some(interactive_dir) = self.interactive_dir() {
            self.merge_interactive_log(&interactive_dir, window, &mut new_cache)
                .await?;
        }
        self.merge_interactive(&mut new_cache).await;

        let mut cache = self.cache.write().await;
        *cache = new_cache;
        drop(cache);
        let mut interactive = self.interactive.write().await;
        interactive.clear();
        Ok(())
    }

    /// Build a fresh cache by scanning all dispatch JSONL files in `dir`.
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

    /// Stream a single dispatch JSONL file and accumulate stats into `map`.
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
                    // Counted (#4519) so a run of truncated writes is visible
                    // rather than only ever showing up as quietly thin data.
                    self.skipped_malformed_lines.fetch_add(1, Ordering::Relaxed);
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

    /// Scan the durable interactive-outcome log under `interactive_dir` and
    /// merge its contents into `map` (#4519).
    ///
    /// A missing `interactive_dir` is not an error: it means no interactive
    /// outcome has ever been durably persisted yet (e.g. a fresh instance,
    /// or one that has only ever run the dispatch path).
    async fn merge_interactive_log(
        &self,
        interactive_dir: &Path,
        window: Duration,
        map: &mut HashMap<(ProviderId, TaskCategory), RollingStats>,
    ) -> Result<(), AfterActionStoreError> {
        let paths = match jsonl_paths_in_window(interactive_dir, window).await {
            Ok(paths) => paths,
            Err(AfterActionStoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        for path in paths {
            self.scan_interactive_file(&path, map).await?;
        }
        Ok(())
    }

    /// Stream a single interactive-outcome JSONL file and accumulate stats
    /// into `map` (#4519). Mirrors [`scan_file`](Self::scan_file)'s
    /// line-reading and malformed-line policy for the interactive wire
    /// format ([`InteractiveOutcomeLine`]) instead of the dispatch one.
    async fn scan_interactive_file(
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
            match serde_json::from_str::<InteractiveOutcomeLine>(&line) {
                Ok(record) => {
                    let key = (record.provider_id(), record.category());
                    record_stats(map.entry(key).or_default(), record.success);
                }
                Err(e) => {
                    self.skipped_malformed_lines.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(
                        path = %path.display(),
                        error = %e,
                        "skipping malformed interactive outcome line"
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

/// Durable JSONL wire shape for one interactive-path outcome (#4519).
///
/// A dedicated shape rather than deriving `Serialize`/`Deserialize` directly
/// on [`TurnOutcome`]: this decouples the durable format from the in-memory
/// API type, the same way [`AfterActionLine`] already decouples the
/// dispatch-path wire format from energeia's internal types.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct InteractiveOutcomeLine {
    provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    /// WHY: a plain string via `Display`/`parse_category`, not `TaskCategory`'s
    /// own derived (unrenamed, so case-sensitive-on-the-Rust-identifier)
    /// `Serialize`/`Deserialize` — matches [`AfterActionSession::category`]'s
    /// existing convention on the dispatch side, and the same
    /// unrecognized-category-degrades-to-`Feature` forward-compatibility
    /// applies here (a future `TaskCategory` variant must not turn every
    /// interactive line written before it existed into a malformed line).
    task_category: String,
    success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interactive_outcome: Option<InteractiveOutcome>,
}

impl InteractiveOutcomeLine {
    fn from_outcome(outcome: &TurnOutcome, success: bool) -> Self {
        Self {
            provider: outcome.provider.to_string(),
            model: outcome.model.as_ref().map(ToString::to_string),
            task_category: outcome.task_category.to_string(),
            success,
            interactive_outcome: outcome.interactive_outcome.clone(),
        }
    }

    fn category(&self) -> TaskCategory {
        parse_category(&self.task_category)
    }

    fn provider_id(&self) -> ProviderId {
        ProviderId::new(self.provider.as_str())
    }
}

/// Append one interactive outcome to today's durable JSONL file under
/// `interactive_dir` (#4519), creating the directory on first use.
async fn append_interactive_outcome(
    interactive_dir: &Path,
    outcome: &TurnOutcome,
    success: bool,
) -> std::io::Result<()> {
    tokio::fs::create_dir_all(interactive_dir).await?;

    let today = Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d");
    let path = interactive_dir.join(format!("{today}.jsonl"));

    let line = InteractiveOutcomeLine::from_outcome(outcome, success);
    let mut json = serde_json::to_string(&line)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    json.push('\n');

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    file.write_all(json.as_bytes()).await?;
    file.flush().await
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
            store.record_outcome(&outcome).await;
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
            store.record_outcome(&outcome).await;
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

    // --- Durable interactive persistence (#4519) ---

    /// THE regression test for #4519: `refresh` used to rebuild `cache`
    /// from the dispatch JSONL files alone, then unconditionally clear the
    /// `interactive` scratch map — so an interactive-path outcome vanished
    /// from `rolling_stats` the moment any refresh ran, durable or not.
    #[tokio::test]
    async fn interactive_outcome_survives_refresh() {
        let tmp = tempfile::tempdir().unwrap();
        let store = AfterActionStore::new(tmp.path().to_owned());

        // A dispatch-side refresh with zero JSONL files yet (an empty/fresh
        // `dir`) — this is the case that used to wipe interactive data.
        store.refresh().await.unwrap();

        let provider = ProviderId::new("claude");
        let outcome = TurnOutcome::new(provider.clone(), TaskCategory::Feature, true, true);
        store.record_outcome(&outcome).await;

        // Before the fix this refresh would drop the outcome just recorded.
        store.refresh().await.unwrap();

        // WHY: `.unwrap()` on the outer AND inner `Option` — a `None` here
        // means the interactive outcome did not survive the refresh, which
        // is exactly the regression this test exists to catch.
        let stats = store
            .rolling_stats(&provider, &TaskCategory::Feature, DEFAULT_ROUTING_WINDOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.successes, 1);
    }

    #[tokio::test]
    async fn interactive_outcome_is_durably_logged_to_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let store = AfterActionStore::new(tmp.path().to_owned());

        let provider = ProviderId::new("claude");
        let outcome = TurnOutcome::new(provider.clone(), TaskCategory::Bug, false, true);
        store.record_outcome(&outcome).await;

        let interactive_dir = tmp.path().join("interactive");
        let mut entries = std::fs::read_dir(&interactive_dir)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(entries.len(), 1, "one day-partitioned JSONL file");
        let content = std::fs::read_to_string(entries.remove(0).path()).unwrap();
        assert!(content.contains("\"claude\""));
        assert!(content.contains("\"bug\""));
        assert_eq!(
            store.failed_interactive_persists(),
            0,
            "the append above must have succeeded"
        );
    }

    #[tokio::test]
    async fn interactive_outcomes_persist_across_store_restarts() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = ProviderId::new("claude");

        {
            let store = AfterActionStore::new(tmp.path().to_owned());
            let outcome = TurnOutcome::new(provider.clone(), TaskCategory::Feature, true, true);
            store.record_outcome(&outcome).await;
            // Store dropped here — an in-memory-only mechanism would lose it.
        }

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();
        // WHY: a `None` here means the durably-logged interactive outcome
        // did not survive a fresh store instance reading it back from disk.
        let stats = store
            .rolling_stats(&provider, &TaskCategory::Feature, DEFAULT_ROUTING_WINDOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stats.total, 1);
    }

    #[tokio::test]
    async fn refresh_skips_malformed_interactive_lines_and_counts_them() {
        let tmp = tempfile::tempdir().unwrap();
        let interactive_dir = tmp.path().join("interactive");
        std::fs::create_dir_all(&interactive_dir).unwrap();
        let mut file = std::fs::File::create(interactive_dir.join("2026-04-17.jsonl")).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "provider": "claude",
                "task_category": "feature",
                "success": true,
            })
        )
        .unwrap();
        writeln!(file, "{{\"incomplete\":").unwrap();
        drop(file);

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();

        // WHY: a `None` here means the well-formed line alongside the
        // malformed one was not counted either.
        let stats = store
            .rolling_stats(
                &ProviderId::new("claude"),
                &TaskCategory::Feature,
                DEFAULT_ROUTING_WINDOW,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(
            store.skipped_malformed_lines(),
            1,
            "the truncated line must be counted, not just silently dropped"
        );
    }

    #[tokio::test]
    async fn dispatch_and_durably_persisted_interactive_share_storage_after_refresh() {
        let tmp = tempfile::tempdir().unwrap();

        let lines: Vec<_> = (0..3)
            .map(|_| session_line("shared-provider", "success", "feature"))
            .collect();
        write_jsonl(tmp.path(), "2026-04-22.jsonl", &lines);

        let store = AfterActionStore::new(tmp.path().to_owned());
        let provider = ProviderId::new("shared-provider");
        for i in 0..2u32 {
            let outcome = TurnOutcome::new(provider.clone(), TaskCategory::Feature, i == 0, true);
            store.record_outcome(&outcome).await;
        }

        // Unlike `dispatch_and_interactive_share_storage` above (which reads
        // the pre-refresh scratch map), this refreshes first — the case
        // that used to drop the interactive contribution entirely.
        store.refresh().await.unwrap();

        let stats = store
            .rolling_stats(&provider, &TaskCategory::Feature, DEFAULT_ROUTING_WINDOW)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stats.total, 5, "dispatch (3) + interactive (2) = 5");
        assert_eq!(stats.successes, 4);
        assert_eq!(stats.failures, 1);
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
        store.record_outcome(&outcome).await;

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
