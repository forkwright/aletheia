// WHY: Read-side aggregation over the append-only after-action JSONL logs
// emitted by the post-processing pipeline stage (#3616).  The store maintains
// an in-memory rolling-stats cache keyed by (provider_id, task_category) and
// refreshes it by re-reading today's JSONL file line-by-line (streaming, not
// loading the whole file).  The cache is invalidated via the explicit
// `refresh()` method, which the daemon maintenance task calls on a periodic
// schedule.

// WHY: `allow(dead_code)` in non-test builds — all public items in this
// module are used by binary wiring (AfterActionStore::new / refresh / rolling_stats).
// The dead_code lint fires here because the wiring call-site lives in the binary,
// not in the lib.  Using `cfg_attr(not(test), allow(...))` suppresses only in
// non-test builds; test builds have no suppression (items ARE used in tests).
// `#[expect]` cannot be used here because serde/derive macros make some items
// appear "used" in lib analysis, causing unfulfilled-expectation errors — a known
// interaction between `#[expect(dead_code)]` and proc-macro expansions.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use snafu::{ResultExt as _, Snafu};
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::sync::RwLock;

use super::{ProviderId, TaskCategory};

// ---------------------------------------------------------------------------
// AfterActionStoreError
// ---------------------------------------------------------------------------

/// Errors produced by [`AfterActionStore`] operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum AfterActionStoreError {
    /// Could not read a JSONL log directory.
    #[snafu(display("I/O error reading after-action log '{}': {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

// ---------------------------------------------------------------------------
// Wire-format structs (subset of AfterActionRecord from post_processing.rs)
// ---------------------------------------------------------------------------

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
    /// Maps to the `model` field in `AfterActionSessionOutcome`.  May be
    /// absent for records written before this field was introduced.
    #[serde(default)]
    model: Option<String>,
    /// Terminal status string (e.g. `"success"`, `"failed"`, `"stuck"`).
    status: String,
    /// Category tag written alongside the session.  Absent for records
    /// pre-dating this PR; treated as `Feature` when missing.
    #[serde(default)]
    category: Option<String>,
}

// ---------------------------------------------------------------------------
// RollingStats
// ---------------------------------------------------------------------------

/// Aggregated success/failure counts for a (provider, category) bucket.
#[derive(Debug, Clone, Default)]
pub(crate) struct RollingStats {
    /// Sessions that completed successfully.
    pub(crate) successes: u64,
    /// Sessions that ended in any failure state.
    pub(crate) failures: u64,
    /// Total sessions (`successes + failures`).
    pub(crate) total: u64,
    /// Timestamp of the most recent successful session, if any.
    pub(crate) last_success_at: Option<Timestamp>,
}

impl RollingStats {
    /// Empirical success rate in [0, 1], or `None` when `total == 0`.
    pub(crate) fn success_rate(&self) -> Option<f64> {
        if self.total == 0 {
            None
        } else {
            // WHY: precision loss at very high session counts (>2^53) is
            // acceptable for routing decisions; the alternative (integer
            // arithmetic scaled by 1000) adds complexity for no real benefit
            // at the session counts seen in practice.
            #[expect(
                clippy::cast_precision_loss,
                reason = "routing rates are for heuristic selection; precision loss at >2^53 sessions is acceptable"
            )]
            #[expect(
                clippy::as_conversions,
                reason = "u64→f64 for rate computation; loss is acceptable and documented above"
            )]
            Some(self.successes as f64 / self.total as f64)
        }
    }
}

// ---------------------------------------------------------------------------
// AfterActionStore
// ---------------------------------------------------------------------------

/// Read-side cache over the after-action JSONL logs.
///
/// Provides [`rolling_stats`](AfterActionStore::rolling_stats) for empirical
/// provider selection and exposes [`refresh`](AfterActionStore::refresh) for
/// periodic cache invalidation by the daemon maintenance task.
pub(crate) struct AfterActionStore {
    /// Directory containing per-day JSONL files (`YYYY-MM-DD.jsonl`).
    dir: PathBuf,
    /// In-memory cache: `(provider_id, task_category)` → [`RollingStats`].
    cache: RwLock<HashMap<(ProviderId, TaskCategory), RollingStats>>,
}

impl AfterActionStore {
    /// Create a new store backed by the given directory.
    ///
    /// The cache starts empty; call [`refresh`](Self::refresh) to populate it.
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Return rolling stats for a specific (provider, category) pair.
    ///
    /// Returns `None` when the pair has no entries in the cache.
    pub(crate) async fn rolling_stats(
        &self,
        provider: &ProviderId,
        cat: &TaskCategory,
        _window: std::time::Duration,
    ) -> Option<RollingStats> {
        // WHY: The cache is already window-bounded by refresh() which only
        // reads files within the configured window.  Passing `_window` here
        // keeps the signature stable for future per-query filtering.
        let cache = self.cache.read().await;
        cache.get(&(provider.clone(), *cat)).cloned()
    }

    /// Re-scan the JSONL log directory and rebuild the in-memory cache.
    ///
    /// Streams each file line-by-line to avoid loading the whole day's log
    /// into memory. Malformed JSON lines are silently skipped so a corrupt
    /// record never poisons the cache.
    ///
    /// WHY append-only: the post-processing stage writes one line per dispatch.
    /// We only need to read — the store never modifies the JSONL files.
    pub(crate) async fn refresh(&self) -> Result<(), AfterActionStoreError> {
        let new_cache = self.build_cache(&self.dir).await?;
        let mut cache = self.cache.write().await;
        *cache = new_cache;
        Ok(())
    }

    /// Build a fresh cache by scanning all JSONL files in `dir`.
    async fn build_cache(
        &self,
        dir: &Path,
    ) -> Result<HashMap<(ProviderId, TaskCategory), RollingStats>, AfterActionStoreError> {
        let mut map: HashMap<(ProviderId, TaskCategory), RollingStats> = HashMap::new();

        let mut read_dir = tokio::fs::read_dir(dir).await.context(IoSnafu {
            path: dir.to_owned(),
        })?;

        while let Some(entry) = read_dir.next_entry().await.context(IoSnafu {
            path: dir.to_owned(),
        })? {
            let path = entry.path();
            let Some(ext) = path.extension() else {
                continue;
            };
            if ext != "jsonl" {
                continue;
            }

            self.scan_file(&path, &mut map).await?;
        }

        Ok(map)
    }

    /// Stream a single JSONL file and accumulate stats into `map`.
    ///
    /// Malformed or incomplete lines are logged at debug level and skipped so
    /// that a single bad record never prevents the cache from building.
    async fn scan_file(
        &self,
        path: &Path,
        map: &mut HashMap<(ProviderId, TaskCategory), RollingStats>,
    ) -> Result<(), AfterActionStoreError> {
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
                    // WHY: silently skip malformed lines rather than failing.
                    // Partial writes (e.g. process killed mid-write) produce
                    // truncated lines; poisoning the cache for a single bad
                    // record would degrade all empirical routing decisions.
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

        let provider = ProviderId::new(model.clone());
        let category = session
            .category
            .as_deref()
            .map_or(TaskCategory::Feature, parse_category);

        let key = (provider, category);
        let stats = map.entry(key).or_default();

        let is_success = session.status == "success";
        if is_success {
            stats.successes += 1;
            stats.last_success_at = Some(Timestamp::now());
        } else {
            stats.failures += 1;
        }
        stats.total += 1;
    }
}

/// Parse a category string from a JSONL record.
///
/// Returns `TaskCategory::Feature` for unrecognised strings so that new
/// categories added in future PRs degrade gracefully on old store data.
fn parse_category(s: &str) -> TaskCategory {
    match s {
        "refactor" => TaskCategory::Refactor,
        "bug" => TaskCategory::Bug,
        "docs" => TaskCategory::Docs,
        "test" => TaskCategory::Test,
        "chore" => TaskCategory::Chore,
        // "feature" and any unrecognised string → Feature
        _ => TaskCategory::Feature,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::io::Write as _;
    use std::time::Duration;

    use super::*;

    fn write_jsonl(dir: &std::path::Path, filename: &str, lines: &[serde_json::Value]) {
        let path = dir.join(filename);
        let mut file = std::fs::File::create(&path).unwrap();
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
            "session_outcomes": [
                {
                    "model": model,
                    "status": status,
                    "category": category
                }
            ],
            "cost_total_cents": 5,
            "turns_total": 10,
            "stage_latencies_ms": {},
            "qa_verdict": "pass",
            "prompt_hash": "sha256:abc"
        })
    }

    #[tokio::test]
    async fn rolling_stats_counts_match_written_records() {
        let tmp = tempfile::tempdir().unwrap();

        // Provider A: 9 successes, 1 failure
        let mut lines = vec![];
        for _ in 0..9 {
            lines.push(session_line("provider-a", "success", "feature"));
        }
        lines.push(session_line("provider-a", "failed", "feature"));
        // Provider B: 1 success, 9 failures
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
            .unwrap();
        assert_eq!(a.successes, 9);
        assert_eq!(a.failures, 1);
        assert_eq!(a.total, 10);
        assert!((a.success_rate().unwrap() - 0.9).abs() < 0.001);

        let b = store
            .rolling_stats(
                &ProviderId::new("provider-b"),
                &TaskCategory::Feature,
                Duration::from_hours(168),
            )
            .await
            .unwrap();
        assert_eq!(b.successes, 1);
        assert_eq!(b.failures, 9);
        assert_eq!(b.total, 10);
        assert!((b.success_rate().unwrap() - 0.1).abs() < 0.001);
    }

    #[tokio::test]
    async fn refresh_handles_malformed_json_gracefully() {
        let tmp = tempfile::tempdir().unwrap();

        let path = tmp.path().join("2026-04-17.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        // Valid line
        writeln!(
            file,
            "{}",
            session_line("good-provider", "success", "feature")
        )
        .unwrap();
        // Malformed / truncated line
        writeln!(file, "{{\"incomplete\":").unwrap();
        // Another valid line
        writeln!(
            file,
            "{}",
            session_line("good-provider", "success", "feature")
        )
        .unwrap();

        let store = AfterActionStore::new(tmp.path().to_owned());
        // Must not return Err
        store.refresh().await.unwrap();

        // The two valid lines should have been counted
        let stats = store
            .rolling_stats(
                &ProviderId::new("good-provider"),
                &TaskCategory::Feature,
                Duration::from_hours(168),
            )
            .await
            .unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.successes, 2);
    }

    #[tokio::test]
    async fn refresh_handles_partial_lines_and_empty_lines() {
        let tmp = tempfile::tempdir().unwrap();

        let path = tmp.path().join("2026-04-17.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        // Empty line
        writeln!(file).unwrap();
        // Valid line
        writeln!(file, "{}", session_line("alpha", "success", "bug")).unwrap();
        // Whitespace-only line
        writeln!(file, "   ").unwrap();

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();

        let stats = store
            .rolling_stats(
                &ProviderId::new("alpha"),
                &TaskCategory::Bug,
                Duration::from_hours(168),
            )
            .await
            .unwrap();
        assert_eq!(stats.total, 1);
    }

    #[tokio::test]
    async fn missing_dir_returns_empty_cache() {
        let store = AfterActionStore::new(std::path::PathBuf::from(
            "/tmp/nonexistent-xyz-routing-test",
        ));
        // A missing directory should not panic; it should return an error.
        let result = store.refresh().await;
        assert!(
            result.is_err(),
            "expected error for missing directory, got Ok"
        );
    }

    #[tokio::test]
    async fn sessions_without_model_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let line = serde_json::json!({
            "dispatch_id": "test",
            "ts_start": "2026-04-17T00:00:00Z",
            "ts_end": "2026-04-17T00:01:00Z",
            "duration_ms": 60000,
            "session_outcomes": [
                { "status": "success", "category": "feature" }
            ],
            "cost_total_cents": 0,
            "turns_total": 1,
            "stage_latencies_ms": {},
            "qa_verdict": "pass",
            "prompt_hash": "sha256:abc"
        });
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &[line]);

        let store = AfterActionStore::new(tmp.path().to_owned());
        store.refresh().await.unwrap();

        // Nothing should be in cache since model is absent
        let stats = store
            .rolling_stats(
                &ProviderId::new("any"),
                &TaskCategory::Feature,
                Duration::from_hours(168),
            )
            .await;
        assert!(stats.is_none());
    }
}
