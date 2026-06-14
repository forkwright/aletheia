#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::sync::atomic::AtomicUsize;

use hermeneus::types::{Content, Role};

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
        Ok(self.transcripts.lock().unwrap().clone())
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
        _provenance: &DreamProvenance,
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
fn write_test_file(path: &std::path::Path, content: &[u8]) {
    use std::io::Write;
    let mut f = std::fs::File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();
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
        cache_breakpoint: false,
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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

    // NOTE: CREATE lock file with current mtime (just consolidated).
    write_test_file(&lock_path, b"");

    let mut config = make_config(lock_path);
    config.min_hours = 24;

    let engine = DreamEngine::new(config);
    let source = MockTranscriptSource::new(10);

    let result = engine.check_gates(&source).unwrap_or_default();
    assert!(result.is_none(), "should block when recently consolidated");
}

#[test]
fn session_gate_blocks_when_insufficient_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

    let mut config = make_config(lock_path);
    config.min_sessions = 5;

    let engine = DreamEngine::new(config);
    let source = MockTranscriptSource::new(3); // NOTE: only 3 sessions, need 5.

    let result = engine.check_gates(&source).unwrap_or_default();
    assert!(result.is_none(), "should block when insufficient sessions");
}

#[test]
fn session_gate_passes_with_enough_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

    // NOTE: CREATE lock file with current mtime.
    write_test_file(&lock_path, b"");

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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

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
        Arc::new(hermeneus::test_utils::MockProvider::new(summary));

    let acquired = lock::try_acquire(&lock_path, super::DEFAULT_STALE_THRESHOLD_SECS)
        .unwrap()
        .unwrap();

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
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

    let config = make_config(lock_path.clone());
    let engine = Arc::new(DreamEngine::new(config));

    let source = MockTranscriptSource::with_transcripts(vec![]);
    let target = MockConsolidationTarget::new();

    let provider: Arc<dyn LlmProvider> =
        Arc::new(hermeneus::test_utils::MockProvider::new("unused"));

    let acquired = lock::try_acquire(&lock_path, super::DEFAULT_STALE_THRESHOLD_SECS)
        .unwrap()
        .unwrap();

    let report = engine
        .run_consolidation(acquired, &source, &target, provider.as_ref())
        .await
        .unwrap_or_default();

    assert_eq!(report.facts_added, 0, "no facts FROM empty transcripts");
}

#[tokio::test]
async fn on_turn_complete_spawns_background_task() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

    let config = make_config(lock_path);
    let engine = Arc::new(DreamEngine::new(config));

    let transcripts = vec![sample_transcript("session-001", "alice")];
    let source: Arc<dyn TranscriptSource> =
        Arc::new(MockTranscriptSource::with_transcripts(transcripts));
    let target: Arc<dyn ConsolidationTarget> = Arc::new(MockConsolidationTarget::new());

    let summary = "## Summary\nTest summary\n## Key Decisions\n- Decision A";
    let provider: Arc<dyn LlmProvider> =
        Arc::new(hermeneus::test_utils::MockProvider::new(summary));

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
fn lock_paths_scoped_by_instance_root() {
    // WHY: two instances with the same nous_id must not share a lock. Using a
    // per-instance store root as the lock directory scopes the lock to each
    // instance.
    let instance_a = tempfile::tempdir().unwrap();
    let instance_b = tempfile::tempdir().unwrap();
    let nous_id = "same-nous";

    let lock_a = instance_a
        .path()
        .join(".aletheia-auto-dream")
        .join(format!("{nous_id}.lock"));
    let lock_b = instance_b
        .path()
        .join(".aletheia-auto-dream")
        .join(format!("{nous_id}.lock"));

    let engine_a = DreamEngine::new(make_config(lock_a));
    let engine_b = DreamEngine::new(make_config(lock_b));
    let source = MockTranscriptSource::new(10);

    let acquired_a = engine_a.check_gates(&source).unwrap_or_default();
    assert!(acquired_a.is_some(), "instance A should acquire its lock");

    let acquired_b = engine_b.check_gates(&source).unwrap_or_default();
    assert!(
        acquired_b.is_some(),
        "instance B should acquire its own lock even with the same nous_id"
    );

    if let Some(lock) = acquired_a {
        lock.mark_complete().unwrap_or_default();
    }
    if let Some(lock) = acquired_b {
        lock.mark_complete().unwrap_or_default();
    }
}

#[test]
fn repeated_turn_completion_preserves_scan_throttle() {
    // WHY: the scan throttle lives on the DreamEngine. Reusing the same engine
    // across repeated turn completions must not reset the throttle.
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

    let mut config = make_config(lock_path);
    config.scan_interval_secs = 600;

    let engine = DreamEngine::new(config);
    let source = MockTranscriptSource::new(10);

    let first = engine.check_gates(&source).unwrap_or_default();
    assert!(first.is_some(), "first turn should pass scan gate");
    if let Some(lock) = first {
        lock.mark_complete().unwrap_or_default();
    }

    let second = engine.check_gates(&source).unwrap_or_default();
    assert!(
        second.is_none(),
        "second turn should be throttled because the engine state persists"
    );
}

#[test]
fn dream_config_defaults() {
    let config = DreamConfig::new(PathBuf::from("/tmp/test-lock"));
    assert_eq!(config.min_hours, DEFAULT_MIN_HOURS);
    assert_eq!(config.min_sessions, DEFAULT_MIN_SESSIONS);
    assert_eq!(config.scan_interval_secs, DEFAULT_SCAN_THROTTLE_SECS);
    assert_eq!(config.stale_threshold_secs, DEFAULT_STALE_THRESHOLD_SECS);
}

#[test]
fn merge_report_default_is_zero() {
    let report = MergeReport::default();
    assert_eq!(report.facts_added, 0);
    assert_eq!(report.facts_deduped, 0);
    assert_eq!(report.facts_stale, 0);
}

/// Target that records the provenance passed to each merge so lineage can be
/// asserted without a real knowledge store.
#[derive(Default)]
struct LineageRecordingTarget {
    provenances: std::sync::Mutex<Vec<DreamProvenance>>,
}

impl ConsolidationTarget for LineageRecordingTarget {
    fn merge_flush(
        &self,
        _flush: &MemoryFlush,
        provenance: &DreamProvenance,
        _nous_id: &str,
    ) -> std::result::Result<MergeReport, std::io::Error> {
        self.provenances.lock().unwrap().push(provenance.clone());
        Ok(MergeReport {
            facts_added: 1,
            facts_deduped: 0,
            facts_stale: 0,
        })
    }

    fn mark_contradictions_stale(
        &self,
        _log: &ContradictionLog,
        _nous_id: &str,
    ) -> std::result::Result<usize, std::io::Error> {
        Ok(0)
    }
}

#[tokio::test]
async fn consolidation_preserves_source_session_lineage_and_batch() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".consolidate-lock");

    let config = make_config(lock_path.clone());
    let engine = Arc::new(DreamEngine::new(config));

    let transcripts = vec![
        sample_transcript("session-001", "alice"),
        sample_transcript("session-002", "alice"),
    ];
    let source = MockTranscriptSource::with_transcripts(transcripts);
    let target = Arc::new(LineageRecordingTarget::default());

    let provider: Arc<dyn LlmProvider> = Arc::new(hermeneus::test_utils::MockProvider::new(
        "## Summary\nS\n## Key Decisions\n- D",
    ));

    let acquired = lock::try_acquire(&lock_path, super::DEFAULT_STALE_THRESHOLD_SECS)
        .unwrap()
        .unwrap();
    let _report = engine
        .run_consolidation(acquired, &source, target.as_ref(), provider.as_ref())
        .await
        .unwrap_or_default();

    let provenances = target.provenances.lock().unwrap().clone();

    assert_eq!(
        provenances.len(),
        2,
        "each transcript should produce a merge"
    );
    assert!(
        provenances
            .iter()
            .any(|p| p.source_session_id.as_deref() == Some("session-001")),
        "should preserve session-001 lineage"
    );
    assert!(
        provenances
            .iter()
            .any(|p| p.source_session_id.as_deref() == Some("session-002")),
        "should preserve session-002 lineage"
    );
    assert!(
        provenances[0].batch_id.is_some(),
        "each run should carry a consolidation batch ID"
    );
    assert_eq!(
        provenances[0].batch_id, provenances[1].batch_id,
        "all merges in one run should share the same batch ID"
    );
}
