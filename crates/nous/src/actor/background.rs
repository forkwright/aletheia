//! Background tasks — extraction, skill analysis, distillation, and task reaping.

use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{Instrument, debug, info, warn};

#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_mneme::store::SessionStore;

use aletheia_hermeneus::provider::ProviderRegistry;

use super::{MAX_SPAWNED_TASKS, NousActor};

impl NousActor {
    /// Reap completed background tasks and log any failures.
    pub(super) fn reap_background_tasks(&mut self) {
        while let Some(result) = self.background_tasks.try_join_next() {
            match result {
                Ok(()) => {}
                Err(e) => {
                    warn!(nous_id = %self.id, error = %e, "background task failed");
                }
            }
        }
    }

    pub(super) fn maybe_spawn_extraction(&mut self, user_content: &str, assistant_content: &str) {
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

        let config = extraction_config.clone();
        let providers = Arc::clone(&self.providers);
        let nous_id = self.id.clone();
        let user = user_content.to_owned();
        let assistant = assistant_content.to_owned();
        let span = tracing::info_span!("extraction", nous.id = %nous_id);
        #[cfg(feature = "knowledge-store")]
        let knowledge_store = self.knowledge_store.clone();

        if self.background_tasks.len() >= MAX_SPAWNED_TASKS {
            warn!(nous_id = %self.id, limit = MAX_SPAWNED_TASKS, "background task limit reached, skipping extraction");
            return;
        }

        self.background_tasks.spawn(
            async move {
                run_extraction(
                    &config,
                    providers,
                    &nous_id,
                    &user,
                    &assistant,
                    #[cfg(feature = "knowledge-store")]
                    knowledge_store.as_ref(),
                )
                .await;
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
        session_key: &str,
    ) {
        use aletheia_mneme::skills::{ToolCallRecord, TrackResult};

        if tool_calls.is_empty() {
            return;
        }

        // Convert pipeline ToolCalls to mneme ToolCallRecords
        let records: Vec<ToolCallRecord> = tool_calls
            .iter()
            .map(|tc| {
                if tc.is_error {
                    ToolCallRecord::errored(&tc.name, tc.duration_ms)
                } else {
                    ToolCallRecord::new(&tc.name, tc.duration_ms)
                }
            })
            .collect();

        let nous_id = self.id.clone();
        let result = self
            .candidate_tracker
            .track_sequence(&records, session_key, &nous_id);

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
                info!(candidate_id = %candidate_id, "skill analysis: candidate promoted, spawning LLM extraction");
                self.spawn_skill_extraction(&candidate_id, &records);
            }
        }
    }

    /// Spawn background LLM extraction for a promoted skill candidate.
    fn spawn_skill_extraction(
        &mut self,
        candidate_id: &str,
        tool_calls: &[aletheia_mneme::skills::ToolCallRecord],
    ) {
        let Some(ref extraction_config) = self.pipeline_config.extraction else {
            return;
        };

        // Use the extraction model (Haiku) for cost-effective skill extraction
        let model = extraction_config.model.clone();
        let providers = Arc::clone(&self.providers);
        let nous_id = self.id.clone();
        let candidate_id = candidate_id.to_owned();
        let tool_calls = tool_calls.to_vec();
        let tracker = Arc::clone(&self.candidate_tracker);
        #[cfg(feature = "knowledge-store")]
        let knowledge_store = self.knowledge_store.clone();
        let span = tracing::info_span!("skill_extraction", nous.id = %nous_id, candidate.id = %candidate_id);

        if self.background_tasks.len() >= MAX_SPAWNED_TASKS {
            warn!(nous_id = %self.id, limit = MAX_SPAWNED_TASKS, "background task limit reached, skipping skill extraction");
            return;
        }

        self.background_tasks.spawn(
            async move {
                run_skill_extraction(
                    &model,
                    providers,
                    &nous_id,
                    &candidate_id,
                    &tool_calls,
                    &tracker,
                    #[cfg(feature = "knowledge-store")]
                    knowledge_store.as_ref(),
                )
                .await;
            }
            .instrument(span),
        );
    }

    pub(super) async fn maybe_spawn_distillation(&mut self, session_key: &str) {
        let Some(ref store_arc) = self.session_store else {
            return;
        };
        let Some(session_state) = self.sessions.get(session_key) else {
            return;
        };
        let session_id = session_state.id.clone();

        // Quick trigger check under the lock — guard is dropped before any spawn
        let should_distill = {
            let store = store_arc.lock().await;
            let Ok(Some(session)) = store.find_session_by_id(&session_id) else {
                return;
            };
            let config = crate::distillation::DistillTriggerConfig::default();
            crate::distillation::should_trigger_distillation(
                &session,
                u64::from(self.config.context_window),
                &config,
            )
            .is_some()
        };

        if !should_distill {
            return;
        }

        let config = crate::distillation::DistillTriggerConfig::default();
        if self.providers.find_provider(&config.model).is_none() {
            warn!(model = %config.model, "no provider for distillation model");
            return;
        }

        let store = Arc::clone(store_arc);
        let providers = Arc::clone(&self.providers);
        let nous_id = self.id.clone();
        let span =
            tracing::info_span!("distillation", nous.id = %nous_id, session.id = %session_id);

        if self.background_tasks.len() >= MAX_SPAWNED_TASKS {
            warn!(nous_id = %self.id, limit = MAX_SPAWNED_TASKS, "background task limit reached, skipping distillation");
            return;
        }

        self.background_tasks.spawn(
            run_background_distillation(store, providers, session_id, nous_id, config)
                .instrument(span),
        );
    }
}

// ---------------------------------------------------------------------------
// Free async functions for background work
// ---------------------------------------------------------------------------

/// Run extraction as a background task. Logs results, never panics.
async fn run_extraction(
    config: &aletheia_mneme::extract::ExtractionConfig,
    providers: Arc<ProviderRegistry>,
    nous_id: &str,
    user_content: &str,
    assistant_content: &str,
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<&Arc<KnowledgeStore>>,
) {
    use aletheia_mneme::extract::{ConversationMessage, ExtractionEngine};

    let engine = ExtractionEngine::new(config.clone());
    let provider = crate::extraction::HermeneusExtractionProvider::new(providers, &config.model);

    let messages = vec![
        ConversationMessage {
            role: "user".to_owned(),
            content: user_content.to_owned(),
        },
        ConversationMessage {
            role: "assistant".to_owned(),
            content: assistant_content.to_owned(),
        },
    ];

    match engine.extract_refined(&messages, &provider).await {
        Ok(refined) => {
            let entities = refined.extraction.entities.len();
            let relationships = refined.extraction.relationships.len();
            let facts = refined.extraction.facts.len();

            #[cfg(feature = "knowledge-store")]
            if let Some(store) = knowledge_store {
                match engine.persist(&refined.extraction, store, "background", nous_id) {
                    Ok(result) => {
                        info!(
                            nous_id = %nous_id,
                            entities_persisted = result.entities_inserted,
                            relationships_persisted = result.relationships_inserted,
                            facts_persisted = result.facts_inserted,
                            "extraction persisted to knowledge store"
                        );
                    }
                    Err(e) => {
                        warn!(nous_id = %nous_id, error = %e, "extraction persist failed");
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
        }
    }
}

/// Run LLM skill extraction as a background task. Logs results, never panics.
async fn run_skill_extraction(
    model: &str,
    providers: Arc<ProviderRegistry>,
    nous_id: &str,
    candidate_id: &str,
    tool_calls: &[aletheia_mneme::skills::ToolCallRecord],
    tracker: &aletheia_mneme::skills::CandidateTracker,
    #[cfg(feature = "knowledge-store")] knowledge_store: Option<&Arc<KnowledgeStore>>,
) {
    use aletheia_mneme::skills::SkillExtractor;

    // Find the candidate in the tracker
    let candidates = tracker.candidates_for(nous_id);
    let Some(candidate) = candidates.iter().find(|c| c.id == candidate_id) else {
        warn!(candidate_id = %candidate_id, "candidate not found in tracker");
        return;
    };

    // Build the extraction provider (uses Haiku for cost-effectiveness)
    let provider = crate::extraction::HermeneusSkillExtractionProvider::new(providers, model);
    let extractor = SkillExtractor::new(provider);

    // The current turn's tool calls are the only sequence we have at this point.
    // In a richer implementation, we'd collect sequences from all session_refs.
    let sequences = vec![tool_calls.to_vec()];

    match extractor.extract_skill(candidate, &sequences).await {
        Ok(extracted) => {
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
                // Check for duplicates before storing
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
                    Ok(None) => {} // No duplicate, proceed
                    Err(e) => {
                        warn!(error = %e, "failed to check skill duplicates, proceeding with storage");
                    }
                }

                // Store as pending_review fact
                let pending = aletheia_mneme::skills::PendingSkill::new(&extracted, candidate_id);
                match pending.to_json() {
                    Ok(content) => {
                        let fact_id =
                            aletheia_mneme::id::FactId::from(ulid::Ulid::new().to_string());
                        let now = jiff::Timestamp::now();
                        let fact = aletheia_mneme::knowledge::Fact {
                            id: fact_id.clone(),
                            nous_id: nous_id.to_owned(),
                            content,
                            confidence: 0.6, // Pending review — moderate confidence
                            tier: aletheia_mneme::knowledge::EpistemicTier::Inferred,
                            valid_from: now,
                            valid_to: jiff::Timestamp::from_second(i64::MAX / 2).unwrap_or(now),
                            superseded_by: None,
                            source_session_id: None,
                            recorded_at: now,
                            access_count: 0,
                            last_accessed_at: None,
                            stability_hours: 720.0,
                            fact_type: "skill_pending".to_owned(),
                            is_forgotten: false,
                            forgotten_at: None,
                            forget_reason: None,
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
                let _ = candidate_id; // suppress unused warning
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
        }
    }
}

/// Run distillation as a background task. Loads history, calls LLM, applies results.
async fn run_background_distillation(
    store: Arc<Mutex<SessionStore>>,
    providers: Arc<ProviderRegistry>,
    session_id: String,
    nous_id: String,
    config: crate::distillation::DistillTriggerConfig,
) {
    let Some(provider) = providers.find_provider(&config.model) else {
        return;
    };

    // Load history under the lock, then release before async work.
    // Guard is scoped to the block and dropped before the .await below.
    let (history, session) = {
        let s = store.lock().await;
        let Ok(Some(session)) = s.find_session_by_id(&session_id) else {
            return;
        };
        match s.get_history(&session_id, None) {
            Ok(h) if !h.is_empty() => (h, session),
            Ok(_) => return,
            Err(e) => {
                warn!(error = %e, "failed to load history for distillation");
                return;
            }
        }
    }; // guard dropped here — lock released before await

    let messages = crate::distillation::convert_to_hermeneus_messages(&history);
    let engine =
        aletheia_melete::distill::DistillEngine::new(aletheia_melete::distill::DistillConfig {
            model: config.model.clone(),
            verbatim_tail: config.verbatim_tail,
            ..Default::default()
        });

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "distillation count is small non-negative"
    )]
    let distill_count = session.distillation_count as u32;
    let result = match engine
        .distill(&messages, &nous_id, provider, distill_count + 1)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "distillation LLM call failed");
            return;
        }
    };

    // Apply results under the lock — guard is scoped to this block
    let s = store.lock().await;
    if let Err(e) = crate::distillation::apply_distillation(&s, &session_id, &result, &history) {
        warn!(error = %e, "failed to apply distillation");
        return;
    }

    info!(
        session_id = %session_id,
        messages_distilled = result.messages_distilled,
        "background distillation complete"
    );
}
