#[cfg(any(feature = "gliner", feature = "nuextract"))]
use eidos::bookkeeping::BookkeepingProvider;
use snafu::IntoError;
use tracing::instrument;

#[cfg(any(feature = "gliner", feature = "nuextract"))]
use super::error::LlmCallSnafu;
#[cfg(feature = "mneme-engine")]
use super::error::PersistSnafu;
use super::error::{ExtractionError, ParseResponseSnafu};
use super::provider::ExtractionProvider;
use super::refinement;
#[cfg(feature = "mneme-engine")]
use super::types::PersistResult;
use super::types::{
    BookkeepingProviderKind, ConversationMessage, Extraction, ExtractionConfig, ExtractionPrompt,
    RefinedExtraction,
};
#[cfg(feature = "mneme-engine")]
use super::utils::slugify;
use super::utils::strip_code_fences;
#[cfg(feature = "gliner")]
use crate::bookkeeping::GlinerExtractionProvider;
use crate::bookkeeping::LlmBookkeepingProvider;
#[cfg(feature = "nuextract")]
use crate::bookkeeping::NuExtractProvider;
use crate::causal;

/// Small heuristic for first-person / assistant self-reference in fact subjects.
fn is_self_reference(subject: &str) -> bool {
    matches!(
        subject.trim().to_lowercase().as_str(),
        "i" | "me" | "myself" | "assistant"
    )
}

/// Drives the extraction pipeline: prompt building, LLM calling, response parsing.
///
/// # Examples
///
/// ```no_run
/// use episteme::extract::{ExtractionConfig, ExtractionEngine};
///
/// let config = ExtractionConfig::default();
/// let engine = ExtractionEngine::new(config);
/// ```
pub struct ExtractionEngine {
    config: ExtractionConfig,
}

impl ExtractionEngine {
    /// Create an extraction engine with the given configuration.
    #[must_use]
    #[instrument(skip(config))]
    pub fn new(config: ExtractionConfig) -> Self {
        Self { config }
    }

    /// Access the extraction configuration.
    #[must_use]
    #[instrument(skip(self))]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "extraction engine config accessor for pipeline consumers"
        )
    )]
    pub(crate) fn config(&self) -> &ExtractionConfig {
        &self.config
    }

    /// Build the system prompt and user message for knowledge extraction.
    ///
    /// When `turn_type` is `Some`, appends context-dependent extraction
    /// instructions to the base prompt.
    #[must_use]
    #[instrument(skip(self, messages), fields(msg_count = messages.len()))]
    pub(crate) fn build_prompt(&self, messages: &[ConversationMessage]) -> ExtractionPrompt {
        self.build_prompt_with_turn_type(messages, None)
    }

    /// Build the extraction prompt with optional turn-type-specific instructions.
    #[must_use]
    #[instrument(skip(self, messages), fields(msg_count = messages.len(), turn_type))]
    pub(crate) fn build_prompt_with_turn_type(
        &self,
        messages: &[ConversationMessage],
        turn_type: Option<refinement::TurnType>,
    ) -> ExtractionPrompt {
        let mut system = format!(
            r#"You are a knowledge extraction engine. Analyze the conversation and extract structured knowledge.

Output ONLY valid JSON matching this schema — no commentary, no markdown fences:
{{
  "entities": [
    {{ "name": "...", "entity_type": "person|project|concept|tool|location", "description": "..." }}
  ],
  "relationships": [
    {{ "source": "...", "relation": "verb phrase", "target": "...", "confidence": 0.0-1.0 }}
  ],
  "facts": [
    {{ "subject": "...", "predicate": "...", "object": "...", "confidence": 0.0-1.0 }}
  ]
}}

Rules:
- Extract entities mentioned: people, projects, concepts, tools, locations.
- Extract relationships between entities as verb phrases ("works on", "depends on", "created by").
- Extract factual claims as subject-predicate-object triples.
- Assign confidence: 1.0 for explicit statements, 0.5-0.8 for inferences, below 0.5 for weak signals.
- Normalize entity names: use proper nouns ("Alice" not "she", "Aletheia" not "the project").
- Skip greetings, small talk, and meta-conversation ("let me think about that").
- Maximum {max_entities} entities, {max_relationships} relationships, {max_facts} facts.
- If the conversation contains no extractable knowledge, return empty arrays.
- Process facts: when the assistant used tools to derive an answer, extract facts about the process itself (e.g., "Agent used tool X to verify Y"). Include tool names and what they were used to accomplish.
- Reasoning blocks: if reasoning is present, extract the underlying rationale as facts about why certain approaches were chosen."#,
            max_entities = self.config.max_entities,
            max_relationships = self.config.max_relationships,
            max_facts = self.config.max_facts,
        );

        if let Some(tt) = turn_type {
            system.push_str("\n\nContext-specific instructions:\n");
            system.push_str(tt.prompt_appendix());
        }

        if self.config.events_only_prompt {
            system.push_str(
                "\n\nAdditional rule: extraction must capture only concrete events and observations. \
                 Do NOT emit self-descriptive facts, preferences, identity claims, or meta-relational facts.\n",
            );
        }

        let mut conversation = String::new();
        for msg in messages {
            conversation.push_str(&msg.role);
            conversation.push_str(": ");
            conversation.push_str(&msg.content);
            conversation.push('\n');
            if let Some(ref reasoning) = msg.reasoning {
                conversation.push_str("reasoning: ");
                conversation.push_str(reasoning);
                conversation.push('\n');
            }
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    conversation.push_str("tool_call: ");
                    conversation.push_str(&tc.name);
                    conversation.push_str(" id=");
                    conversation.push_str(&tc.id);
                    conversation.push_str(" input=");
                    conversation.push_str(&tc.input.to_string());
                    if let Some(ref result) = tc.result {
                        conversation.push_str(" result=");
                        conversation.push_str(result);
                    }
                    if tc.is_error {
                        conversation.push_str(" [ERROR]");
                    }
                    conversation.push('\n');
                }
            }
        }

        ExtractionPrompt {
            system,
            user_message: conversation,
        }
    }

    /// Parse a JSON extraction response from the LLM.
    ///
    /// Strips markdown code fences if present. On parse failure, includes the
    /// first 500 characters of the raw response for debugging.
    #[instrument(skip(self, response))]
    #[expect(
        clippy::unused_self,
        reason = "method signature kept for API consistency"
    )]
    pub(crate) fn parse_response(&self, response: &str) -> Result<Extraction, ExtractionError> {
        let trimmed = strip_code_fences(response);
        serde_json::from_str(trimmed).map_err(|source| {
            // WHY: include response text so operators can diagnose malformed LLM output
            // without correlating with provider logs.
            let response_snippet: String = response.chars().take(500).collect();
            ParseResponseSnafu { response_snippet }.into_error(source)
        })
    }

    /// Run extraction end-to-end: build prompt, call provider, parse response.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider call fails or if the response cannot be parsed.
    #[instrument(skip(self, provider))]
    pub async fn extract(
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
    ) -> Result<Extraction, ExtractionError> {
        let total_len: usize = messages
            .iter()
            .map(|m| {
                let mut len = m.content.len();
                if let Some(ref reasoning) = m.reasoning {
                    len += reasoning.len();
                }
                if let Some(ref tool_calls) = m.tool_calls {
                    for tc in tool_calls {
                        len += tc.name.len() + tc.id.len() + tc.input.to_string().len();
                        if let Some(ref result) = tc.result {
                            len += result.len();
                        }
                    }
                }
                len
            })
            .sum();
        if total_len < self.config.min_message_length {
            return Ok(Extraction {
                entities: vec![],
                relationships: vec![],
                facts: vec![],
            });
        }

        self.extract_with_selected_provider(messages, provider, None)
            .await
    }

    /// Run extraction with context-dependent refinement.
    ///
    /// Classifies the turn, applies per-type prompt instructions, detects
    /// corrections, classifies fact types, applies quality filters, and boosts
    /// confidence where appropriate. Quality signals are recorded as metrics so
    /// operators can track extraction precision, calibration, and drift.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider call fails or if the response cannot be parsed.
    #[instrument(skip(self, provider))]
    #[expect(
        clippy::too_many_lines,
        reason = "refinement pipeline: classify, prompt, parse, filter, boost — sequential by design"
    )]
    pub async fn extract_refined(
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
        nous_id: &str,
        producer: &str,
    ) -> Result<RefinedExtraction, ExtractionError> {
        let total_len: usize = messages
            .iter()
            .map(|m| {
                let mut len = m.content.len();
                if let Some(ref reasoning) = m.reasoning {
                    len += reasoning.len();
                }
                if let Some(ref tool_calls) = m.tool_calls {
                    for tc in tool_calls {
                        len += tc.name.len() + tc.id.len() + tc.input.to_string().len();
                        if let Some(ref result) = tc.result {
                            len += result.len();
                        }
                    }
                }
                len
            })
            .sum();
        if total_len < self.config.min_message_length {
            return Ok(RefinedExtraction {
                extraction: Extraction {
                    entities: vec![],
                    relationships: vec![],
                    facts: vec![],
                },
                turn_type: refinement::TurnType::Discussion,
                facts_filtered: 0,
                causal_signal: None,
            });
        }

        let combined: String =
            messages
                .iter()
                .map(|m| m.content.as_str())
                .fold(String::new(), |mut acc, s| {
                    if !acc.is_empty() {
                        acc.push('\n');
                    }
                    acc.push_str(s);
                    acc
                });
        let turn_type = refinement::classify_turn(&combined);
        let mut extraction = self
            .extract_with_selected_provider(messages, provider, Some(turn_type))
            .await?;
        let correction = refinement::detect_correction(&combined);
        let boost = turn_type.confidence_boost() + correction.confidence_boost;
        let mut filtered_count = 0;
        let provider_label = provider.provider_label();
        let model_label = provider.model_label();

        // WHY: check for LLM confidence inflation before filtering individual facts,
        // so the warning fires even if some facts are later filtered for other reasons.
        let confidence_tuples: Vec<(f64,)> =
            extraction.facts.iter().map(|f| (f.confidence,)).collect();
        if refinement::has_confidence_inflation(&confidence_tuples) {
            tracing::warn!(
                total_facts = extraction.facts.len(),
                "confidence inflation detected: >80% of extracted facts have confidence >= 0.95"
            );
            crate::metrics::record_confidence_inflation(
                nous_id,
                producer,
                &provider_label,
                &model_label,
            );
        }

        extraction.facts = extraction
            .facts
            .into_iter()
            .filter_map(|mut fact| {
                let raw_confidence = fact.confidence;
                crate::metrics::record_extraction_confidence(
                    nous_id,
                    producer,
                    &provider_label,
                    &model_label,
                    "extracted",
                    raw_confidence,
                );

                // WHY: reject facts with empty triple fields before any other processing —
                // a fact with no subject, predicate, or object has zero semantic value.
                if !refinement::validate_triple_fields(&fact.subject, &fact.predicate, &fact.object)
                {
                    filtered_count += 1;
                    crate::metrics::record_extraction_quality(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                        "rejected",
                        "empty_field",
                    );
                    crate::metrics::record_extraction_confidence(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                        "rejected",
                        raw_confidence,
                    );
                    tracing::debug!(
                        subject = %fact.subject,
                        predicate = %fact.predicate,
                        object = %fact.object,
                        "fact rejected: empty subject, predicate, or object"
                    );
                    return None;
                }

                if !self.config.extract_self_facts && is_self_reference(&fact.subject) {
                    filtered_count += 1;
                    crate::metrics::record_extraction_quality(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                        "rejected",
                        "self_reference",
                    );
                    crate::metrics::record_extraction_confidence(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                        "rejected",
                        raw_confidence,
                    );
                    tracing::debug!(
                        subject = %fact.subject,
                        "fact filtered out: self-reference"
                    );
                    return None;
                }

                let fact_content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
                let classified_type = refinement::classify_fact(&fact_content);
                fact.fact_type = Some(classified_type.as_str().to_owned());
                if correction.is_correction {
                    fact.is_correction = true;
                    crate::metrics::record_extraction_correction(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                    );
                }
                fact.confidence = refinement::boosted_confidence(fact.confidence, boost);
                let filter = refinement::filter_fact(&fact_content, fact.confidence);
                if filter.passed {
                    crate::metrics::record_extraction_quality(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                        "accepted",
                        "",
                    );
                    crate::metrics::record_extraction_confidence(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                        "accepted",
                        fact.confidence,
                    );
                    Some(fact)
                } else {
                    filtered_count += 1;
                    let reason = filter
                        .reason
                        .as_ref()
                        .map_or_else(String::new, std::string::ToString::to_string);
                    crate::metrics::record_extraction_quality(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                        "rejected",
                        &reason,
                    );
                    crate::metrics::record_extraction_confidence(
                        nous_id,
                        producer,
                        &provider_label,
                        &model_label,
                        "rejected",
                        raw_confidence,
                    );
                    tracing::debug!(
                        subject = %fact.subject,
                        reason = ?filter.reason,
                        "fact filtered out during extraction refinement"
                    );
                    None
                }
            })
            .collect();

        // WHY: detect causal language in the combined session text so callers
        // can trigger causal edge extraction without re-scanning the text.
        let causal_signal = causal::detect_causal_cue(&combined);
        if causal_signal.is_some() {
            tracing::debug!("causal signal detected in session text during extraction refinement");
        }

        Ok(RefinedExtraction {
            extraction,
            turn_type,
            facts_filtered: filtered_count,
            causal_signal,
        })
    }

    /// Persist an extraction to the knowledge store.
    ///
    /// # Errors
    ///
    /// Returns an error if storing entities, relationships, or facts fails.
    #[cfg(feature = "mneme-engine")]
    #[instrument(
        skip(self, store, extraction),
        fields(
            entity_count = extraction.entities.len(),
            relationship_count = extraction.relationships.len(),
            fact_count = extraction.facts.len(),
        )
    )]
    pub fn persist(
        &self,
        extraction: &Extraction,
        store: &crate::knowledge_store::KnowledgeStore,
        source: &str,
        nous_id: &str,
    ) -> Result<PersistResult, ExtractionError> {
        self.persist_with_scope(
            extraction,
            store,
            source,
            nous_id,
            Some(crate::knowledge::MemoryScope::Project),
        )
    }

    /// Persist an extraction to the knowledge store with an explicit memory scope.
    ///
    /// `scope` is copied onto every inserted fact. Pass `None` only for legacy
    /// imports where no parent session or project scope exists.
    #[cfg(feature = "mneme-engine")]
    #[instrument(
        skip(self, store, extraction),
        fields(
            entity_count = extraction.entities.len(),
            relationship_count = extraction.relationships.len(),
            fact_count = extraction.facts.len(),
        )
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "sequential extraction pipeline: entities → relationships → facts"
    )]
    pub fn persist_with_scope(
        &self,
        extraction: &Extraction,
        store: &crate::knowledge_store::KnowledgeStore,
        source: &str,
        nous_id: &str,
        scope: Option<crate::knowledge::MemoryScope>,
    ) -> Result<PersistResult, ExtractionError> {
        use crate::knowledge::{
            Entity, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal, Relationship,
            far_future,
        };

        let now = jiff::Timestamp::now();
        let mut result = PersistResult::default();

        if extraction.entities.len() > self.config.max_entities {
            tracing::warn!(
                count = extraction.entities.len(),
                limit = self.config.max_entities,
                "extraction entity limit exceeded, truncating"
            );
        }
        let entities = extraction.entities.iter().take(self.config.max_entities);

        if extraction.relationships.len() > self.config.max_relationships {
            tracing::warn!(
                count = extraction.relationships.len(),
                limit = self.config.max_relationships,
                "extraction relationship limit exceeded, truncating"
            );
        }
        let relationships = extraction
            .relationships
            .iter()
            .take(self.config.max_relationships);

        if extraction.facts.len() > self.config.max_facts {
            tracing::warn!(
                count = extraction.facts.len(),
                limit = self.config.max_facts,
                "extraction fact limit exceeded, truncating"
            );
        }
        let facts = extraction.facts.iter().take(self.config.max_facts);

        // #4675: track the entities written in this extraction so each fact can
        // be linked to the subject/object entities it references. Linking is
        // scoped to entities known from this batch; a subject/object name that
        // did not resolve to an inserted entity is skipped rather than linked to
        // a dangling id. Existing facts are reconnected by the v17->v18 backfill.
        let mut known_entity_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for entity in entities {
            // WHY: reject entities with empty names — they cannot be referenced or queried.
            if entity.name.trim().is_empty() {
                tracing::debug!(
                    entity_type = %entity.entity_type,
                    "entity rejected: empty name"
                );
                continue;
            }
            let id = crate::id::EntityId::new(slugify(&entity.name)).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let aliases = if entity.description.is_empty() {
                vec![]
            } else {
                vec![entity.description.clone()]
            };
            let entity_type = if entity.entity_type.is_empty() {
                tracing::debug!(name = %entity.name, "entity has empty type, defaulting to 'concept'");
                "concept".to_owned()
            } else {
                entity.entity_type.clone()
            };
            let e = Entity {
                id,
                name: entity.name.clone(),
                entity_type,
                aliases,
                created_at: now,
                updated_at: now,
            };
            known_entity_ids.insert(e.id.as_str().to_owned());
            store.insert_entity(&e).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            result.entities_inserted += 1;
        }

        for rel in relationships {
            let relation_type = match crate::vocab::normalize_relation(&rel.relation) {
                crate::vocab::RelationType::Known(canonical) => canonical.to_owned(),
                crate::vocab::RelationType::Novel(normalized) => {
                    tracing::info!(
                        raw = %rel.relation,
                        normalized = %normalized,
                        source = %rel.source,
                        target = %rel.target,
                        "accepting novel relationship type from LLM"
                    );
                    normalized
                }
                crate::vocab::RelationType::Rejected => {
                    tracing::warn!(
                        relation = %rel.relation,
                        source = %rel.source,
                        target = %rel.target,
                        "rejected relationship with banned type"
                    );
                    result.relationships_skipped += 1;
                    continue;
                }
                crate::vocab::RelationType::Malformed => {
                    tracing::warn!(
                        relation = %rel.relation,
                        source = %rel.source,
                        target = %rel.target,
                        "rejected relationship with malformed type"
                    );
                    result.relationships_skipped += 1;
                    continue;
                }
            };
            let src = crate::id::EntityId::new(slugify(&rel.source)).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let dst = crate::id::EntityId::new(slugify(&rel.target)).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let r = Relationship {
                src,
                dst,
                relation: relation_type,
                weight: rel.confidence,
                created_at: now,
            };
            store.insert_relationship(&r).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            result.relationships_inserted += 1;
        }

        for (i, fact) in facts.enumerate() {
            // WHY: guard against empty triple fields reaching the knowledge store —
            // the extraction pipeline should have caught these, but persist is the
            // last line of defense.
            if fact.subject.trim().is_empty()
                || fact.predicate.trim().is_empty()
                || fact.object.trim().is_empty()
            {
                tracing::debug!(
                    subject = %fact.subject,
                    predicate = %fact.predicate,
                    object = %fact.object,
                    "fact skipped during persist: empty subject, predicate, or object"
                );
                continue;
            }
            let content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
            let id = crate::id::FactId::new(format!(
                "{}-{}-{i}",
                slugify(&fact.subject),
                slugify(&fact.predicate)
            ))
            .map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let classified_type = fact.fact_type.as_deref().map_or_else(
                || crate::knowledge::FactType::classify(&content),
                crate::knowledge::FactType::from_str_lossy,
            );
            // WHY: Identity-type facts ("I am X", "My name is Y") are self-descriptions
            // about the agent, not episodic knowledge. Persisting them causes persona drift
            // when they are later recalled as facts about external entities. The upstream
            // is_self_reference filter catches subject="I/me" triples; this guard catches
            // any Identity-typed fact that slipped through (e.g. third-person extraction).
            if classified_type == crate::knowledge::FactType::Identity {
                tracing::debug!(content = %content, "fact skipped at persist: identity-type self-description");
                continue;
            }
            let is_correction =
                fact.is_correction || crate::conflict::is_correction_heuristic(&content);
            let confidence = if is_correction {
                crate::conflict::apply_correction_boost(fact.confidence)
            } else {
                fact.confidence
            };
            let f = Fact {
                id,
                nous_id: nous_id.to_owned(),
                content,
                fact_type: classified_type.as_str().to_owned(),
                scope,
                project_id: None,
                temporal: FactTemporal {
                    valid_from: now,
                    valid_to: far_future(),
                    recorded_at: now,
                },
                provenance: FactProvenance {
                    confidence,
                    tier: self.config.default_tier,
                    source_session_id: Some(source.to_owned()),
                    stability_hours: classified_type.base_stability_hours(),
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
                sensitivity: crate::knowledge::FactSensitivity::Public,
                visibility: crate::knowledge::Visibility::Private,
            };
            store.insert_fact(&f).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            result.facts_inserted += 1;

            // #4675: link the fact to the subject/object entities it references
            // so graph-aware recall, scoped dedup, and consolidation see real
            // fact-entity edges. The edge is idempotent (keyed put); subject and
            // object are de-duplicated per fact so a reflexive triple links once.
            let mut linked_this_fact: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            for reference in [fact.subject.as_str(), fact.object.as_str()] {
                let entity_slug = slugify(reference);
                if !known_entity_ids.contains(&entity_slug)
                    || !linked_this_fact.insert(entity_slug.clone())
                {
                    continue;
                }
                let Ok(entity_id) = crate::id::EntityId::new(entity_slug) else {
                    continue;
                };
                match store.insert_fact_entity(&f.id, &entity_id) {
                    Ok(()) => result.fact_entities_inserted += 1,
                    Err(error) => tracing::debug!(
                        %error,
                        fact_id = %f.id,
                        entity_id = %entity_id,
                        "failed to link fact to referenced entity"
                    ),
                }
            }
        }

        Ok(result)
    }

    async fn extract_with_selected_provider(
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
        turn_type: Option<refinement::TurnType>,
    ) -> Result<Extraction, ExtractionError> {
        match self.config.provider {
            BookkeepingProviderKind::Llm => {
                let bookkeeping = LlmBookkeepingProvider::new(self, provider);
                if let Some(turn_type) = turn_type {
                    bookkeeping
                        .extract_messages_with_turn_type(messages, turn_type)
                        .await
                } else {
                    bookkeeping.extract_messages(messages).await
                }
            }
            BookkeepingProviderKind::Gliner => {
                #[cfg(feature = "gliner")]
                {
                    let bookkeeping = GlinerExtractionProvider::new(self, provider)
                        .map_err(|err| bookkeeping_to_extraction_error(&err))?;
                    if let Some(turn_type) = turn_type {
                        bookkeeping
                            .extract_messages_with_turn_type(
                                messages,
                                turn_type,
                                &self.config.schema(),
                            )
                            .await
                            .map_err(|err| bookkeeping_to_extraction_error(&err))
                    } else {
                        bookkeeping
                            .extract_knowledge(messages, &self.config.schema())
                            .await
                            .map_err(|err| bookkeeping_to_extraction_error(&err))
                    }
                }
                #[cfg(not(feature = "gliner"))]
                {
                    tracing::warn!(
                        "GLiNER bookkeeping provider requested but episteme/gliner is disabled; falling back to LLM"
                    );
                    let bookkeeping = LlmBookkeepingProvider::new(self, provider);
                    if let Some(turn_type) = turn_type {
                        bookkeeping
                            .extract_messages_with_turn_type(messages, turn_type)
                            .await
                    } else {
                        bookkeeping.extract_messages(messages).await
                    }
                }
            }
            BookkeepingProviderKind::NuExtract => {
                #[cfg(feature = "nuextract")]
                {
                    let bookkeeping = NuExtractProvider::new()
                        .map_err(|err| bookkeeping_to_extraction_error(&err))?;
                    bookkeeping
                        .extract_knowledge(messages, &self.config.schema())
                        .await
                        .map_err(|err| bookkeeping_to_extraction_error(&err))
                }
                #[cfg(not(feature = "nuextract"))]
                {
                    tracing::warn!(
                        "NuExtract bookkeeping provider requested but episteme/nuextract is disabled; falling back to LLM"
                    );
                    let bookkeeping = LlmBookkeepingProvider::new(self, provider);
                    if let Some(turn_type) = turn_type {
                        bookkeeping
                            .extract_messages_with_turn_type(messages, turn_type)
                            .await
                    } else {
                        bookkeeping.extract_messages(messages).await
                    }
                }
            }
        }
    }
}

#[cfg(any(feature = "gliner", feature = "nuextract"))]
fn bookkeeping_to_extraction_error(
    error: &eidos::bookkeeping::BookkeepingError,
) -> ExtractionError {
    LlmCallSnafu {
        message: error.to_string(),
    }
    .build()
}
