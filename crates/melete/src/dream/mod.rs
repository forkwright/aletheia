//! Auto-dream memory consolidation.
//!
//! Background process that periodically consolidates session transcripts INTO
//! the knowledge graph. Uses a triple-gate system (time → sessions → lock) to
//! so consolidation runs infrequently, only when meaningful new data exists,
//! and never concurrently.
//!
//! Gate ORDER matters: each subsequent gate is more expensive.
//! 1. **Time gate**  -  single stat call on the lock file.
//! 2. **Session gate**  -  directory scan, throttled to 10-minute intervals.
//! 3. **Lock gate**  -  rustix flock write + PID verify.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

use snafu::ResultExt;
use tracing::{Instrument as _, instrument};

use hermeneus::provider::LlmProvider;
use hermeneus::types::Message;

use crate::contradiction::ContradictionLog;
use crate::distill::{DistillConfig, DistillEngine};
use crate::error::{
    DreamConsolidationTargetSnafu, DreamTranscriptSourceSnafu, ProbeVerificationSnafu, Result,
};
use crate::flush::MemoryFlush;
use crate::probe::{ProbeConfig, ProbeVerifier};

pub(crate) mod lock;

/// Default minimum hours between consolidation runs.
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::dream_min_hours`.
pub const DEFAULT_MIN_HOURS: u64 = 24;

/// Default minimum sessions required to trigger consolidation.
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::dream_min_sessions`.
pub const DEFAULT_MIN_SESSIONS: usize = 5;

/// Default session scan throttle interval (10 minutes in seconds).
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::dream_scan_throttle_secs`.
pub const DEFAULT_SCAN_THROTTLE_SECS: i64 = 600;

/// Default stale lock threshold (1 hour in seconds).
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::dream_stale_threshold_secs`.
pub const DEFAULT_STALE_THRESHOLD_SECS: i64 = 3_600;

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
            scan_interval_secs: DEFAULT_SCAN_THROTTLE_SECS,
            stale_threshold_secs: DEFAULT_STALE_THRESHOLD_SECS,
            distill_config: DistillConfig::default(),
        }
    }
}

/// A session transcript available for consolidation.
#[derive(Debug, Clone)]
pub struct SessionTranscript {
    /// Session identifier.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: koina::SessionId is UUID-backed; migration to newtype tracked separately to avoid breaking callers
    /// Nous (agent) identifier.
    pub nous_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: koina::NousId newtype available; migration tracked separately to avoid breaking callers
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
/// Implementors provide access to session storage (e.g. fjall via graphe).
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
        transcript: &SessionTranscript,
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
    probe: ProbeVerifier,
    /// Unix timestamp of the last session scan (for 10-minute throttle).
    /// WHY: `AtomicI64` because we need lock-free reads FROM the gate check
    /// hot path. i64 holds Unix seconds until year ~292 billion.
    last_scan_at: AtomicI64,
    /// Handle to the in-flight background consolidation task, if any.
    ///
    /// WHY(#5734): storing the handle lets us observe panics and await
    /// completion at shutdown instead of dropping it and losing the result.
    active_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
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
            probe: ProbeVerifier::new(ProbeConfig::default()),
            last_scan_at: AtomicI64::new(0),
            active_task: Mutex::new(None),
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
                    let min_secs_i64 = i64::try_from(min_secs).unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: negative min_secs is pathological; 0 seconds means no minimum wait
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

        // GATE 3: Lock gate (rustix flock + PID verify).
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

        // WHY(#5734): keep the JoinHandle so panics surface at shutdown/join
        // instead of being silently discarded by Tokio's default panic hook.
        let handle = tokio::spawn(
            async move {
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
                        let duration_ms = start.elapsed().as_millis().min(u64::MAX.into());
                        let duration_ms = u64::try_from(duration_ms).unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: duration_ms is from min(as_millis, u64::MAX); the try_from never fails here
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
            }
            .instrument(tracing::info_span!("auto_dream_consolidation")),
        );

        let mut guard = self
            .active_task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // WHY(#5679): the lock gate prevents actual concurrent runs, but a
        // still-running task at the next turn is a shutdown-risk signal.
        if let Some(ref previous) = *guard
            && !previous.is_finished()
        {
            tracing::warn!(
                "auto-dream consolidation still in progress; next turn will re-evaluate gates"
            );
        }
        *guard = Some(handle);
    }

    /// Wait for any in-flight consolidation task to finish.
    ///
    /// Call this from the shutdown path to avoid aborting a consolidation
    /// mid-flight, which could leave the lock mtime stale and cause the next
    /// run to re-process already-merged sessions.
    ///
    /// # Errors
    ///
    /// Returns the task's [`JoinError`] if the consolidation task panicked or
    /// was aborted.
    pub async fn shutdown(&self) -> std::result::Result<(), tokio::task::JoinError> {
        let handle = self
            .active_task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            handle.await?;
        }
        Ok(())
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
                    acquired.rollback().unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: best-effort cleanup; failure means lock state is already invalid
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

            if let Err(e) = self.verify_flush_grounding(&result.memory_flush, &transcript.messages)
            {
                acquired.rollback().unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default WHY: best-effort cleanup; failure means lock state is already invalid
                return Err(e);
            }

            // NOTE: merge extracted facts INTO the knowledge graph.
            match target
                .merge_flush(&result.memory_flush, transcript)
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

    fn verify_flush_grounding(&self, flush: &MemoryFlush, messages: &[Message]) -> Result<()> {
        let transcript_text = format_probe_transcript(messages);
        let probe_report = self.probe.verify(flush, &transcript_text);
        if probe_report.all_passed() {
            return Ok(());
        }
        ProbeVerificationSnafu {
            failure_count: probe_report.failure_count(),
            total_probes: probe_report.total_probes(),
        }
        .fail()
    }
}

fn format_probe_transcript(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|message| match &message.content {
            hermeneus::types::Content::Text(text) => text.clone(),
            hermeneus::types::Content::Blocks(blocks) => blocks
                .iter()
                .filter_map(|block| match block {
                    hermeneus::types::ContentBlock::Text { text, .. } => Some(text.clone()),
                    hermeneus::types::ContentBlock::ToolUse { name, input, .. } => {
                        Some(format!("{name} {input}"))
                    }
                    hermeneus::types::ContentBlock::ToolResult { content, .. } => {
                        Some(content.text_summary())
                    }
                    hermeneus::types::ContentBlock::Thinking { thinking, .. } => {
                        Some(thinking.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        })
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod dream_tests;
