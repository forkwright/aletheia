// kanon:ignore RUST/file-too-long — background task orchestration; split planned with #3747
//! Background tasks: extraction, skill analysis, distillation, and task reaping.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(feature = "knowledge-store")]
use hermeneus::provider::LlmProvider;
use hermeneus::provider::ProviderRegistry;
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::KnowledgeStore;
use mneme::store::SessionStore;
use tokio::sync::Mutex;
use tracing::{Instrument, debug, info, warn};

use super::{MAX_SPAWNED_TASKS, NousActor};

/// Drop guard that clears the distillation-in-progress flag on drop.
/// Prevents the flag from being stuck if the background task panics.
struct DistillationFlagGuard(Arc<AtomicBool>);

impl Drop for DistillationFlagGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl NousActor {
    /// Reap completed background tasks, log failures, count panics/failures, record metrics.
    pub(super) fn reap_background_tasks(&mut self) {
        while let Some(result) = self.runtime.background_tasks.try_join_next() {
            match result {
                // NOTE: task completed successfully, no action needed
                Ok(()) => {}
                Err(e) => {
                    if e.is_panic() {
                        // WHY: Background tasks run outside the main panic boundary.
                        // They are logged and counted as background failures but do NOT
                        // trigger the pipeline `Degraded` lifecycle.
                        let message = e.to_string();
                        self.record_background_panic(Some(message));
                        crate::metrics::record_background_failure(&self.id, "panic");
                    } else {
                        let message = e.to_string();
                        warn!(nous_id = %self.id, error = %message, "background task failed");
                        self.record_background_failure("error", Some(message));
                        crate::metrics::record_background_failure(&self.id, "error");
                    }
                }
            }
        }
    }

    /// Record a background task panic occurrence. Logs a warning but does NOT trigger pipeline degraded mode.
    pub(super) fn record_background_panic(&mut self, message: Option<String>) {
        self.runtime.background_panic_count += 1;
        let now = std::time::Instant::now();
        self.runtime.background_panic_timestamps.push(now);

        // WHY: drop timestamps outside the window so logging/monitoring stay accurate
        let degraded_window = Duration::from_secs(self.nous_behavior.degraded_window_secs);
        let cutoff = std::time::Instant::now()
            .checked_sub(degraded_window)
            .unwrap_or(self.runtime.started_at);
        self.runtime
            .background_panic_timestamps
            .retain(|t| *t > cutoff);

        self.record_background_failure("panic", message);

        warn!(
            nous_id = %self.id,
            background_panic_count = self.runtime.background_panic_count,
            recent_background_panics = self.runtime.background_panic_timestamps.len(),
            "background task panicked"
        );
    }

    /// Record a generic background task failure (panic or non-panic join error).
    ///
    /// Updates the total/recent counters and latest message/kind exposed in
    /// `NousStatus` and `ActorHealth`. Does NOT change the pipeline lifecycle.
    pub(super) fn record_background_failure(&mut self, kind: &str, message: Option<String>) {
        self.runtime.background_failure.total_count += 1;
        let now = std::time::Instant::now();
        self.runtime.background_failure.timestamps.push(now);

        // WHY(#5147): keep only failures inside the configured degraded window so
        // `background_health_degraded` reflects recent flapping, not ancient history.
        let degraded_window = Duration::from_secs(self.nous_behavior.degraded_window_secs);
        let cutoff = std::time::Instant::now()
            .checked_sub(degraded_window)
            .unwrap_or(self.runtime.started_at);
        self.runtime
            .background_failure
            .timestamps
            .retain(|t| *t > cutoff);

        self.runtime.background_failure.latest_kind = Some(kind.to_owned());
        self.runtime.background_failure.latest_message = message;
    }

    pub(super) fn maybe_spawn_extraction(
        &mut self,
        user_content: &str,
        assistant_content: &str,
        tool_calls: &[crate::pipeline::ToolCall],
        reasoning: &str,
    ) {
        let Some(ref extraction_config) = self.pipeline_config.extraction else {
            return;
        };
        if !extraction_config.enabled {
            return;
        }

        let content_len = user_content.len() + assistant_content.len();
        if content_len < extraction_config.min_message_length {
            return;
        }

        #[cfg(feature = "gliner")]
        let mut config = extraction_config.clone();
        #[cfg(not(feature = "gliner"))]
        let mut config = extraction_config.clone();
        if config.project_id.is_none() {
            config
                .project_id
                .clone_from(&self.pipeline_config.project_id);
        }
        config.provider = match self.config.behavior.knowledge_extraction_provider {
            taxis::config::BookkeepingProviderKind::Llm => {
                mneme::extract::BookkeepingProviderKind::Llm
            }
            taxis::config::BookkeepingProviderKind::Gliner => {
                mneme::extract::BookkeepingProviderKind::Gliner
            }
            _ => mneme::extract::BookkeepingProviderKind::Llm,
        };
        #[cfg(not(feature = "gliner"))]
        if matches!(
            config.provider,
            mneme::extract::BookkeepingProviderKind::Gliner
        ) {
            warn!(
                nous_id = %self.id,
                "GLiNER extraction provider requested but nous/gliner is disabled; falling back to LLM"
            );
            config.provider = mneme::extract::BookkeepingProviderKind::Llm;
        }
        let providers = Arc::clone(&self.services.providers);
        let nous_id = self.id.clone();
        let user = user_content.to_owned();
        let assistant = assistant_content.to_owned();
        let tool_calls: Vec<crate::pipeline::ToolCall> = tool_calls.to_vec();
        let reasoning = reasoning.to_owned();
        let span = tracing::info_span!("extraction", nous.id = %nous_id);
        #[cfg(feature = "knowledge-store")]
        let knowledge_store = self.stores.knowledge_store.clone();
        let cross_tx = self.channel.cross_tx.clone();

        if self.runtime.background_tasks.len() >= MAX_SPAWNED_TASKS {
            warn!(nous_id = %self.id, limit = MAX_SPAWNED_TASKS, current = self.runtime.background_tasks.len(), task_type = "extraction", "background task limit reached, skipping");
            return;
        }

        let cancel = self.channel.cancel.child_token();
        self.runtime.background_tasks.spawn(
            async move {
                // WHY: background tasks respect the actor's cancellation token so
                // they exit promptly during shutdown instead of running until the
                // drain timeout expires. (#3256)
                tokio::select! {
                    () = cancel.cancelled() => {
                        info!(nous_id = %nous_id, task_type = "extraction", "background task cancelled during shutdown");
                    }
                    () = run_extraction(
                        &config,
                        providers,
                        &nous_id,
                        &user,
                        &assistant,
                        &tool_calls,
                        &reasoning,
                        #[cfg(feature = "knowledge-store")]
                        knowledge_store.as_ref(),
                        cross_tx,
                    ) => {}
                }
            }
            .instrument(span),
        );
    }

    /// Analyze tool calls from the completed turn for skill auto-capture.
    ///
    /// Converts the turn's tool calls to mneme's [`ToolCallRecord`] format,
    /// runs them through the heuristic filter and candidate tracker, and
    /// spawns LLM extraction if a candidate is promoted (Rule of Three).
    pub(super) fn maybe_spawn_skill_analysis(
        &mut self,
        tool_calls: &[crate::pipeline::ToolCall],
        source_session_id: &str,
    ) {
        use mneme::skills::{ToolCallRecord, TrackResult};

        if tool_calls.is_empty() {
            return;
        }

        let records: Vec<ToolCallRecord> = tool_calls
            .iter()
            .map(|tc| {
                let record = if tc.is_error {
                    ToolCallRecord::errored(&tc.name, tc.duration_ms)
                } else {
                    ToolCallRecord::new(&tc.name, tc.duration_ms)
                };
                record.with_evidence(
                    &tc.id,
                    &tc.input,
                    tc.result.as_deref(),
                    tc.receipt.as_deref(),
                )
            })
            .collect();

        let nous_id = self.id.clone();
        let result =
            self.services
                .candidate_tracker
                .track_sequence(&records, source_session_id, &nous_id);

        match result {
            TrackResult::Rejected => {
                debug!("skill analysis: sequence rejected by heuristic filter");
            }
            TrackResult::New => {
                info!("skill analysis: new candidate tracked");
            }
            TrackResult::Tracked(count) => {
                info!(count, "skill analysis: candidate recurrence updated");
            }
            TrackResult::Promoted(candidate_id) => {
                info!(candidate_id = %candidate_id, "skill analysis: candidate promoted, spawning extraction");
                self.spawn_skill_extraction(
                    &candidate_id,
                    &records,
                    #[cfg(feature = "knowledge-store")]
                    source_session_id,
                );
            }
            _ => {
                // WHY: TrackResult is #[non_exhaustive]; future variants are silently ignored here.
            }
        }
    }

    /// Spawn background LLM extraction for a promoted skill candidate.
    fn spawn_skill_extraction(
        &mut self,
        candidate_id: &str,
        tool_calls: &[mneme::skills::ToolCallRecord],
        #[cfg(feature = "knowledge-store")] source_session_id: &str,
    ) {
        let Some(ref extraction_config) = self.pipeline_config.extraction else {
            return;
        };

        // WHY: extraction model (Haiku) for cost-effectiveness
        let model = extraction_config.model.clone();
        let providers = Arc::clone(&self.services.providers);
        let nous_id = self.id.clone();
        let candidate_id = candidate_id.to_owned();
        let tool_calls = tool_calls.to_vec();
        #[cfg(feature = "knowledge-store")]
        let source_session_id = source_session_id.to_owned();
        let tracker = Arc::clone(&self.services.candidate_tracker);
        #[cfg(feature = "knowledge-store")]
        let knowledge_store = self.stores.knowledge_store.clone();
        let span = tracing::info_span!("skill_extraction", nous.id = %nous_id, candidate.id = %candidate_id);

        if self.runtime.background_tasks.len() >= MAX_SPAWNED_TASKS {
            warn!(nous_id = %self.id, limit = MAX_SPAWNED_TASKS, current = self.runtime.background_tasks.len(), task_type = "skill_extraction", "background task limit reached, skipping");
            return;
        }

        let cancel = self.channel.cancel.child_token();
        self.runtime.background_tasks.spawn(
            async move {
                tokio::select! {
                    () = cancel.cancelled() => {
                        info!(nous_id = %nous_id, task_type = "skill_extraction", "background task cancelled during shutdown");
                    }
                    () = run_skill_extraction(
                        &model,
                        providers,
                        &nous_id,
                        &candidate_id,
                        &tool_calls,
                        #[cfg(feature = "knowledge-store")]
                        &source_session_id,
                        &tracker,
                        #[cfg(feature = "knowledge-store")]
                        knowledge_store.as_ref(),
                    ) => {}
                }
            }
            .instrument(span),
        );
    }

    pub(super) async fn maybe_spawn_distillation(&mut self, session_key: &str) {
        // WHY: two turns finishing close together can both observe the distillation trigger before
        // either task commits: the atomic flag ensures only one distillation task runs at a time (#1035)
        if self
            .runtime
            .distillation_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            debug!(nous_id = %self.id, "distillation already in progress, skipping");
            return;
        }

        let did_spawn = self.try_spawn_distillation(session_key).await;

        // NOTE: clear immediately if no task was spawned; spawned task clears the flag itself on completion
        if !did_spawn {
            self.runtime
                .distillation_in_progress
                .store(false, Ordering::Release);
        }
    }

    /// Attempt to spawn a distillation task. Returns `true` if a task was spawned.
    async fn try_spawn_distillation(&mut self, session_key: &str) -> bool {
        let Some(ref store_arc) = self.stores.session_store else {
            return false;
        };
        let Some(session_state) = self.sessions.get(session_key) else {
            return false;
        };
        let session_id = session_state.id.clone();

        // WHY: trigger check under lock, guard dropped before spawn to avoid holding lock across .await
        let should_distill = {
            let store = store_arc.lock().await;
            let Ok(Some(session)) = store.find_session_by_id(&session_id) else {
                return false;
            };
            let config = crate::distillation::DistillTriggerConfig::default();
            crate::distillation::should_trigger_distillation(
                &session,
                u64::from(self.config.generation.context_window),
                &config,
            )
            .is_some()
        };

        if !should_distill {
            return false;
        }

        let config = crate::distillation::DistillTriggerConfig::default();
        if self
            .services
            .providers
            .find_provider(&config.model)
            .is_none()
        {
            warn!(model = %config.model, "no provider for distillation model");
            return false;
        }

        let store = Arc::clone(store_arc);
        let providers = Arc::clone(&self.services.providers);
        #[cfg(feature = "knowledge-store")]
        let knowledge_store = self.stores.knowledge_store.as_ref().map(Arc::clone);
        let project_id = self.pipeline_config.project_id.clone();
        let nous_id = self.id.clone();
        let span =
            tracing::info_span!("distillation", nous.id = %nous_id, session.id = %session_id);

        if self.runtime.background_tasks.len() >= MAX_SPAWNED_TASKS {
            warn!(nous_id = %self.id, limit = MAX_SPAWNED_TASKS, current = self.runtime.background_tasks.len(), task_type = "distillation", "background task limit reached, skipping");
            return false;
        }

        let flag = Arc::clone(&self.runtime.distillation_in_progress);
        let cancel = self.channel.cancel.child_token();
        self.runtime.background_tasks.spawn(
            async move {
                // WHY: drop guard ensures the flag is cleared even if the task panics,
                // preventing distillation from being permanently blocked (#2155)
                let _guard = DistillationFlagGuard(Arc::clone(&flag));
                tokio::select! {
                    () = cancel.cancelled() => {
                        info!(nous_id = %nous_id, task_type = "distillation", "background task cancelled during shutdown");
                    }
                    () = run_background_distillation(
                        store,
                        providers,
                        #[cfg(feature = "knowledge-store")]
                        knowledge_store,
                        session_id,
                        nous_id.clone(),
                        config,
                        project_id,
                    ) => {}
                }
                // NOTE: guard Drop handles flag.store(false) for both normal and panic paths
            }
            .instrument(span),
        );
        true
    }

    #[cfg(feature = "knowledge-store")]
    pub(super) async fn maybe_run_auto_dream(&mut self) {
        let (Some(session_store), Some(knowledge_store)) = (
            self.stores.session_store.as_ref(),
            self.stores.knowledge_store.as_ref(),
        ) else {
            return;
        };

        let config = crate::distillation::DistillTriggerConfig::default();
        if self
            .services
            .providers
            .find_provider(&config.model)
            .is_none()
        {
            return;
        }
        let provider: Arc<dyn LlmProvider> = Arc::new(RegistryLlmProvider::new(
            Arc::clone(&self.services.providers),
            config.model.clone(),
        ));

        let lock_dir = std::env::temp_dir().join("aletheia-auto-dream");
        if let Err(e) = std::fs::create_dir_all(&lock_dir) {
            warn!(nous_id = %self.id, error = %e, "auto-dream lock directory unavailable");
            return;
        }

        let mut dream_config = melete::dream::DreamConfig::new(
            lock_dir.join(format!("{}.lock", self.id.replace('/', "_"))),
        );
        dream_config.min_hours = self.config.behavior.dream_min_hours;
        dream_config.min_sessions = self.config.behavior.dream_min_sessions;
        dream_config.scan_interval_secs = self.config.behavior.dream_scan_throttle_secs;
        dream_config.stale_threshold_secs = self.config.behavior.dream_stale_threshold_secs;
        dream_config.distill_config.model = config.model;
        dream_config.distill_config.verbatim_tail = config.verbatim_tail;

        // WHY: keep one engine per actor so `last_scan_at` survives across turns
        // and the 10-minute intra-day throttle is not reset every turn (#5700).
        let engine = self
            .runtime
            .auto_dream_engine
            .get_or_insert_with(|| Arc::new(melete::dream::DreamEngine::new(dream_config)));
        let engine = engine.clone();

        let source: Arc<dyn melete::dream::TranscriptSource> = Arc::new(
            SessionStoreTranscriptSource::new(Arc::clone(session_store), self.id.clone()),
        );
        let target: Arc<dyn melete::dream::ConsolidationTarget> =
            Arc::new(KnowledgeStoreConsolidationTarget::new(
                Arc::clone(knowledge_store),
                self.pipeline_config.project_id.clone(),
            ));
        engine.on_turn_complete(&source, &target, &provider).await;
    }
}

#[cfg(feature = "knowledge-store")]
// kanon:ignore RUST/no-arc-mutex-anti-pattern — tokio::sync::Mutex in sync SessionStore adapter; held only for brief store ops
struct SessionStoreTranscriptSource {
    // kanon:ignore RUST/no-arc-mutex-anti-pattern — tokio::sync::Mutex in sync SessionStore adapter; held only for brief store ops
    store: Arc<Mutex<SessionStore>>,
    /// Nous ID whose sessions are eligible for consolidation.
    ///
    /// WHY: restricts index scans to this actor's partition of the session
    /// store instead of scanning every session row (#5700).
    nous_id: String,
}

#[cfg(feature = "knowledge-store")]
// kanon:ignore RUST/no-arc-mutex-anti-pattern — tokio::sync::Mutex in sync SessionStore adapter; held only for brief store ops
impl SessionStoreTranscriptSource {
    // kanon:ignore RUST/no-arc-mutex-anti-pattern — tokio::sync::Mutex in sync SessionStore adapter; held only for brief store ops
    fn new(store: Arc<Mutex<SessionStore>>, nous_id: String) -> Self {
        Self { store, nous_id }
    }
}

#[cfg(feature = "knowledge-store")]
impl melete::dream::TranscriptSource for SessionStoreTranscriptSource {
    fn count_sessions_since(
        &self,
        since: jiff::Timestamp,
    ) -> std::result::Result<usize, std::io::Error> {
        let store = self.store.try_lock().map_err(|e| {
            std::io::Error::other(format!(
                "session store busy during auto-dream transcript scan: {e}"
            ))
        })?;
        store
            .count_sessions_since(since, &self.nous_id)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn load_transcripts_since(
        &self,
        since: jiff::Timestamp,
    ) -> std::result::Result<Vec<melete::dream::SessionTranscript>, std::io::Error> {
        let store = self.store.try_lock().map_err(|e| {
            std::io::Error::other(format!(
                "session store busy during auto-dream transcript load: {e}"
            ))
        })?;
        let session_ids = store
            .list_session_ids_since(since, &self.nous_id)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        session_ids
            .into_iter()
            .map(|session_id| {
                let history = store
                    .get_history(&session_id, None)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                Ok(melete::dream::SessionTranscript {
                    session_id,
                    nous_id: self.nous_id.clone(),
                    messages: crate::distillation::convert_to_hermeneus_messages(&history),
                })
            })
            .collect()
    }
}

#[cfg(feature = "knowledge-store")]
struct KnowledgeStoreConsolidationTarget {
    store: Arc<KnowledgeStore>,
    project_id: Option<mneme::workspace::ProjectId>,
}

#[cfg(feature = "knowledge-store")]
impl KnowledgeStoreConsolidationTarget {
    fn new(store: Arc<KnowledgeStore>, project_id: Option<mneme::workspace::ProjectId>) -> Self {
        Self { store, project_id }
    }
}

#[cfg(feature = "knowledge-store")]
impl melete::dream::ConsolidationTarget for KnowledgeStoreConsolidationTarget {
    fn merge_flush(
        &self,
        flush: &melete::flush::MemoryFlush,
        nous_id: &str,
    ) -> std::result::Result<melete::dream::MergeReport, std::io::Error> {
        let facts_added = crate::distillation::persist_memory_flush_items(
            &self.store,
            flush,
            "auto-dream",
            nous_id,
            self.project_id.as_ref(),
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(melete::dream::MergeReport {
            facts_added,
            facts_deduped: 0,
            facts_stale: 0,
        })
    }

    fn mark_contradictions_stale(
        &self,
        _log: &melete::contradiction::ContradictionLog,
        _nous_id: &str,
    ) -> std::result::Result<usize, std::io::Error> {
        Err(std::io::Error::other(
            "auto-dream contradiction stale marking is not supported by KnowledgeStore yet",
        ))
    }
}

#[cfg(feature = "knowledge-store")]
struct RegistryLlmProvider {
    registry: Arc<ProviderRegistry>,
    model: String,
}

#[cfg(feature = "knowledge-store")]
impl RegistryLlmProvider {
    fn new(registry: Arc<ProviderRegistry>, model: String) -> Self {
        Self { registry, model }
    }
}

#[cfg(feature = "knowledge-store")]
impl LlmProvider for RegistryLlmProvider {
    fn complete<'a>(
        &'a self,
        request: &'a hermeneus::types::CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = hermeneus::error::Result<hermeneus::types::CompletionResponse>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let Some(provider) = self.registry.find_provider(&request.model) else {
                return Err(hermeneus::error::UnsupportedModelSnafu {
                    model: request.model.clone(),
                }
                .build());
            };
            provider.complete(request).await
        })
    }

    fn supported_models(&self) -> &[&str] {
        &[]
    }

    fn supports_model(&self, model: &str) -> bool {
        model == self.model && self.registry.find_provider(model).is_some()
    }

    fn name(&self) -> &'static str {
        "provider-registry"
    }
}

/// Run extraction as a background task. Logs results, never panics.
#[expect(
    clippy::too_many_arguments,
    reason = "background extraction async runner: config + providers + ids + content + tool_calls + reasoning + optional store + cross_tx"
)]
#[expect(
    clippy::too_many_lines,
    reason = "background extraction pipeline: build prompt, call LLM, parse, conflict detect, persist — sequential by design"
)]
async fn run_extraction(
    config: &mneme::extract::ExtractionConfig,
    providers: Arc<ProviderRegistry>,
    nous_id: &str,
    user_content: &str,
    assistant_content: &str,
    tool_calls: &[crate::pipeline::ToolCall],
    reasoning: &str,
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<&Arc<KnowledgeStore>>,
    cross_tx: Option<tokio::sync::mpsc::Sender<crate::cross::CrossNousEnvelope>>,
) {
    use mneme::extract::{ConversationMessage, ExtractedToolCall, ExtractionEngine};

    #[cfg(not(feature = "knowledge-store"))]
    let _ = cross_tx;

    let engine = ExtractionEngine::new(config.clone());
    let provider = crate::extraction::HermeneusExtractionProvider::new(providers, &config.model);

    let extracted_tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(
            tool_calls
                .iter()
                .map(|tc| ExtractedToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                    result: tc.result.clone(),
                    is_error: tc.is_error,
                })
                .collect(),
        )
    };
    let reasoning = if reasoning.is_empty() {
        None
    } else {
        Some(reasoning.to_owned())
    };

    let messages = vec![
        ConversationMessage {
            role: "user".to_owned(),
            content: user_content.to_owned(),
            tool_calls: None,
            reasoning: None,
        },
        ConversationMessage {
            role: "assistant".to_owned(),
            content: assistant_content.to_owned(),
            tool_calls: extracted_tool_calls,
            reasoning,
        },
    ];

    match engine
        .extract_refined(&messages, &provider, nous_id, "background")
        .await
    {
        Ok(refined) => {
            let entities = refined.extraction.entities.len();
            let relationships = refined.extraction.relationships.len();
            let facts = refined.extraction.facts.len();

            #[cfg(feature = "knowledge-store")]
            if let Some(store) = knowledge_store {
                if config.detect_conflict {
                    for fact in &refined.extraction.facts {
                        match mneme::verification::detect_conflict(fact, store, nous_id) {
                            Ok(Some(conflict)) => {
                                tracing::info!(
                                    nous_id = %nous_id,
                                    existing_fact_id = %conflict.existing,
                                    conflict_kind = ?conflict.kind,
                                    "conflict detected during extraction"
                                );
                                if let Some(ref tx) = cross_tx {
                                    let msg = crate::cross::knowledge::contest_message(
                                        nous_id,
                                        "broadcast",
                                        conflict.existing.clone(),
                                        format!(
                                            "conflict detected during extraction: {:?}",
                                            conflict.kind
                                        ),
                                    );
                                    let envelope = crate::cross::CrossNousEnvelope { message: msg };
                                    if let Err(e) = tx.send(envelope).await {
                                        tracing::warn!(nous_id = %nous_id, error = %e, "failed to emit contest event");
                                    }
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::warn!(nous_id = %nous_id, error = %e, "conflict detection failed");
                            }
                        }
                    }
                }

                match engine.persist_with_scope(
                    &refined.extraction,
                    store,
                    "background",
                    nous_id,
                    Some(mneme::knowledge::MemoryScope::Project),
                ) {
                    Ok(result) => {
                        info!(
                            nous_id = %nous_id,
                            entities_persisted = result.entities_inserted,
                            relationships_persisted = result.relationships_inserted,
                            relationships_skipped = result.relationships_skipped,
                            facts_persisted = result.facts_inserted,
                            "extraction persisted to knowledge store"
                        );
                        mneme::metrics::record_extraction(nous_id, true);
                    }
                    Err(e) => {
                        warn!(nous_id = %nous_id, error = %e, "extraction persist failed");
                        crate::metrics::record_background_failure(nous_id, "extraction_persist");
                        mneme::metrics::record_extraction(nous_id, false);
                    }
                }
            }

            info!(
                nous_id = %nous_id,
                turn_type = %refined.turn_type,
                entities,
                relationships,
                facts,
                facts_filtered = refined.facts_filtered,
                "refined extraction completed"
            );
        }
        Err(e) => {
            warn!(nous_id = %nous_id, error = %e, "extraction failed");
            crate::metrics::record_background_failure(nous_id, "extraction");
            #[cfg(feature = "knowledge-store")]
            mneme::metrics::record_extraction(nous_id, false);
        }
    }
}

// NOTE(#940): 113 lines: extract → persist → handle lifecycle for skill extraction.
// Single cohesive async operation, splitting would obscure the three-phase flow.
#[expect(
    clippy::too_many_lines,
    reason = "three-phase skill extraction pipeline: extract, persist, lifecycle; splitting would obscure the sequential flow"
)]
#[cfg_attr(
    feature = "knowledge-store",
    expect(
        clippy::too_many_arguments,
        reason = "background skill extraction runner needs provider state, ids, evidence, tracker, and optional store"
    )
)]
/// Run skill extraction as a background task. Logs results, never panics.
async fn run_skill_extraction(
    model: &str,
    providers: Arc<ProviderRegistry>,
    nous_id: &str,
    candidate_id: &str,
    tool_calls: &[mneme::skills::ToolCallRecord],
    #[cfg(feature = "knowledge-store")] source_session_id: &str,
    tracker: &mneme::skills::CandidateTracker,
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<&Arc<KnowledgeStore>>,
) {
    use mneme::skills::SkillExtractor;

    let candidates = tracker.candidates_for(nous_id);
    let Some(candidate) = candidates.iter().find(|c| c.id == candidate_id) else {
        warn!(candidate_id = %candidate_id, "candidate not found in tracker");
        return;
    };

    let provider = crate::extraction::HermeneusSkillExtractionProvider::new(providers, model);
    let extractor = SkillExtractor::new(provider);

    let sequences = if candidate.evidence.is_empty() {
        vec![tool_calls.to_vec()]
    } else {
        candidate
            .evidence
            .iter()
            .map(|item| item.tool_calls.clone())
            .collect()
    };

    match extractor
        .extract_skill_with_audit(candidate, &sequences, Some(model))
        .await
    {
        Ok(result) => {
            let extracted = result.skill;
            info!(
                nous_id = %nous_id,
                skill_name = %extracted.name,
                steps = extracted.steps.len(),
                tools = extracted.tools_used.len(),
                domains = ?extracted.domain_tags,
                "skill extracted from promoted candidate"
            );

            #[cfg(feature = "knowledge-store")]
            if let Some(store) = knowledge_store {
                let skill_content = extracted.to_skill_content();
                match store.find_duplicate_skill(nous_id, &skill_content) {
                    Ok(Some(existing_id)) => {
                        info!(
                            existing_id = %existing_id,
                            skill_name = %extracted.name,
                            "duplicate skill detected, skipping storage"
                        );
                        return;
                    }
                    // NOTE: no duplicate found, proceed with storage
                    Ok(None) => {}
                    Err(e) => {
                        warn!(error = %e, "failed to check skill duplicates, proceeding with storage");
                    }
                }

                let mut pending = mneme::skills::PendingSkill::new_with_provenance(
                    &extracted,
                    candidate,
                    result.audit,
                );
                if pending.source_session_id.is_none() {
                    pending.source_session_id = Some(source_session_id.to_owned());
                }
                match pending.to_json() {
                    Ok(content) => {
                        use mneme::knowledge::{
                            FactAccess, FactLifecycle, FactProvenance, FactTemporal,
                        };
                        let fact_id =
                            match mneme::id::FactId::new(koina::ulid::Ulid::new().to_string()) {
                                Ok(id) => id,
                                Err(e) => {
                                    warn!(error = %e, "failed to create fact ID for skill");
                                    return;
                                }
                            };
                        let now = jiff::Timestamp::now();
                        let fact = mneme::knowledge::Fact {
                            id: fact_id.clone(),
                            nous_id: nous_id.to_owned(),
                            content,
                            fact_type: "skill_pending".to_owned(),
                            scope: None,
                            project_id: None,
                            temporal: FactTemporal {
                                valid_from: now,
                                valid_to: jiff::Timestamp::from_second(i64::MAX / 2).unwrap_or(now),
                                recorded_at: now,
                            },
                            provenance: FactProvenance {
                                confidence: 0.6, // Pending review: moderate confidence
                                tier: mneme::knowledge::EpistemicTier::Inferred,
                                source_session_id: pending.source_session_id.clone(),
                                stability_hours: 720.0,
                            },
                            lifecycle: FactLifecycle {
                                superseded_by: None,
                                is_forgotten: false,
                                forgotten_at: None,
                                forget_reason: None,
                            },
                            access: FactAccess {
                                access_count: 0,
                                last_accessed_at: None,
                            },
                            sensitivity: mneme::knowledge::FactSensitivity::Public,
                            visibility: mneme::knowledge::Visibility::Private,
                        };

                        match store.insert_fact(&fact) {
                            Ok(()) => {
                                info!(
                                    fact_id = %fact_id,
                                    skill_name = %extracted.name,
                                    "pending skill stored for review"
                                );
                            }
                            Err(e) => {
                                warn!(error = %e, "failed to store pending skill");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to serialize pending skill");
                    }
                }
            }

            #[cfg(not(feature = "knowledge-store"))]
            {
                let _ = candidate_id; // WHY: suppress unused-variable warning in non-knowledge-store builds
                info!("skill extracted but knowledge-store feature disabled, not persisting");
            }
        }
        Err(e) => {
            warn!(
                nous_id = %nous_id,
                candidate_id = %candidate_id,
                error = %e,
                "skill extraction failed"
            );
            crate::metrics::record_background_failure(nous_id, "skill_extraction");
        }
    }
}

/// Run distillation as a background task. Loads history, calls LLM, applies results.
async fn run_background_distillation(
    store: Arc<Mutex<SessionStore>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern
    providers: Arc<ProviderRegistry>,
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<Arc<KnowledgeStore>>,
    session_id: String,
    nous_id: String,
    config: crate::distillation::DistillTriggerConfig,
    project_id: Option<mneme::workspace::ProjectId>,
) {
    let Some(provider) = providers.find_provider(&config.model) else {
        warn!(
            nous_id = %nous_id,
            session_id = %session_id,
            model = %config.model,
            "distillation aborted: no provider for configured model"
        );
        crate::metrics::record_background_failure(&nous_id, "distillation");
        return;
    };

    // WHY: load under lock, then release before async work to avoid holding lock across .await
    let (history, session) = {
        let s = store.lock().await;
        let Ok(Some(session)) = s.find_session_by_id(&session_id) else {
            warn!(
                nous_id = %nous_id,
                session_id = %session_id,
                "distillation aborted: session not found"
            );
            crate::metrics::record_background_failure(&nous_id, "distillation");
            return;
        };
        match s.get_history(&session_id, None) {
            Ok(h) if !h.is_empty() => (h, session),
            Ok(_) => return,
            Err(e) => {
                warn!(nous_id = %nous_id, error = %e, "failed to load history for distillation");
                crate::metrics::record_background_failure(&nous_id, "distillation");
                return;
            }
        }
    };

    let messages = crate::distillation::convert_to_hermeneus_messages(&history);
    let engine = melete::distill::DistillEngine::new(melete::distill::DistillConfig {
        model: config.model.clone(),
        verbatim_tail: config.verbatim_tail,
        ..Default::default()
    });

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "i64→u32: distillation count is small non-negative"
    )]
    let distill_count = session.metrics.distillation_count as u32; // kanon:ignore RUST/as-cast
    let result = match engine
        .distill(&messages, &nous_id, provider, distill_count + 1)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!(nous_id = %nous_id, session_id = %session_id, error = %e, "distillation LLM call failed");
            crate::metrics::record_background_failure(&nous_id, "distillation");
            return;
        }
    };

    #[cfg(feature = "knowledge-store")]
    if let Some(store) = knowledge_store.as_ref()
        && let Err(e) = crate::distillation::commit_memory_flush(
            store,
            &session_id,
            &nous_id,
            &result,
            &history,
            project_id.as_ref(),
        )
    {
        warn!(nous_id = %nous_id, session_id = %session_id, error = %e, "failed to commit distillation memory flush");
        crate::metrics::record_background_failure(&nous_id, "distillation_memory_flush");
        return;
    }

    let s = store.lock().await;
    if let Err(e) = crate::distillation::apply_distillation(&s, &session_id, &result, &history) {
        warn!(nous_id = %nous_id, session_id = %session_id, error = %e, "failed to apply distillation");
        crate::metrics::record_background_failure(&nous_id, "distillation");
        return;
    }

    info!(
        session_id = %session_id,
        messages_distilled = result.messages_distilled,
        "background distillation complete"
    );
}

// WHY: knowledge-store gates both `KnowledgeStore` and the persistence branch
// of `run_skill_extraction`; the test exercises that branch end-to-end.
#[cfg(all(test, feature = "knowledge-store"))]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use std::sync::Arc;

    use hermeneus::provider::ProviderRegistry;
    use hermeneus::test_utils::MockProvider;
    use melete::dream::TranscriptSource;

    use super::{SessionStoreTranscriptSource, run_skill_extraction};

    /// Drive real candidate/turn data through `run_skill_extraction` and assert
    /// the persisted pending skill carries derived source-session, redacted
    /// tool-call evidence with sequence hashes, and extraction audit refs.
    #[tokio::test]
    async fn run_skill_extraction_persists_pending_with_provenance() {
        let skill_json = r#"{
            "name": "diagnose-and-patch",
            "description": "Diagnose a failure then patch it",
            "steps": ["grep", "read", "edit"],
            "tools_used": ["Grep", "Read", "Edit", "Bash"],
            "domain_tags": ["debugging"],
            "when_to_use": "when fixing bugs"
        }"#;

        let mut providers = ProviderRegistry::new();
        providers.register(Box::new(
            MockProvider::new(skill_json).models(&["test-model"]),
        ));
        let providers = Arc::new(providers);

        // Build a candidate via the live tracker path with a secret-bearing
        // tool call so redaction is exercised, not assumed.
        let secret = "super-secret-token-value";
        let tool_calls = vec![
            mneme::skills::ToolCallRecord::new("Grep", 10).with_evidence(
                "t0",
                &serde_json::json!({ "pattern": "needle" }),
                Some("hits"),
                Some("receipt-0"),
            ),
            mneme::skills::ToolCallRecord::new("Read", 10),
            mneme::skills::ToolCallRecord::new("Read", 10),
            mneme::skills::ToolCallRecord::new("Edit", 10).with_evidence(
                "t3",
                &serde_json::json!({ "api_key": secret }),
                Some("patched"),
                Some("receipt-3"),
            ),
            mneme::skills::ToolCallRecord::new("Bash", 10),
            mneme::skills::ToolCallRecord::new("Bash", 10),
        ];

        let tracker = mneme::skills::CandidateTracker::new();
        tracker.track_sequence(&tool_calls, "session-alpha", "review-nous");
        let candidate_id = tracker
            .candidates_for("review-nous")
            .pop()
            .expect("candidate tracked") // kanon:ignore RUST/expect — test assertion
            .id;

        let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("knowledge store"); // kanon:ignore RUST/expect — test assertion

        run_skill_extraction(
            "test-model",
            providers,
            "review-nous",
            &candidate_id,
            &tool_calls,
            "turn-derived-session",
            &tracker,
            Some(&store),
        )
        .await;

        let pending_facts = store
            .find_pending_skills("review-nous")
            .expect("pending skills query"); // kanon:ignore RUST/expect — test assertion
        assert_eq!(pending_facts.len(), 1, "one pending skill persisted");
        let pending_fact = pending_facts.first().expect("one pending skill persisted"); // kanon:ignore RUST/expect — test assertion

        let pending = mneme::skills::PendingSkill::from_json(&pending_fact.content)
            .expect("pending skill deserializes"); // kanon:ignore RUST/expect — test assertion

        // Source session is derived (from the candidate evidence on the live
        // path), never None.
        assert_eq!(
            pending.source_session_id.as_deref(),
            Some("session-alpha"),
            "source session derived from candidate evidence"
        );
        assert_eq!(
            pending_fact.provenance.source_session_id.as_deref(),
            Some("session-alpha"),
            "fact provenance carries the derived source session"
        );

        // Evidence carries a non-empty sequence hash.
        let observation = pending
            .source_evidence
            .observations
            .first()
            .expect("observation evidence present"); // kanon:ignore RUST/expect — test assertion
        assert!(
            !observation.sequence_hash.is_empty(),
            "observation carries a sequence hash"
        );

        // Tool-call params are redacted, with the secret value absent.
        let redacted = observation
            .tool_calls
            .iter()
            .find(|tc| tc.tool_name == "Edit")
            .and_then(|tc| tc.redacted_input.as_ref())
            .and_then(|value| value.get("api_key"))
            .and_then(serde_json::Value::as_str)
            .expect("redacted api_key present"); // kanon:ignore RUST/expect — test assertion
        assert_eq!(redacted, "[REDACTED]", "secret tool param is redacted");
        assert!(
            !pending_fact.content.contains(secret),
            "secret value must not be persisted"
        );

        // Extraction prompt/response audit refs are retained.
        assert!(
            pending.extraction_audit.is_some(),
            "extraction audit refs retained"
        );
    }

    /// Regression test for #5750.
    ///
    /// WHY: `load_transcripts_since` previously listed sessions under one lock
    /// acquisition and then loaded history under a second, allowing a concurrent
    /// finalize to delete the session in between and produce an empty transcript
    /// from a stale session list. After the fix, a single guard covers both
    /// operations, so any session that appears in the result must still have
    /// non-empty history.
    #[tokio::test(flavor = "multi_thread")]
    async fn load_transcripts_since_is_atomic_against_concurrent_finalize() {
        let store = mneme::store::SessionStore::open_in_memory().expect("in-memory store"); // kanon:ignore RUST/expect - test asserts setup invariant
        let session_id = "550e8400-e29b-41d4-a716-446655440001";
        let nous_id = "nous-test-5750";
        let session_key = "test-key-5750";

        store
            .create_session(session_id, nous_id, session_key, None, None)
            .expect("create session"); // kanon:ignore RUST/expect - test asserts setup invariant
        store
            .append_message(session_id, mneme::types::Role::User, "hello", None, None, 1)
            .expect("append message"); // kanon:ignore RUST/expect - test asserts setup invariant

        let store = Arc::new(tokio::sync::Mutex::new(store));
        let source = SessionStoreTranscriptSource::new(Arc::clone(&store), nous_id.to_owned());

        let since = jiff::Timestamp::from_second(1_600_000_000).expect("valid epoch"); // kanon:ignore RUST/expect - test asserts fixed timestamp

        let finalizer_store = Arc::clone(&store);
        // kanon:ignore RUST/spawn-no-instrument - test task has no production tracing requirement
        let finalizer = tokio::spawn(async move {
            for i in 0..300 {
                if let Ok(guard) = finalizer_store.try_lock() {
                    guard.delete_session(session_id).expect("delete session"); // kanon:ignore RUST/expect - test asserts mutation invariant
                    if i % 2 == 1 {
                        guard
                            .create_session(session_id, nous_id, session_key, None, None)
                            .expect("create session"); // kanon:ignore RUST/expect - test asserts setup invariant
                        guard
                            .append_message(
                                session_id,
                                mneme::types::Role::User,
                                "hello",
                                None,
                                None,
                                1,
                            )
                            .expect("append message"); // kanon:ignore RUST/expect - test asserts setup invariant
                    }
                }
                tokio::task::yield_now().await;
            }
        });

        for _ in 0..300 {
            if let Ok(transcripts) = source.load_transcripts_since(since) {
                for transcript in transcripts {
                    if transcript.session_id == session_id {
                        assert!(
                            !transcript.messages.is_empty(),
                            "stale-list interleaving: session listed but history empty"
                        );
                    }
                }
            }
            tokio::task::yield_now().await;
        }

        finalizer.await.expect("finalizer task completes"); // kanon:ignore RUST/expect - test asserts task completion
    }
}
