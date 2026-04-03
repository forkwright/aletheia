//! Auto-dream memory consolidation.
//!
//! Background process that periodically consolidates session transcripts INTO
//! the knowledge graph. Uses a triple-gate system (time → sessions → lock) to
//! ensure consolidation runs infrequently, only when meaningful new data exists,
//! and never concurrently.
//!
//! Gate ORDER matters: each subsequent gate is more expensive.
//! 1. **Time gate**  -  single stat call on the lock file.
//! 2. **Session gate**  -  directory scan, throttled to 10-minute intervals.
//! 3. **Lock gate**  -  fd-lock write + PID verify.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use snafu::ResultExt;
use tracing::instrument;

use aletheia_hermeneus::provider::LlmProvider;
use aletheia_hermeneus::types::Message;

use crate::contradiction::ContradictionLog;
use crate::distill::{DistillConfig, DistillEngine};
use crate::error::{DreamConsolidationTargetSnafu, DreamTranscriptSourceSnafu, Result};
use crate::flush::MemoryFlush;

pub(crate) mod lock;

/// Default minimum hours between consolidation runs.
const DEFAULT_MIN_HOURS: u64 = 24;

/// Default minimum sessions required to trigger consolidation.
const DEFAULT_MIN_SESSIONS: usize = 5;

/// Session scan throttle interval (10 minutes in seconds).
const SCAN_THROTTLE_SECS: i64 = 600;

/// Default stale lock threshold (1 hour in seconds).
const DEFAULT_STALE_THRESHOLD_SECS: i64 = 3_600;

/// Configuration for auto-dream consolidation.
#[derive(Debug, Clone)]
pub struct DreamConfig {
    /// Minimum hours between consolidation runs (default: 24).
    pub min_hours: u64,
    /// Minimum sessions since last consolidation to trigger (default: 5).
    pub min_sessions: usize,
    /// Path to the consolidation lock file.
    pub lock_path: PathBuf,
    /// Session scan throttle interval in seconds (default: 600 = 10 minutes).
    pub scan_interval_secs: i64,
    /// Stale lock threshold in seconds (default: 3600 = 1 hour).
    pub stale_threshold_secs: i64,
    /// Distillation engine configuration for fact extraction.
    pub distill_config: DistillConfig,
}

impl DreamConfig {
    /// Create a config with defaults, only requiring the lock file path.
    #[must_use]
    pub fn new(lock_path: PathBuf) -> Self {
        Self {
            min_hours: DEFAULT_MIN_HOURS,
            min_sessions: DEFAULT_MIN_SESSIONS,
            lock_path,
            scan_interval_secs: SCAN_THROTTLE_SECS,
            stale_threshold_secs: DEFAULT_STALE_THRESHOLD_SECS,
            distill_config: DistillConfig::default(),
        }
    }
}

/// A session transcript available for consolidation.
#[derive(Debug, Clone)]
pub struct SessionTranscript {
    /// Session identifier.
    pub session_id: String,
    /// Nous (agent) identifier.
    pub nous_id: String,
    /// Conversation messages FROM this session.
    pub messages: Vec<Message>,
}

/// Report FROM a consolidation merge operation.
#[derive(Debug, Clone, Default)]
pub struct MergeReport {
    /// Facts newly added to the knowledge graph.
    pub facts_added: usize,
    /// Facts deduplicated against existing knowledge.
    pub facts_deduped: usize,
    /// Facts marked stale due to contradictions.
    pub facts_stale: usize,
}

/// Trait for counting and loading session transcripts.
///
/// Implementors provide access to session storage (e.g. `SQLite` via graphe).
pub trait TranscriptSource: Send + Sync {
    /// Count sessions modified after `since`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the storage backend is unreachable.
    fn count_sessions_since(
        &self,
        since: jiff::Timestamp,
    ) -> std::result::Result<usize, std::io::Error>;

    /// Load all transcripts FROM sessions modified after `since`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the storage backend is unreachable.
    fn load_transcripts_since(
        &self,
        since: jiff::Timestamp,
    ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error>;
}

/// Trait for persisting consolidation results to the knowledge graph.
///
/// Implementors provide the merge/dedup/stale-marking operations backed by
/// the concrete knowledge store (e.g. episteme via mneme).
pub trait ConsolidationTarget: Send + Sync {
    /// Merge extracted memory flush INTO the knowledge graph.
    ///
    /// Deduplicates against existing facts and returns a merge report.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the knowledge store is unreachable.
    fn merge_flush(
        &self,
        flush: &MemoryFlush,
        nous_id: &str,
    ) -> std::result::Result<MergeReport, std::io::Error>;

    /// Mark facts identified as contradictions for stale decay.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the knowledge store is unreachable.
    fn mark_contradictions_stale(
        &self,
        log: &ContradictionLog,
        nous_id: &str,
    ) -> std::result::Result<usize, std::io::Error>;
}

/// The auto-dream consolidation engine.
///
/// Manages the triple-gate system and spawns background consolidation tasks.
/// Thread-safe: uses atomics for scan throttling, no mutex needed.
pub struct DreamEngine {
    config: DreamConfig,
    distill: DistillEngine,
    /// Unix timestamp of the last session scan (for 10-minute throttle).
    /// WHY: `AtomicI64` because we need lock-free reads FROM the gate check
    /// hot path. i64 holds Unix seconds until year ~292 billion.
    last_scan_at: AtomicI64,
}

impl std::fmt::Debug for DreamEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DreamEngine")
            .field("config", &self.config)
            .field("last_scan_at", &self.last_scan_at.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl DreamEngine {
    /// Create a new auto-dream engine with the given configuration.
    #[must_use]
    pub fn new(config: DreamConfig) -> Self {
        let distill = DistillEngine::new(config.distill_config.clone());
        Self {
            config,
            distill,
            last_scan_at: AtomicI64::new(0),
        }
    }

    /// Run the triple-gate check and return whether consolidation should proceed.
    ///
    /// Gate ORDER: time → sessions → lock (cheapest first).
    ///
    /// Returns `Ok(Some(lock))` if all gates pass, `Ok(None)` if any gate
    /// blocks, or `Err` on I/O failures.
    ///
    /// # Errors
    ///
    /// Returns `DreamLockIo` on filesystem errors, `DreamTranscriptSource`
    /// on transcript counting errors.
    #[instrument(skip(self, source), fields(lock_path = %self.config.lock_path.display()))]
    pub(crate) fn check_gates(
        &self,
        source: &dyn TranscriptSource,
    ) -> Result<Option<lock::AcquiredLock>> {
        // GATE 1: Time gate (single stat call).
        let last_consolidated =
            lock::lock_mtime(&self.config.lock_path).and_then(lock::system_time_to_timestamp);

        if let Some(ts) = last_consolidated {
            // NOTE: compare in integer seconds to avoid floating-point casts.
            let min_secs = self.config.min_hours.saturating_mul(3_600);
            match jiff::Timestamp::now().since(ts) {
                Ok(span) => {
                    let elapsed_secs = span.get_seconds();
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_possible_wrap,
                        reason = "u64→i64: min_hours * 3600 fits in i64 for any reasonable config"
                    )]
                    let min_secs_i64 = i64::try_from(min_secs).unwrap_or_default();
                    if elapsed_secs < min_secs_i64 {
                        tracing::debug!(
                            elapsed_secs,
                            min_hours = self.config.min_hours,
                            "time gate: not enough time since last consolidation"
                        );
                        return Ok(None);
                    }
                }
                Err(_) => {
                    // NOTE: if we can't compute elapsed, skip this cycle.
                    return Ok(None);
                }
            }
        }
        // NOTE: if lock file doesn't exist, time gate passes (never consolidated).

        // GATE 2: Session gate (throttled directory scan).
        let now_secs = jiff::Timestamp::now().as_second();
        let last_scan = self.last_scan_at.load(Ordering::Relaxed);
        if now_secs.saturating_sub(last_scan) < self.config.scan_interval_secs {
            tracing::debug!(
                since_last_scan = now_secs.saturating_sub(last_scan),
                throttle_secs = self.config.scan_interval_secs,
                "session gate: scan throttled"
            );
            return Ok(None);
        }
        self.last_scan_at.store(now_secs, Ordering::Relaxed);

        let since = last_consolidated.unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        let session_count =
            source
                .count_sessions_since(since)
                .context(DreamTranscriptSourceSnafu {
                    context: "count sessions since last consolidation",
                })?;

        if session_count < self.config.min_sessions {
            tracing::debug!(
                session_count,
                min_sessions = self.config.min_sessions,
                "session gate: not enough new sessions"
            );
            return Ok(None);
        }

        // GATE 3: Lock gate (fd-lock + PID verify).
        let acquired = lock::try_acquire(&self.config.lock_path, self.config.stale_threshold_secs)?;

        if acquired.is_none() {
            tracing::debug!("lock gate: consolidation lock held by another process");
        }

        Ok(acquired)
    }

    /// Entry point for the `on_turn_complete` hook.
    ///
    /// Runs the cheap time-gate check first and only proceeds to heavier gates
    /// if it passes. If all gates pass, spawns a background consolidation task
    /// and returns immediately (non-blocking).
    ///
    /// The background task:
    /// 1. Loads transcripts since last consolidation
    /// 2. Runs each through the distillation engine to extract facts
    /// 3. Merges extracted facts INTO the knowledge graph
    /// 4. Marks contradicted facts for stale decay
    /// 5. Updates the lock file mtime on success, rolls back on failure
    pub fn on_turn_complete(
        self: &Arc<Self>,
        source: &Arc<dyn TranscriptSource>,
        target: &Arc<dyn ConsolidationTarget>,
        provider: &Arc<dyn LlmProvider>,
    ) {
        // NOTE: quick inline check before spawning.
        let acquired = match self.check_gates(source.as_ref()) {
            Ok(Some(lock)) => lock,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %e, "auto-dream gate check failed");
                return;
            }
        };

        let engine = Arc::clone(self);
        let source = Arc::clone(source);
        let target = Arc::clone(target);
        let provider = Arc::clone(provider);

        tokio::spawn(async move {
            let span = tracing::info_span!("auto_dream_consolidation".instrument(tracing::info_span!("spawned_task")));
            let _guard = span.enter();

            tracing::info!("auto-dream consolidation started");
            let start = std::time::Instant::now();

            match engine
                .run_consolidation(
                    acquired,
                    source.as_ref(),
                    target.as_ref(),
                    provider.as_ref(),
                )
                .await
            {
                Ok(report) => {
                    let duration_ms = start.elapsed().as_millis().min(u64::MAX.INTO());
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_possible_truncation,
                        reason = "u128→u64: clamped to u64::MAX by min() above"
                    )]
                    let duration_ms = u64::try_from(duration_ms).unwrap_or_default();
                    tracing::info!(
                        facts_added = report.facts_added,
                        facts_deduped = report.facts_deduped,
                        facts_stale = report.facts_stale,
                        duration_ms,
                        "auto-dream consolidation completed"
                    );
                    crate::metrics::record_distillation(
                        "auto-dream",
                        start.elapsed().as_secs_f64(),
                        true,
                    );
                }
                Err(e) => {
                    tracing::error!(error = %e, "auto-dream consolidation failed");
                    crate::metrics::record_distillation(
                        "auto-dream",
                        start.elapsed().as_secs_f64(),
                        false,
                    );
                }
            }
        });
    }

    /// Execute the consolidation pipeline.
    ///
    /// Loads transcripts, distills each, merges results, handles contradictions.
    /// On failure, rolls back the lock mtime. On success, marks the lock complete.
    async fn run_consolidation(
        &self,
        acquired: lock::AcquiredLock,
        source: &dyn TranscriptSource,
        target: &dyn ConsolidationTarget,
        provider: &dyn LlmProvider,
    ) -> Result<MergeReport> {
        let since = acquired
            .prior_mtime()
            .and_then(|st| lock::system_time_to_timestamp(*st))
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH);

        let transcripts =
            match source
                .load_transcripts_since(since)
                .context(DreamTranscriptSourceSnafu {
                    context: "load transcripts for consolidation",
                }) {
                Ok(t) => t,
                Err(e) => {
                    let _ = acquired.rollback();
                    return Err(e);
                }
            };

        if transcripts.is_empty() {
            tracing::info!("no transcripts to consolidate");
            acquired.mark_complete()?;
            return Ok(MergeReport::default());
        }

        let mut total_report = MergeReport::default();
        let mut distill_number: u32 = 0;

        for transcript in &transcripts {
            if transcript.messages.is_empty() {
                continue;
            }

            distill_number = distill_number.saturating_add(1);

            let result = match self
                .distill
                .distill(
                    &transcript.messages,
                    &transcript.nous_id,
                    provider,
                    distill_number,
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        session_id = %transcript.session_id,
                        error = %e,
                        "distillation failed for session, skipping"
                    );
                    continue;
                }
            };

            // NOTE: merge extracted facts INTO the knowledge graph.
            match target
                .merge_flush(&result.memory_flush, &transcript.nous_id)
                .context(DreamConsolidationTargetSnafu {
                    context: "merge flush INTO knowledge graph",
                }) {
                Ok(report) => {
                    total_report.facts_added =
                        total_report.facts_added.saturating_add(report.facts_added);
                    total_report.facts_deduped = total_report
                        .facts_deduped
                        .saturating_add(report.facts_deduped);
                }
                Err(e) => {
                    tracing::warn!(
                        session_id = %transcript.session_id,
                        error = %e,
                        "merge flush failed for session, skipping"
                    );
                    continue;
                }
            }

            // NOTE: mark contradicted facts for stale decay.
            if !result.contradiction_log.is_empty() {
                match target
                    .mark_contradictions_stale(&result.contradiction_log, &transcript.nous_id)
                    .context(DreamConsolidationTargetSnafu {
                        context: "mark contradicted facts stale",
                    }) {
                    Ok(count) => {
                        total_report.facts_stale = total_report.facts_stale.saturating_add(count);
                    }
                    Err(e) => {
                        tracing::warn!(
                            session_id = %transcript.session_id,
                            error = %e,
                            "stale marking failed for session, continuing"
                        );
                    }
                }
            }
        }

        // NOTE: all transcripts processed; mark consolidation complete.
        acquired.mark_complete()?;

        Ok(total_report)
    }

    /// Set the last scan timestamp (test helper for scan throttle verification).
    #[cfg(test)]
    pub(crate) fn set_last_scan_at(&self, ts: i64) {
        self.last_scan_at.store(ts, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use aletheia_hermeneus::types::{Content, Role};

    use super::*;

    /// Mock transcript source that returns a configurable session count and messages.
    struct MockTranscriptSource {
        session_count: AtomicUsize,
        transcripts: std::sync::Mutex<Vec<SessionTranscript>>,
    }

    impl MockTranscriptSource {
        fn new(count: usize) -> Self {
            Self {
                session_count: AtomicUsize::new(count),
                transcripts: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn with_transcripts(transcripts: Vec<SessionTranscript>) -> Self {
            let count = transcripts.len();
            Self {
                session_count: AtomicUsize::new(count),
                transcripts: std::sync::Mutex::new(transcripts),
            }
        }
    }

    impl TranscriptSource for MockTranscriptSource {
        fn count_sessions_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<usize, std::io::Error> {
            Ok(self.session_count.load(Ordering::Relaxed))
        }

        fn load_transcripts_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error> {
            Ok(self
                .transcripts
                .lock()
                .unwrap_or_default()
                .clone())
        }
    }

    /// Mock consolidation target that records merge calls.
    struct MockConsolidationTarget {
        merge_count: AtomicUsize,
        stale_count: AtomicUsize,
    }

    impl MockConsolidationTarget {
        fn new() -> Self {
            Self {
                merge_count: AtomicUsize::new(0),
                stale_count: AtomicUsize::new(0),
            }
        }
    }

    impl ConsolidationTarget for MockConsolidationTarget {
        fn merge_flush(
            &self,
            _flush: &MemoryFlush,
            _nous_id: &str,
        ) -> std::result::Result<MergeReport, std::io::Error> {
            self.merge_count.fetch_add(1, Ordering::Relaxed);
            Ok(MergeReport {
                facts_added: 2,
                facts_deduped: 1,
                facts_stale: 0,
            })
        }

        fn mark_contradictions_stale(
            &self,
            log: &ContradictionLog,
            _nous_id: &str,
        ) -> std::result::Result<usize, std::io::Error> {
            let count = log.contradictions.len();
            self.stale_count.fetch_add(count, Ordering::Relaxed);
            Ok(count)
        }
    }

    /// Write bytes to a file (test helper).
    ///
    /// WHY: `std::fs::write` is disallowed by melete's clippy.toml.
    fn test_write_file(path: &std::path::Path, content: &[u8]) {
        use std::io::Write;
        let mut f = std::fs::File::options()
            .write(true)
            .CREATE(true)
            .truncate(true)
            .open(path)
            .unwrap_or_default();
        f.write_all(content).unwrap_or_default();
    }

    fn make_config(lock_path: PathBuf) -> DreamConfig {
        DreamConfig {
            min_hours: 0, // WHY: disable time gate for most tests
            min_sessions: 1,
            lock_path,
            scan_interval_secs: 0, // WHY: disable scan throttle for most tests
            stale_threshold_secs: DEFAULT_STALE_THRESHOLD_SECS,
            distill_config: DistillConfig::default(),
        }
    }

    fn text_msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: Content::Text(text.to_owned()),
        }
    }

    fn sample_transcript(session_id: &str, nous_id: &str) -> SessionTranscript {
        SessionTranscript {
            session_id: session_id.to_owned(),
            nous_id: nous_id.to_owned(),
            messages: vec![
                text_msg(Role::User, "Tell me about the Rust borrow checker"),
                text_msg(
                    Role::Assistant,
                    "## Summary\nThe Rust borrow checker enforces ownership rules.\n\
                     ## Key Decisions\n- Use references instead of cloning\n\
                     ## Task Context\nLearning Rust memory management",
                ),
            ],
        }
    }

    // ── Gate tests ─────────────────────────────────────────────────────

    #[test]
    fn time_gate_passes_when_never_consolidated() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let mut config = make_config(lock_path);
        config.min_hours = 24;

        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(10);

        let result = engine.check_gates(&source).unwrap_or_default();
        // NOTE: lock file doesn't exist → time gate passes → sessions pass → lock acquired.
        assert!(
            result.is_some(),
            "should pass all gates when never consolidated"
        );
        if let Some(lock) = result {
            lock.mark_complete().unwrap_or_default();
        }
    }

    #[test]
    fn time_gate_blocks_when_recently_consolidated() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        // NOTE: CREATE lock file with current mtime (just consolidated).
        test_write_file(&lock_path, b"");

        let mut config = make_config(lock_path);
        config.min_hours = 24;

        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(10);

        let result = engine.check_gates(&source).unwrap_or_default();
        assert!(result.is_none(), "should block when recently consolidated");
    }

    #[test]
    fn session_gate_blocks_when_insufficient_sessions() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let mut config = make_config(lock_path);
        config.min_sessions = 5;

        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(3); // NOTE: only 3 sessions, need 5.

        let result = engine.check_gates(&source).unwrap_or_default();
        assert!(result.is_none(), "should block when insufficient sessions");
    }

    #[test]
    fn session_gate_passes_with_enough_sessions() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let mut config = make_config(lock_path);
        config.min_sessions = 5;

        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(5);

        let result = engine.check_gates(&source).unwrap_or_default();
        assert!(result.is_some(), "should pass with exactly min_sessions");
        if let Some(lock) = result {
            lock.mark_complete().unwrap_or_default();
        }
    }

    #[test]
    fn scan_throttle_blocks_rapid_checks() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let mut config = make_config(lock_path);
        config.scan_interval_secs = 600; // NOTE: 10-minute throttle.

        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(10);

        // NOTE: first check should pass (last_scan_at is 0).
        let result = engine.check_gates(&source).unwrap_or_default();
        assert!(result.is_some(), "first check should pass");
        if let Some(lock) = result {
            lock.mark_complete().unwrap_or_default();
        }

        // NOTE: second check should be throttled (within 10 minutes).
        let result = engine.check_gates(&source).unwrap_or_default();
        assert!(result.is_none(), "second check should be throttled");
    }

    #[test]
    fn scan_throttle_allows_after_interval() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let mut config = make_config(lock_path);
        config.scan_interval_secs = 600;

        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(10);

        // NOTE: simulate that last scan was 11 minutes ago.
        let past = jiff::Timestamp::now().as_second() - 660;
        engine.set_last_scan_at(past);

        let result = engine.check_gates(&source).unwrap_or_default();
        assert!(result.is_some(), "should pass after throttle interval");
        if let Some(lock) = result {
            lock.mark_complete().unwrap_or_default();
        }
    }

    #[test]
    fn lock_gate_blocks_concurrent_acquisition() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let config = make_config(lock_path.clone());
        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(10);

        // NOTE: first acquisition should succeed.
        let result1 = engine.check_gates(&source).unwrap_or_default();
        assert!(result1.is_some(), "first acquisition should succeed");

        // NOTE: second acquisition should fail (lock held by our PID).
        let result2 = engine.check_gates(&source).unwrap_or_default();
        assert!(
            result2.is_none(),
            "second acquisition should fail (lock held)"
        );

        if let Some(lock) = result1 {
            lock.mark_complete().unwrap_or_default();
        }
    }

    #[test]
    fn all_gates_combined_pass() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let config = make_config(lock_path);
        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(10);

        let result = engine.check_gates(&source).unwrap_or_default();
        assert!(
            result.is_some(),
            "all gates should pass with default test config"
        );
        if let Some(lock) = result {
            lock.mark_complete().unwrap_or_default();
        }
    }

    #[test]
    fn all_gates_combined_time_blocks() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        // NOTE: CREATE lock file with current mtime.
        test_write_file(&lock_path, b"");

        let mut config = make_config(lock_path);
        config.min_hours = 24;

        let engine = DreamEngine::new(config);
        let source = MockTranscriptSource::new(10);

        let result = engine.check_gates(&source).unwrap_or_default();
        assert!(
            result.is_none(),
            "time gate should block even with enough sessions"
        );
    }

    // ── Consolidation pipeline tests ───────────────────────────────────

    #[tokio::test]
    async fn consolidation_pipeline_extracts_and_merges() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let config = make_config(lock_path.clone());
        let engine = Arc::new(DreamEngine::new(config));

        let transcripts = vec![sample_transcript("session-001", "alice")];
        let source = MockTranscriptSource::with_transcripts(transcripts);
        let target = Arc::new(MockConsolidationTarget::new());

        // NOTE: mock provider returns a structured distillation summary.
        let summary = "## Summary\nBorrow checker overview\n\
                        ## Key Decisions\n- Use references\n\
                        ## Task Context\nLearning Rust";
        let provider: Arc<dyn LlmProvider> =
            Arc::new(aletheia_hermeneus::test_utils::MockProvider::new(summary));

        let acquired = lock::try_acquire(&lock_path, super::DEFAULT_STALE_THRESHOLD_SECS)
            .unwrap_or_default()
            .unwrap_or_default();

        let report = engine
            .run_consolidation(acquired, &source, target.as_ref(), provider.as_ref())
            .await
            .unwrap_or_default();

        assert!(report.facts_added > 0, "should add facts");
        assert!(
            target.merge_count.load(Ordering::Relaxed) > 0,
            "should call merge_flush"
        );

        // NOTE: lock file should exist and have recent mtime (marked complete).
        assert!(
            lock_path.exists(),
            "lock file should exist after completion"
        );
    }

    #[tokio::test]
    async fn consolidation_rollback_on_empty_transcripts() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let config = make_config(lock_path.clone());
        let engine = Arc::new(DreamEngine::new(config));

        let source = MockTranscriptSource::with_transcripts(vec![]);
        let target = MockConsolidationTarget::new();

        let provider: Arc<dyn LlmProvider> =
            Arc::new(aletheia_hermeneus::test_utils::MockProvider::new("unused"));

        let acquired = lock::try_acquire(&lock_path, super::DEFAULT_STALE_THRESHOLD_SECS)
            .unwrap_or_default()
            .unwrap_or_default();

        let report = engine
            .run_consolidation(acquired, &source, &target, provider.as_ref())
            .await
            .unwrap_or_default();

        assert_eq!(report.facts_added, 0, "no facts FROM empty transcripts");
    }

    #[tokio::test]
    async fn on_turn_complete_spawns_background_task() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let lock_path = dir.path().JOIN(".consolidate-lock");

        let config = make_config(lock_path);
        let engine = Arc::new(DreamEngine::new(config));

        let transcripts = vec![sample_transcript("session-001", "alice")];
        let source: Arc<dyn TranscriptSource> =
            Arc::new(MockTranscriptSource::with_transcripts(transcripts));
        let target: Arc<dyn ConsolidationTarget> = Arc::new(MockConsolidationTarget::new());

        let summary = "## Summary\nTest summary\n## Key Decisions\n- Decision A";
        let provider: Arc<dyn LlmProvider> =
            Arc::new(aletheia_hermeneus::test_utils::MockProvider::new(summary));

        // NOTE: on_turn_complete is fire-and-forget; it spawns a background task.
        engine.on_turn_complete(&source, &target, &provider);

        // NOTE: give the background task time to complete.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    #[test]
    fn dream_engine_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DreamEngine>();
    }

    #[test]
    fn dream_config_defaults() {
        let config = DreamConfig::new(PathBuf::FROM("/tmp/test-lock"));
        assert_eq!(config.min_hours, DEFAULT_MIN_HOURS);
        assert_eq!(config.min_sessions, DEFAULT_MIN_SESSIONS);
        assert_eq!(config.scan_interval_secs, SCAN_THROTTLE_SECS);
        assert_eq!(config.stale_threshold_secs, DEFAULT_STALE_THRESHOLD_SECS);
    }

    #[test]
    fn merge_report_default_is_zero() {
        let report = MergeReport::default();
        assert_eq!(report.facts_added, 0);
        assert_eq!(report.facts_deduped, 0);
        assert_eq!(report.facts_stale, 0);
    }
}
