use snafu::ResultExt;
use tracing::instrument;

#[cfg(feature = "mneme-engine")]
use super::error::PersistSnafu;
use super::error::{ExtractionError, ParseResponseSnafu};
use super::provider::ExtractionProvider;
use super::refinement;
#[cfg(feature = "mneme-engine")]
use super::types::PersistResult;
use super::types::{
    ConversationMessage, Extraction, ExtractionConfig, ExtractionPrompt, RefinedExtraction,
};
#[cfg(feature = "mneme-engine")]
use super::utils::slugify;
use super::utils::strip_code_fences;

/// Drives the extraction pipeline: prompt building, LLM calling, response parsing.
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
    pub fn config(&self) -> &ExtractionConfig {
        &self.config
    }

    /// Build the system prompt and user message for knowledge extraction.
    ///
    /// When `turn_type` is `Some`, appends context-dependent extraction
    /// instructions to the base prompt.
    #[must_use]
    #[instrument(skip(self, messages), fields(msg_count = messages.len()))]
    pub fn build_prompt(&self, messages: &[ConversationMessage]) -> ExtractionPrompt {
        self.build_prompt_with_turn_type(messages, None)
    }

    /// Build the extraction prompt with optional turn-type-specific instructions.
    #[must_use]
    #[instrument(skip(self, messages), fields(msg_count = messages.len(), turn_type))]
    pub fn build_prompt_with_turn_type(
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
- If the conversation contains no extractable knowledge, return empty arrays."#,
            max_entities = self.config.max_entities,
            max_relationships = self.config.max_relationships,
            max_facts = self.config.max_facts,
        );

        if let Some(tt) = turn_type {
            system.push_str("\n\nContext-specific instructions:\n");
            system.push_str(tt.prompt_appendix());
        }

        let mut conversation = String::new();
        for msg in messages {
            conversation.push_str(&msg.role);
            conversation.push_str(": ");
            conversation.push_str(&msg.content);
            conversation.push('\n');
        }

        ExtractionPrompt {
            system,
            user_message: conversation,
        }
    }

    /// Parse a JSON extraction response from the LLM.
    ///
    /// Strips markdown code fences if present.
    #[instrument(skip(self, response))]
    #[must_use]
    pub fn parse_response(&self, response: &str) -> Result<Extraction, ExtractionError> {
        let trimmed = strip_code_fences(response);
        serde_json::from_str(trimmed).context(ParseResponseSnafu)
    }

    /// Run extraction end-to-end: build prompt, call provider, parse response.
    #[instrument(skip(self, provider))]
    pub async fn extract(
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
    ) -> Result<Extraction, ExtractionError> {
        let total_len: usize = messages.iter().map(|m| m.content.len()).sum();
        if total_len < self.config.min_message_length {
            return Ok(Extraction {
                entities: vec![],
                relationships: vec![],
                facts: vec![],
            });
        }

        let prompt = self.build_prompt(messages);
        let response = provider
            .complete(&prompt.system, &prompt.user_message)
            .await?;
        self.parse_response(&response)
    }

    /// Run extraction with context-dependent refinement.
    ///
    /// Classifies the turn, applies per-type prompt instructions, detects
    /// corrections, classifies fact types, applies quality filters, and boosts
    /// confidence where appropriate.
    #[instrument(skip(self, provider))]
    pub async fn extract_refined(
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
    ) -> Result<RefinedExtraction, ExtractionError> {
        let total_len: usize = messages.iter().map(|m| m.content.len()).sum();
        if total_len < self.config.min_message_length {
            return Ok(RefinedExtraction {
                extraction: Extraction {
                    entities: vec![],
                    relationships: vec![],
                    facts: vec![],
                },
                turn_type: refinement::TurnType::Discussion,
                facts_filtered: 0,
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
        let prompt = self.build_prompt_with_turn_type(messages, Some(turn_type));
        let response = provider
            .complete(&prompt.system, &prompt.user_message)
            .await?;
        let mut extraction = self.parse_response(&response)?;
        let correction = refinement::detect_correction(&combined);
        let boost = turn_type.confidence_boost() + correction.confidence_boost;
        let mut filtered_count = 0;

        extraction.facts = extraction
            .facts
            .into_iter()
            .filter_map(|mut fact| {
                let fact_content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
                let classified_type = refinement::classify_fact(&fact_content);
                fact.fact_type = Some(classified_type.as_str().to_owned());
                if correction.is_correction {
                    fact.is_correction = true;
                }
                fact.confidence = refinement::boosted_confidence(fact.confidence, boost);
                let filter = refinement::filter_fact(&fact_content, fact.confidence);
                if filter.passed {
                    Some(fact)
                } else {
                    filtered_count += 1;
                    tracing::debug!(
                        subject = %fact.subject,
                        reason = ?filter.reason,
                        "fact filtered out during extraction refinement"
                    );
                    None
                }
            })
            .collect();

        Ok(RefinedExtraction {
            extraction,
            turn_type,
            facts_filtered: filtered_count,
        })
    }

    /// Persist an extraction to the knowledge store.
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
    pub fn persist(
        &self,
        extraction: &Extraction,
        store: &crate::knowledge_store::KnowledgeStore,
        source: &str,
        nous_id: &str,
    ) -> Result<PersistResult, ExtractionError> {
        use crate::knowledge::{Entity, EpistemicTier, Fact, Relationship, far_future};

        let now = jiff::Timestamp::now();
        let mut result = PersistResult::default();

        let entities = if extraction.entities.len() > self.config.max_entities {
            tracing::warn!(
                count = extraction.entities.len(),
                limit = self.config.max_entities,
                "extraction entity limit exceeded, truncating"
            );
            &extraction.entities[..self.config.max_entities]
        } else {
            &extraction.entities
        };
        let relationships = if extraction.relationships.len() > self.config.max_relationships {
            tracing::warn!(
                count = extraction.relationships.len(),
                limit = self.config.max_relationships,
                "extraction relationship limit exceeded, truncating"
            );
            &extraction.relationships[..self.config.max_relationships]
        } else {
            &extraction.relationships
        };
        let facts = if extraction.facts.len() > self.config.max_facts {
            tracing::warn!(
                count = extraction.facts.len(),
                limit = self.config.max_facts,
                "extraction fact limit exceeded, truncating"
            );
            &extraction.facts[..self.config.max_facts]
        } else {
            &extraction.facts
        };

        for entity in entities {
            let id = crate::id::EntityId::from(slugify(&entity.name));
            let aliases = if entity.description.is_empty() {
                vec![]
            } else {
                vec![entity.description.clone()]
            };
            let e = Entity {
                id,
                name: entity.name.clone(),
                entity_type: entity.entity_type.clone(),
                aliases,
                created_at: now,
                updated_at: now,
            };
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
            let r = Relationship {
                src: crate::id::EntityId::from(slugify(&rel.source)),
                dst: crate::id::EntityId::from(slugify(&rel.target)),
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

        for (i, fact) in facts.iter().enumerate() {
            let content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
            let id = crate::id::FactId::from(format!(
                "{}-{}-{i}",
                slugify(&fact.subject),
                slugify(&fact.predicate)
            ));
            let classified_type = fact.fact_type.as_deref().map_or_else(
                || crate::knowledge::FactType::classify(&content),
                crate::knowledge::FactType::from_str_lossy,
            );
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
                confidence,
                tier: EpistemicTier::Inferred,
                valid_from: now,
                valid_to: far_future(),
                superseded_by: None,
                source_session_id: Some(source.to_owned()),
                recorded_at: now,
                access_count: 0,
                last_accessed_at: None,
                stability_hours: classified_type.base_stability_hours(),
                fact_type: classified_type.as_str().to_owned(),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            };
            store.insert_fact(&f).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            result.facts_inserted += 1;
        }

        Ok(result)
    }
}
