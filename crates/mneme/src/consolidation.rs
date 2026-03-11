//! LLM-driven fact consolidation for knowledge maintenance.
//!
//! When an entity has 10+ facts or a community cluster exceeds 20 facts,
//! the system sends them to an LLM for summarization into fewer, higher-quality
//! facts. Originals are superseded, not deleted — full provenance is preserved
//! via the `consolidation_audit` relation.
//!
//! ## Safeguards
//!
//! - **Age gate:** Only facts older than 7 days are eligible
//! - **Tier protection:** `Verified` facts are never consolidated
//! - **Dry-run mode:** Log proposed consolidations without executing
//! - **Batch limit:** Max 50 facts per LLM call
//! - **Rate limit:** Max 1 consolidation cycle per hour per nous

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use crate::id::{EntityId, FactId};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Thresholds and limits for fact consolidation.
#[derive(Debug, Clone)]
pub struct ConsolidationConfig {
    /// Minimum facts per entity before consolidation triggers (default: 10).
    pub entity_fact_threshold: usize,
    /// Minimum facts per community cluster before consolidation triggers (default: 20).
    pub community_fact_threshold: usize,
    /// Minimum age in days before a fact is eligible for consolidation (default: 7).
    pub min_age_days: u32,
    /// Maximum facts to send in a single LLM call (default: 50).
    pub batch_limit: usize,
    /// Minimum hours between consolidation cycles for the same nous (default: 1).
    pub rate_limit_hours: f64,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            entity_fact_threshold: 10,
            community_fact_threshold: 20,
            min_age_days: 7,
            batch_limit: 50,
            rate_limit_hours: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the fact consolidation pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum ConsolidationError {
    /// The LLM consolidation call failed.
    #[snafu(display("consolidation LLM call failed: {message}"))]
    LlmCall {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The LLM response could not be parsed as valid consolidation JSON.
    #[snafu(display("failed to parse consolidation response: {source}"))]
    ParseResponse {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Knowledge store operation failed during consolidation.
    #[snafu(display("consolidation store error: {message}"))]
    Store {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Rate limit exceeded — too soon since the last consolidation cycle.
    #[snafu(display(
        "rate limited: last consolidation was {elapsed_hours:.1}h ago (min {min_hours:.1}h)"
    ))]
    RateLimited {
        elapsed_hours: f64,
        min_hours: f64,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Why a consolidation was triggered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsolidationTrigger {
    /// An entity accumulated more than the threshold of active facts.
    EntityOverflow {
        entity_id: EntityId,
        fact_count: usize,
    },
    /// A Louvain community cluster accumulated more than the threshold of active facts.
    CommunityOverflow {
        cluster_id: i64,
        fact_count: usize,
    },
}

impl ConsolidationTrigger {
    /// Short label for audit logging.
    #[must_use]
    pub fn trigger_type(&self) -> &'static str {
        match self {
            Self::EntityOverflow { .. } => "entity_overflow",
            Self::CommunityOverflow { .. } => "community_overflow",
        }
    }

    /// The entity or cluster identifier as a string.
    #[must_use]
    pub fn trigger_id(&self) -> String {
        match self {
            Self::EntityOverflow { entity_id, .. } => entity_id.to_string(),
            Self::CommunityOverflow { cluster_id, .. } => cluster_id.to_string(),
        }
    }
}

/// A cluster of facts ready for LLM consolidation.
#[derive(Debug, Clone)]
pub struct ConsolidationCandidate {
    /// Why this cluster was selected.
    pub trigger: ConsolidationTrigger,
    /// IDs of the facts to consolidate.
    pub fact_ids: Vec<FactId>,
    /// Number of eligible facts.
    pub fact_count: usize,
    /// Entity that triggered consolidation (if entity-triggered).
    pub entity_id: Option<EntityId>,
    /// Community cluster that triggered consolidation (if community-triggered).
    pub cluster_id: Option<i64>,
}

/// A single consolidated fact produced by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidatedFact {
    /// The consolidated fact content.
    pub content: String,
    /// Confidence score (fixed at 0.95 for consolidation outputs).
    pub confidence: f64,
    /// Epistemic tier (always `inferred` for LLM consolidation outputs).
    pub tier: String,
    /// IDs of the original facts that were consolidated into this one.
    pub source_fact_ids: Vec<FactId>,
}

/// Result of a consolidation operation.
#[derive(Debug, Clone)]
pub struct ConsolidationResult {
    /// The new consolidated facts.
    pub consolidated_facts: Vec<ConsolidatedFact>,
    /// IDs of facts that were superseded.
    pub superseded_fact_ids: Vec<FactId>,
    /// Number of input facts.
    pub original_count: usize,
    /// Number of output facts.
    pub consolidated_count: usize,
}

/// Record of a completed consolidation for the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationAuditRecord {
    /// Unique audit ID.
    pub id: String,
    /// What triggered this consolidation.
    pub trigger_type: String,
    /// Entity or cluster ID that triggered it.
    pub trigger_id: String,
    /// Number of original facts.
    pub original_count: usize,
    /// Number of consolidated facts.
    pub consolidated_count: usize,
    /// JSON array of original fact IDs.
    pub original_fact_ids: String,
    /// JSON array of consolidated fact IDs.
    pub consolidated_fact_ids: String,
    /// When consolidation was performed.
    pub consolidated_at: String,
}

// ---------------------------------------------------------------------------
// LLM provider trait
// ---------------------------------------------------------------------------

/// Minimal LLM interface for fact consolidation.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the configured LLM provider.
pub trait ConsolidationProvider: Send + Sync {
    /// Send a consolidation prompt and return the raw response.
    fn consolidate(
        &self,
        system: &str,
        user_message: &str,
    ) -> Result<String, ConsolidationError>;
}

// ---------------------------------------------------------------------------
// Consolidation prompt
// ---------------------------------------------------------------------------

/// Build the system prompt for the consolidation LLM call.
#[must_use]
pub fn consolidation_system_prompt() -> &'static str {
    r#"You are a knowledge consolidation engine. Given a set of related facts about the same topic, consolidate them into the essential knowledge.

Rules:
- Preserve important details and nuances
- Eliminate redundancy and repetition
- Resolve contradictions (prefer more recent, higher confidence)
- Each output fact should be self-contained (understandable without context)
- Output fewer facts than input (aim for 30-50% compression)

Output consolidated facts as a JSON array:
[
  {"content": "...", "entities": ["..."], "relationships": [{"from": "...", "to": "...", "type": "..."}]}
]

Output ONLY the JSON array, no other text."#
}

/// Build the user message containing the facts to consolidate.
#[must_use]
pub fn consolidation_user_message(facts: &[(FactId, String, f64, String)]) -> String {
    use std::fmt::Write as _;
    let mut msg = format!("Input facts ({} total):\n\n", facts.len());
    for (i, (id, content, confidence, recorded_at)) in facts.iter().enumerate() {
        let _ = writeln!(
            msg,
            "{}. [id={}, confidence={:.2}, recorded={}] {}",
            i + 1,
            id,
            confidence,
            recorded_at,
            content,
        );
    }
    msg
}

/// Parse the LLM response into consolidated fact entries.
///
/// Expects a JSON array of objects with at least a `content` field.
pub fn parse_consolidation_response(
    response: &str,
) -> Result<Vec<LlmConsolidatedEntry>, ConsolidationError> {
    // Try to find JSON array in the response (LLM may include preamble)
    let json_str = extract_json_array(response).unwrap_or(response);
    serde_json::from_str(json_str).context(ParseResponseSnafu)
}

/// A single entry from the LLM's consolidation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConsolidatedEntry {
    /// The consolidated fact content.
    pub content: String,
    /// Entity names mentioned in this fact.
    #[serde(default)]
    pub entities: Vec<String>,
    /// Relationships mentioned in this fact.
    #[serde(default)]
    pub relationships: Vec<LlmRelationshipEntry>,
}

/// A relationship entry from the LLM's consolidation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRelationshipEntry {
    /// Source entity name.
    pub from: String,
    /// Target entity name.
    pub to: String,
    /// Relationship type.
    #[serde(rename = "type")]
    pub rel_type: String,
}

/// Extract the first JSON array from a string that may contain surrounding text.
fn extract_json_array(s: &str) -> Option<&str> {
    let start = s.find('[')?;
    // Find the matching closing bracket
    let mut depth = 0i32;
    for (i, ch) in s[start..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Candidate identification
// ---------------------------------------------------------------------------

/// Datalog query: find entities with more than N active facts older than the age gate.
///
/// Parameters: `$min_count` (Int), `$cutoff` (String — ISO 8601 timestamp),
///             `$nous_id` (String).
///
/// Returns: `[entity_id, fact_count]` sorted by `fact_count` descending.
pub const ENTITY_OVERFLOW_CANDIDATES: &str = r"
candidates[entity_id, count(fact_id)] :=
    *fact_entities{fact_id, entity_id},
    *facts{id: fact_id, valid_from, nous_id, tier, valid_to, superseded_by, is_forgotten, recorded_at},
    nous_id == $nous_id,
    is_null(superseded_by),
    is_forgotten == false,
    valid_to > $cutoff,
    recorded_at < $cutoff,
    tier != 'verified'

?[entity_id, fact_count] :=
    candidates[entity_id, fact_count],
    fact_count >= $min_count

:sort -fact_count
";

/// Datalog query: find community clusters with more than N active facts older than the age gate.
///
/// Parameters: `$min_count` (Int), `$cutoff` (String — ISO 8601 timestamp),
///             `$nous_id` (String).
///
/// Returns: `[cluster_id, fact_count]` sorted by `fact_count` descending.
pub const COMMUNITY_OVERFLOW_CANDIDATES: &str = r"
candidates[cluster_id, count(fact_id)] :=
    *graph_scores{entity_id, score_type: 'louvain', cluster_id},
    *fact_entities{fact_id, entity_id},
    *facts{id: fact_id, valid_from, nous_id, tier, valid_to, superseded_by, is_forgotten, recorded_at},
    nous_id == $nous_id,
    is_null(superseded_by),
    is_forgotten == false,
    valid_to > $cutoff,
    recorded_at < $cutoff,
    tier != 'verified'

?[cluster_id, fact_count] :=
    candidates[cluster_id, fact_count],
    fact_count >= $min_count

:sort -fact_count
";

/// Datalog query: gather eligible fact IDs for an entity.
///
/// Parameters: `$entity_id` (String), `$cutoff` (String), `$nous_id` (String).
/// Returns: `[fact_id, content, confidence, recorded_at]`.
pub const ENTITY_FACTS_FOR_CONSOLIDATION: &str = r"
?[fact_id, content, confidence, recorded_at] :=
    *fact_entities{fact_id, entity_id: $entity_id},
    *facts{id: fact_id, content, confidence, nous_id, tier, valid_to, superseded_by, is_forgotten, recorded_at},
    nous_id == $nous_id,
    is_null(superseded_by),
    is_forgotten == false,
    valid_to > $cutoff,
    recorded_at < $cutoff,
    tier != 'verified'

:sort -confidence
";

/// Datalog query: gather eligible fact IDs for a community cluster.
///
/// Parameters: `$cluster_id` (Int), `$cutoff` (String), `$nous_id` (String).
/// Returns: `[fact_id, content, confidence, recorded_at]`.
pub const CLUSTER_FACTS_FOR_CONSOLIDATION: &str = r"
?[fact_id, content, confidence, recorded_at] :=
    *graph_scores{entity_id, score_type: 'louvain', cluster_id: $cluster_id},
    *fact_entities{fact_id, entity_id},
    *facts{id: fact_id, content, confidence, nous_id, tier, valid_to, superseded_by, is_forgotten, recorded_at},
    nous_id == $nous_id,
    is_null(superseded_by),
    is_forgotten == false,
    valid_to > $cutoff,
    recorded_at < $cutoff,
    tier != 'verified'

:sort -confidence
";

// ---------------------------------------------------------------------------
// Audit DDL
// ---------------------------------------------------------------------------

/// Datalog DDL for the `consolidation_audit` relation.
pub const CONSOLIDATION_AUDIT_DDL: &str = r":create consolidation_audit {
    id: String =>
    trigger_type: String,
    trigger_id: String,
    original_count: Int,
    consolidated_count: Int,
    original_fact_ids: String,
    consolidated_fact_ids: String,
    consolidated_at: String
}";

// ---------------------------------------------------------------------------
// Engine integration (requires mneme-engine feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "mneme-engine")]
mod engine_impl {
    use crate::consolidation::{
        age_cutoff, batch_facts, consolidation_system_prompt, consolidation_user_message,
        parse_consolidation_response, ConsolidatedFact, ConsolidationAuditRecord,
        ConsolidationCandidate, ConsolidationConfig, ConsolidationError, ConsolidationProvider,
        ConsolidationResult, ConsolidationTrigger, RateLimitedSnafu, StoreSnafu,
        CLUSTER_FACTS_FOR_CONSOLIDATION, COMMUNITY_OVERFLOW_CANDIDATES,
        CONSOLIDATION_AUDIT_DDL, ENTITY_FACTS_FOR_CONSOLIDATION, ENTITY_OVERFLOW_CANDIDATES,
    };
    use crate::engine::DataValue;
    use crate::id::{EntityId, FactId};
    use crate::knowledge::EpistemicTier;
    use crate::knowledge_store::KnowledgeStore;
    use std::collections::BTreeMap;
    use tracing::instrument;

    /// Convert a non-negative `i64` from a Datalog row to `usize`.
    fn i64_as_usize(v: i64) -> usize {
        v.try_into().unwrap_or(0)
    }

    impl KnowledgeStore {
        /// Initialize the `consolidation_audit` relation. Called during schema setup.
        pub fn init_consolidation_audit(&self) -> crate::error::Result<()> {
            self.run_mut_query(CONSOLIDATION_AUDIT_DDL, BTreeMap::new())?;
            Ok(())
        }

        /// Find entity-overflow consolidation candidates.
        #[instrument(skip(self))]
        #[expect(
            clippy::cast_possible_wrap,
            reason = "entity_fact_threshold is small (typically 10), fits i64"
        )]
        pub fn find_entity_overflow_candidates(
            &self,
            nous_id: &str,
            config: &ConsolidationConfig,
        ) -> Result<Vec<ConsolidationCandidate>, ConsolidationError> {
            let cutoff = age_cutoff(config.min_age_days);
            let mut params = BTreeMap::new();
            params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
            params.insert(
                "min_count".to_owned(),
                DataValue::from(config.entity_fact_threshold as i64),
            );
            params.insert("cutoff".to_owned(), DataValue::Str(cutoff.clone().into()));

            let result = self
                .run_query(ENTITY_OVERFLOW_CANDIDATES, params)
                .map_err(|e| StoreSnafu { message: e.to_string() }.build())?;

            let mut candidates = Vec::new();
            for row in &result.rows {
                let entity_id_str = row[0].get_str().unwrap_or_default();
                let fact_count = i64_as_usize(row[1].get_int().unwrap_or(0));
                let entity_id = EntityId::from(entity_id_str);

                let facts = self
                    .gather_entity_facts(nous_id, &entity_id, &cutoff)
                    .map_err(|e| StoreSnafu { message: e.to_string() }.build())?;

                let fact_ids: Vec<FactId> = facts.iter().map(|(id, _, _, _)| id.clone()).collect();

                candidates.push(ConsolidationCandidate {
                    trigger: ConsolidationTrigger::EntityOverflow {
                        entity_id: entity_id.clone(),
                        fact_count,
                    },
                    fact_ids,
                    fact_count,
                    entity_id: Some(entity_id),
                    cluster_id: None,
                });
            }
            Ok(candidates)
        }

        /// Find community-overflow consolidation candidates.
        #[instrument(skip(self))]
        #[expect(
            clippy::cast_possible_wrap,
            reason = "community_fact_threshold is small (typically 20), fits i64"
        )]
        pub fn find_community_overflow_candidates(
            &self,
            nous_id: &str,
            config: &ConsolidationConfig,
        ) -> Result<Vec<ConsolidationCandidate>, ConsolidationError> {
            let cutoff = age_cutoff(config.min_age_days);
            let mut params = BTreeMap::new();
            params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
            params.insert(
                "min_count".to_owned(),
                DataValue::from(config.community_fact_threshold as i64),
            );
            params.insert("cutoff".to_owned(), DataValue::Str(cutoff.clone().into()));

            let result = self
                .run_query(COMMUNITY_OVERFLOW_CANDIDATES, params)
                .map_err(|e| StoreSnafu { message: e.to_string() }.build())?;

            let mut candidates = Vec::new();
            for row in &result.rows {
                let cluster_id = row[0].get_int().unwrap_or(-1);
                let fact_count = i64_as_usize(row[1].get_int().unwrap_or(0));

                let facts = self
                    .gather_cluster_facts(nous_id, cluster_id, &cutoff)
                    .map_err(|e| StoreSnafu { message: e.to_string() }.build())?;

                let fact_ids: Vec<FactId> = facts.iter().map(|(id, _, _, _)| id.clone()).collect();

                candidates.push(ConsolidationCandidate {
                    trigger: ConsolidationTrigger::CommunityOverflow {
                        cluster_id,
                        fact_count,
                    },
                    fact_ids,
                    fact_count,
                    entity_id: None,
                    cluster_id: Some(cluster_id),
                });
            }
            Ok(candidates)
        }

        /// Gather eligible facts for an entity.
        fn gather_entity_facts(
            &self,
            nous_id: &str,
            entity_id: &EntityId,
            cutoff: &str,
        ) -> crate::error::Result<Vec<(FactId, String, f64, String)>> {
            let mut params = BTreeMap::new();
            params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
            params.insert(
                "entity_id".to_owned(),
                DataValue::Str(entity_id.as_str().into()),
            );
            params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));

            let result = self.run_query(ENTITY_FACTS_FOR_CONSOLIDATION, params)?;
            Ok(parse_fact_rows(&result.rows))
        }

        /// Gather eligible facts for a community cluster.
        fn gather_cluster_facts(
            &self,
            nous_id: &str,
            cluster_id: i64,
            cutoff: &str,
        ) -> crate::error::Result<Vec<(FactId, String, f64, String)>> {
            let mut params = BTreeMap::new();
            params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
            params.insert("cluster_id".to_owned(), DataValue::from(cluster_id));
            params.insert("cutoff".to_owned(), DataValue::Str(cutoff.into()));

            let result = self.run_query(CLUSTER_FACTS_FOR_CONSOLIDATION, params)?;
            Ok(parse_fact_rows(&result.rows))
        }

        /// Execute a consolidation: insert new facts, supersede originals, record audit.
        ///
        /// If `dry_run` is true, returns the proposed result without mutations.
        #[instrument(skip(self, provider, candidate))]
        pub fn execute_consolidation(
            &self,
            provider: &dyn ConsolidationProvider,
            candidate: &ConsolidationCandidate,
            nous_id: &str,
            config: &ConsolidationConfig,
            dry_run: bool,
        ) -> Result<ConsolidationResult, ConsolidationError> {
            let cutoff = age_cutoff(config.min_age_days);
            let facts = match &candidate.trigger {
                ConsolidationTrigger::EntityOverflow { entity_id, .. } => self
                    .gather_entity_facts(nous_id, entity_id, &cutoff)
                    .map_err(|e| StoreSnafu { message: e.to_string() }.build())?,
                ConsolidationTrigger::CommunityOverflow { cluster_id, .. } => self
                    .gather_cluster_facts(nous_id, *cluster_id, &cutoff)
                    .map_err(|e| StoreSnafu { message: e.to_string() }.build())?,
            };

            if facts.is_empty() {
                return Ok(ConsolidationResult {
                    consolidated_facts: Vec::new(),
                    superseded_fact_ids: Vec::new(),
                    original_count: 0,
                    consolidated_count: 0,
                });
            }

            let result = run_llm_consolidation(provider, &facts, config)?;

            if dry_run {
                return Ok(result);
            }

            let new_fact_ids = self.persist_consolidated_facts(&result, nous_id)?;
            self.supersede_originals(&result, &new_fact_ids)?;
            self.write_audit_record(candidate, &result, &new_fact_ids)?;

            Ok(result)
        }

        /// Insert consolidated facts into the store.
        fn persist_consolidated_facts(
            &self,
            result: &ConsolidationResult,
            nous_id: &str,
        ) -> Result<Vec<FactId>, ConsolidationError> {
            let now = jiff::Timestamp::now();
            let far_future = crate::knowledge::far_future();
            let mut new_fact_ids = Vec::new();

            for consolidated in &result.consolidated_facts {
                let new_id = FactId::from(ulid::Ulid::new().to_string());
                let fact = crate::knowledge::Fact {
                    id: new_id.clone(),
                    nous_id: nous_id.to_owned(),
                    content: consolidated.content.clone(),
                    confidence: consolidated.confidence,
                    tier: EpistemicTier::Inferred,
                    valid_from: now,
                    valid_to: far_future,
                    superseded_by: None,
                    source_session_id: None,
                    recorded_at: now,
                    access_count: 0,
                    last_accessed_at: None,
                    stability_hours: crate::knowledge::FactType::Observation
                        .base_stability_hours(),
                    fact_type: "observation".to_owned(),
                    is_forgotten: false,
                    forgotten_at: None,
                    forget_reason: None,
                };
                self.insert_fact(&fact)
                    .map_err(|e| StoreSnafu { message: e.to_string() }.build())?;
                new_fact_ids.push(new_id);
            }
            Ok(new_fact_ids)
        }

        /// Mark original facts as superseded.
        fn supersede_originals(
            &self,
            result: &ConsolidationResult,
            new_fact_ids: &[FactId],
        ) -> Result<(), ConsolidationError> {
            let now_str = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
            let superseding_id = new_fact_ids
                .first()
                .map(FactId::as_str)
                .unwrap_or_default();

            for original_id in &result.superseded_fact_ids {
                self.supersede_fact_by_id(original_id, superseding_id, &now_str)
                    .map_err(|e| StoreSnafu { message: e.to_string() }.build())?;
            }
            Ok(())
        }

        /// Write an audit trail record for a consolidation.
        fn write_audit_record(
            &self,
            candidate: &ConsolidationCandidate,
            result: &ConsolidationResult,
            new_fact_ids: &[FactId],
        ) -> Result<(), ConsolidationError> {
            let now_str = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
            let audit_id = ulid::Ulid::new().to_string();
            let original_ids_json = serde_json::to_string(
                &result
                    .superseded_fact_ids
                    .iter()
                    .map(FactId::as_str)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".to_owned());
            let consolidated_ids_json = serde_json::to_string(
                &new_fact_ids.iter().map(FactId::as_str).collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".to_owned());

            self.record_consolidation_audit(&ConsolidationAuditRecord {
                id: audit_id,
                trigger_type: candidate.trigger.trigger_type().to_owned(),
                trigger_id: candidate.trigger.trigger_id(),
                original_count: result.original_count,
                consolidated_count: result.consolidated_count,
                original_fact_ids: original_ids_json,
                consolidated_fact_ids: consolidated_ids_json,
                consolidated_at: now_str,
            })
            .map_err(|e| StoreSnafu { message: e.to_string() }.build())
        }

        /// Mark a fact as superseded by ID, setting `valid_to` and `superseded_by`.
        fn supersede_fact_by_id(
            &self,
            fact_id: &FactId,
            superseding_id: &str,
            now: &str,
        ) -> crate::error::Result<()> {
            let script = r"
?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by,
   source_session_id, recorded_at, access_count, last_accessed_at,
   stability_hours, fact_type, is_forgotten, forgotten_at, forget_reason] :=
    *facts{id, valid_from, content, nous_id, confidence, tier,
           source_session_id, recorded_at, access_count, last_accessed_at,
           stability_hours, fact_type, is_forgotten, forgotten_at, forget_reason},
    id = $id,
    valid_to = $now,
    superseded_by = $superseding_id

:put facts {id, valid_from => content, nous_id, confidence, tier, valid_to,
            superseded_by, source_session_id, recorded_at, access_count,
            last_accessed_at, stability_hours, fact_type, is_forgotten,
            forgotten_at, forget_reason}
";
            let mut params = BTreeMap::new();
            params.insert("id".to_owned(), DataValue::Str(fact_id.as_str().into()));
            params.insert("now".to_owned(), DataValue::Str(now.into()));
            params.insert(
                "superseding_id".to_owned(),
                DataValue::Str(superseding_id.into()),
            );
            self.run_mut_query(script, params)?;
            Ok(())
        }

        /// Record a consolidation audit entry.
        #[expect(
            clippy::cast_possible_wrap,
            reason = "fact counts are small, well within i64 range"
        )]
        fn record_consolidation_audit(
            &self,
            record: &ConsolidationAuditRecord,
        ) -> crate::error::Result<()> {
            let script = r"
?[id, trigger_type, trigger_id, original_count, consolidated_count,
   original_fact_ids, consolidated_fact_ids, consolidated_at] <-
    [[$id, $trigger_type, $trigger_id, $original_count, $consolidated_count,
      $original_fact_ids, $consolidated_fact_ids, $consolidated_at]]

:put consolidation_audit {id => trigger_type, trigger_id, original_count,
                          consolidated_count, original_fact_ids,
                          consolidated_fact_ids, consolidated_at}
";
            let mut params = BTreeMap::new();
            params.insert("id".to_owned(), DataValue::Str(record.id.clone().into()));
            params.insert(
                "trigger_type".to_owned(),
                DataValue::Str(record.trigger_type.clone().into()),
            );
            params.insert(
                "trigger_id".to_owned(),
                DataValue::Str(record.trigger_id.clone().into()),
            );
            params.insert(
                "original_count".to_owned(),
                DataValue::from(record.original_count as i64),
            );
            params.insert(
                "consolidated_count".to_owned(),
                DataValue::from(record.consolidated_count as i64),
            );
            params.insert(
                "original_fact_ids".to_owned(),
                DataValue::Str(record.original_fact_ids.clone().into()),
            );
            params.insert(
                "consolidated_fact_ids".to_owned(),
                DataValue::Str(record.consolidated_fact_ids.clone().into()),
            );
            params.insert(
                "consolidated_at".to_owned(),
                DataValue::Str(record.consolidated_at.clone().into()),
            );
            self.run_mut_query(script, params)?;
            Ok(())
        }

        /// Query the last consolidation timestamp from the audit trail.
        pub fn last_consolidation_time(
            &self,
            _nous_id: &str,
        ) -> Result<Option<String>, ConsolidationError> {
            let script = r"
?[consolidated_at] := *consolidation_audit{consolidated_at}
:sort -consolidated_at
:limit 1
";
            let result = self
                .run_query(script, BTreeMap::new())
                .map_err(|e| StoreSnafu { message: e.to_string() }.build())?;

            if let Some(row) = result.rows.first() {
                Ok(Some(row[0].get_str().unwrap_or_default().to_owned()))
            } else {
                Ok(None)
            }
        }

        /// Run a full consolidation cycle for a nous.
        ///
        /// 1. Check rate limit
        /// 2. Find entity and community overflow candidates
        /// 3. Execute consolidation for each candidate
        ///
        /// If `dry_run` is true, reports candidates and proposed consolidations
        /// without executing mutations.
        #[instrument(skip(self, provider))]
        pub fn consolidate_knowledge(
            &self,
            provider: &dyn ConsolidationProvider,
            nous_id: &str,
            config: &ConsolidationConfig,
            dry_run: bool,
        ) -> Result<Vec<ConsolidationResult>, ConsolidationError> {
            if !dry_run {
                self.check_rate_limit(nous_id, config)?;
            }

            let mut results = Vec::new();

            for candidate in &self.find_entity_overflow_candidates(nous_id, config)? {
                results.push(self.execute_consolidation(
                    provider, candidate, nous_id, config, dry_run,
                )?);
            }

            for candidate in &self.find_community_overflow_candidates(nous_id, config)? {
                results.push(self.execute_consolidation(
                    provider, candidate, nous_id, config, dry_run,
                )?);
            }

            Ok(results)
        }

        /// Check whether the rate limit allows another consolidation cycle.
        fn check_rate_limit(
            &self,
            nous_id: &str,
            config: &ConsolidationConfig,
        ) -> Result<(), ConsolidationError> {
            if let Some(last_time) = self.last_consolidation_time(nous_id)? {
                if let Some(last_ts) = crate::knowledge::parse_timestamp(&last_time) {
                    let now = jiff::Timestamp::now();
                    if let Ok(span) = now.since(last_ts) {
                        let total_minutes =
                            i64::from(span.get_hours()) * 60 + span.get_minutes();
                        #[expect(
                            clippy::cast_precision_loss,
                            reason = "elapsed minutes as f64 is fine for rate limiting"
                        )]
                        let elapsed_hours = total_minutes as f64 / 60.0;
                        if elapsed_hours < config.rate_limit_hours {
                            return Err(RateLimitedSnafu {
                                elapsed_hours,
                                min_hours: config.rate_limit_hours,
                            }
                            .build());
                        }
                    }
                }
            }
            Ok(())
        }
    }

    /// Run the LLM consolidation prompt across batches and collect results.
    fn run_llm_consolidation(
        provider: &dyn ConsolidationProvider,
        facts: &[(FactId, String, f64, String)],
        config: &ConsolidationConfig,
    ) -> Result<ConsolidationResult, ConsolidationError> {
        let batches = batch_facts(facts, config.batch_limit);
        let mut all_consolidated = Vec::new();
        let mut all_superseded = Vec::new();

        for batch in &batches {
            let system = consolidation_system_prompt();
            let user_msg = consolidation_user_message(batch);

            let response = provider.consolidate(system, &user_msg)?;
            let entries = parse_consolidation_response(&response)?;

            let batch_fact_ids: Vec<FactId> =
                batch.iter().map(|(id, _, _, _)| id.clone()).collect();

            for entry in entries {
                all_consolidated.push(ConsolidatedFact {
                    content: entry.content,
                    confidence: 0.95,
                    tier: "inferred".to_owned(),
                    source_fact_ids: batch_fact_ids.clone(),
                });
            }
            all_superseded.extend(batch_fact_ids);
        }

        Ok(ConsolidationResult {
            original_count: facts.len(),
            consolidated_count: all_consolidated.len(),
            consolidated_facts: all_consolidated,
            superseded_fact_ids: all_superseded,
        })
    }

    /// Parse fact rows from query results.
    fn parse_fact_rows(rows: &[Vec<DataValue>]) -> Vec<(FactId, String, f64, String)> {
        rows.iter()
            .map(|row| {
                let id = FactId::from(row[0].get_str().unwrap_or_default());
                let content = row[1].get_str().unwrap_or_default().to_owned();
                let confidence = row[2].get_float().unwrap_or(0.0);
                let recorded_at = row[3].get_str().unwrap_or_default().to_owned();
                (id, content, confidence, recorded_at)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the age cutoff timestamp (now - `min_age_days`).
#[cfg(feature = "mneme-engine")]
pub(crate) fn age_cutoff(min_age_days: u32) -> String {
    let now = jiff::Timestamp::now();
    let cutoff = now
        .checked_sub(jiff::SignedDuration::from_hours(i64::from(min_age_days) * 24))
        .unwrap_or(now);
    crate::knowledge::format_timestamp(&cutoff)
}

/// Split facts into batches of at most `batch_limit`.
#[cfg(any(feature = "mneme-engine", test))]
pub(crate) fn batch_facts(
    facts: &[(FactId, String, f64, String)],
    batch_limit: usize,
) -> Vec<Vec<(FactId, String, f64, String)>> {
    facts
        .chunks(batch_limit)
        .map(<[(FactId, String, f64, String)]>::to_vec)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Mock provider ----

    struct MockConsolidationProvider {
        response: String,
    }

    impl MockConsolidationProvider {
        fn new(response: &str) -> Self {
            Self {
                response: response.to_owned(),
            }
        }
    }

    impl ConsolidationProvider for MockConsolidationProvider {
        fn consolidate(
            &self,
            _system: &str,
            _user_message: &str,
        ) -> Result<String, ConsolidationError> {
            Ok(self.response.clone())
        }
    }

    struct FailingProvider;

    impl ConsolidationProvider for FailingProvider {
        fn consolidate(
            &self,
            _system: &str,
            _user_message: &str,
        ) -> Result<String, ConsolidationError> {
            Err(LlmCallSnafu {
                message: "mock failure",
            }
            .build())
        }
    }

    // ---- Unit tests ----

    #[test]
    fn consolidation_config_defaults() {
        let config = ConsolidationConfig::default();
        assert_eq!(config.entity_fact_threshold, 10);
        assert_eq!(config.community_fact_threshold, 20);
        assert_eq!(config.min_age_days, 7);
        assert_eq!(config.batch_limit, 50);
        assert!((config.rate_limit_hours - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn trigger_type_labels() {
        let entity = ConsolidationTrigger::EntityOverflow {
            entity_id: EntityId::from("e-1"),
            fact_count: 15,
        };
        assert_eq!(entity.trigger_type(), "entity_overflow");
        assert_eq!(entity.trigger_id(), "e-1");

        let community = ConsolidationTrigger::CommunityOverflow {
            cluster_id: 42,
            fact_count: 25,
        };
        assert_eq!(community.trigger_type(), "community_overflow");
        assert_eq!(community.trigger_id(), "42");
    }

    #[test]
    fn parse_valid_consolidation_response() {
        let response = r#"[
            {"content": "Alice works at Acme Corp as a senior engineer", "entities": ["Alice", "Acme Corp"], "relationships": [{"from": "Alice", "to": "Acme Corp", "type": "WORKS_AT"}]},
            {"content": "Alice prefers Rust for backend development", "entities": ["Alice", "Rust"], "relationships": []}
        ]"#;
        let entries = parse_consolidation_response(response).expect("parse succeeds");
        assert_eq!(entries.len(), 2);
        assert!(entries[0].content.contains("Alice works at Acme Corp"));
        assert_eq!(entries[0].entities.len(), 2);
        assert_eq!(entries[0].relationships.len(), 1);
        assert_eq!(entries[0].relationships[0].rel_type, "WORKS_AT");
    }

    #[test]
    fn parse_response_with_preamble() {
        let response = r#"Here are the consolidated facts:
[{"content": "Bob is a data scientist", "entities": ["Bob"]}]
Some trailing text."#;
        let entries = parse_consolidation_response(response).expect("parse succeeds");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Bob is a data scientist");
    }

    #[test]
    fn parse_invalid_response_fails() {
        let response = "This is not JSON at all";
        assert!(parse_consolidation_response(response).is_err());
    }

    #[test]
    fn extract_json_array_finds_array() {
        let text = "prefix [1, 2, 3] suffix";
        assert_eq!(extract_json_array(text), Some("[1, 2, 3]"));
    }

    #[test]
    fn extract_json_array_nested() {
        let text = r#"[{"a": [1, 2]}, {"b": 3}]"#;
        assert_eq!(extract_json_array(text), Some(text));
    }

    #[test]
    fn extract_json_array_none() {
        assert_eq!(extract_json_array("no array here"), None);
    }

    #[test]
    fn user_message_formatting() {
        let facts = vec![
            (
                FactId::from("f-1"),
                "Alice works at Acme".to_owned(),
                0.9,
                "2026-01-01T00:00:00Z".to_owned(),
            ),
            (
                FactId::from("f-2"),
                "Alice likes Rust".to_owned(),
                0.85,
                "2026-01-02T00:00:00Z".to_owned(),
            ),
        ];
        let msg = consolidation_user_message(&facts);
        assert!(msg.contains("2 total"));
        assert!(msg.contains("1. [id=f-1"));
        assert!(msg.contains("2. [id=f-2"));
        assert!(msg.contains("confidence=0.90"));
    }

    #[test]
    fn system_prompt_is_stable() {
        let prompt = consolidation_system_prompt();
        assert!(prompt.contains("knowledge consolidation engine"));
        assert!(prompt.contains("JSON array"));
        assert!(prompt.contains("30-50% compression"));
    }

    #[test]
    fn batch_facts_splits_correctly() {
        let facts: Vec<(FactId, String, f64, String)> = (0..7)
            .map(|i| {
                (
                    FactId::from(format!("f-{i}")),
                    format!("fact {i}"),
                    0.8,
                    "2026-01-01T00:00:00Z".to_owned(),
                )
            })
            .collect();

        let batches = batch_facts(&facts, 3);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[1].len(), 3);
        assert_eq!(batches[2].len(), 1);
    }

    #[test]
    fn batch_facts_single_batch() {
        let facts: Vec<(FactId, String, f64, String)> = (0..5)
            .map(|i| {
                (
                    FactId::from(format!("f-{i}")),
                    format!("fact {i}"),
                    0.8,
                    "2026-01-01T00:00:00Z".to_owned(),
                )
            })
            .collect();

        let batches = batch_facts(&facts, 50);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 5);
    }

    #[test]
    fn consolidated_fact_serde_roundtrip() {
        let fact = ConsolidatedFact {
            content: "Alice is a senior engineer at Acme Corp".to_owned(),
            confidence: 0.95,
            tier: "inferred".to_owned(),
            source_fact_ids: vec![FactId::from("f-1"), FactId::from("f-2")],
        };
        let json = serde_json::to_string(&fact).expect("serialize");
        let back: ConsolidatedFact = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(fact.content, back.content);
        assert!((fact.confidence - back.confidence).abs() < f64::EPSILON);
        assert_eq!(fact.source_fact_ids.len(), back.source_fact_ids.len());
    }

    #[test]
    fn consolidation_trigger_serde_roundtrip() {
        let trigger = ConsolidationTrigger::EntityOverflow {
            entity_id: EntityId::from("e-alice"),
            fact_count: 15,
        };
        let json = serde_json::to_string(&trigger).expect("serialize");
        let back: ConsolidationTrigger = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(trigger, back);
    }

    #[test]
    fn mock_provider_returns_response() {
        let provider = MockConsolidationProvider::new(r#"[{"content": "test"}]"#);
        let result = provider.consolidate("system", "user").expect("should succeed");
        assert!(result.contains("test"));
    }

    #[test]
    fn failing_provider_returns_error() {
        let provider = FailingProvider;
        let result = provider.consolidate("system", "user");
        assert!(result.is_err());
    }

    #[test]
    fn audit_record_serde_roundtrip() {
        let record = ConsolidationAuditRecord {
            id: "audit-1".to_owned(),
            trigger_type: "entity_overflow".to_owned(),
            trigger_id: "e-1".to_owned(),
            original_count: 15,
            consolidated_count: 5,
            original_fact_ids: r#"["f-1","f-2"]"#.to_owned(),
            consolidated_fact_ids: r#"["f-new-1"]"#.to_owned(),
            consolidated_at: "2026-03-01T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&record).expect("serialize");
        let back: ConsolidationAuditRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(record.id, back.id);
        assert_eq!(record.original_count, back.original_count);
    }
}

// ---------------------------------------------------------------------------
// Engine integration tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "mneme-engine"))]
mod engine_tests {
    use super::{
        batch_facts, ConsolidationConfig, ConsolidationError, ConsolidationProvider,
    };
    use crate::id::{EntityId, FactId};
    use crate::knowledge::{self, EpistemicTier, Fact};
    use crate::knowledge_store::KnowledgeStore;
    use std::sync::Arc;

    fn test_store() -> Arc<KnowledgeStore> {
        KnowledgeStore::open_mem().expect("open_mem")
    }

    fn make_fact(id: &str, nous_id: &str, content: &str, tier: EpistemicTier, days_ago: i64) -> Fact {
        let now = jiff::Timestamp::now();
        let recorded = now
            .checked_sub(jiff::SignedDuration::from_hours(days_ago * 24))
            .unwrap_or(now);
        Fact {
            id: FactId::from(id),
            nous_id: nous_id.to_owned(),
            content: content.to_owned(),
            confidence: 0.8,
            tier,
            valid_from: recorded,
            valid_to: knowledge::far_future(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: recorded,
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: "observation".to_owned(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    fn insert_fact_with_entity(
        store: &KnowledgeStore,
        fact: &Fact,
        entity_id: &str,
    ) {
        store.insert_fact(fact).expect("insert fact");
        store
            .insert_fact_entity(
                &fact.id,
                &EntityId::from(entity_id),
            )
            .expect("insert fact_entity");
    }

    struct MockProvider {
        response: String,
    }

    impl ConsolidationProvider for MockProvider {
        fn consolidate(
            &self,
            _system: &str,
            _user_message: &str,
        ) -> Result<String, ConsolidationError> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn entity_with_15_facts_is_candidate() {
        let store = test_store();
        let nous_id = "test-nous";

        // Create entity
        let entity = crate::knowledge::Entity {
            id: EntityId::from("e-alice"),
            name: "Alice".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        // Insert 15 facts older than 7 days
        for i in 0..15 {
            let fact = make_fact(
                &format!("f-{i}"),
                nous_id,
                &format!("Alice fact number {i}"),
                EpistemicTier::Inferred,
                14, // 14 days ago
            );
            insert_fact_with_entity(&store, &fact, "e-alice");
        }

        let config = ConsolidationConfig::default();
        let candidates = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].fact_count, 15);
        assert_eq!(candidates[0].fact_ids.len(), 15);
    }

    #[test]
    fn entity_with_5_facts_is_not_candidate() {
        let store = test_store();
        let nous_id = "test-nous";

        let entity = crate::knowledge::Entity {
            id: EntityId::from("e-bob"),
            name: "Bob".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        for i in 0..5 {
            let fact = make_fact(
                &format!("f-bob-{i}"),
                nous_id,
                &format!("Bob fact number {i}"),
                EpistemicTier::Inferred,
                14,
            );
            insert_fact_with_entity(&store, &fact, "e-bob");
        }

        let config = ConsolidationConfig::default();
        let candidates = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates");

        assert!(candidates.is_empty());
    }

    #[test]
    fn recent_facts_excluded_by_age_gate() {
        let store = test_store();
        let nous_id = "test-nous";

        let entity = crate::knowledge::Entity {
            id: EntityId::from("e-carol"),
            name: "Carol".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        // Insert 15 facts but only 1 day old
        for i in 0..15 {
            let fact = make_fact(
                &format!("f-carol-{i}"),
                nous_id,
                &format!("Carol recent fact {i}"),
                EpistemicTier::Inferred,
                1, // only 1 day ago — should be excluded
            );
            insert_fact_with_entity(&store, &fact, "e-carol");
        }

        let config = ConsolidationConfig::default();
        let candidates = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates");

        assert!(candidates.is_empty(), "recent facts should be excluded by age gate");
    }

    #[test]
    fn verified_facts_excluded_from_consolidation() {
        let store = test_store();
        let nous_id = "test-nous";

        let entity = crate::knowledge::Entity {
            id: EntityId::from("e-dave"),
            name: "Dave".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        // Insert 15 Verified facts — should not be candidates
        for i in 0..15 {
            let fact = make_fact(
                &format!("f-dave-{i}"),
                nous_id,
                &format!("Dave verified fact {i}"),
                EpistemicTier::Verified,
                14,
            );
            insert_fact_with_entity(&store, &fact, "e-dave");
        }

        let config = ConsolidationConfig::default();
        let candidates = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates");

        assert!(candidates.is_empty(), "verified facts must be excluded");
    }

    #[test]
    fn mock_llm_consolidation_produces_fewer_facts() {
        let store = test_store();
        let nous_id = "test-nous";

        let entity = crate::knowledge::Entity {
            id: EntityId::from("e-eve"),
            name: "Eve".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        for i in 0..10 {
            let fact = make_fact(
                &format!("f-eve-{i}"),
                nous_id,
                &format!("Eve fact number {i} about engineering"),
                EpistemicTier::Inferred,
                14,
            );
            insert_fact_with_entity(&store, &fact, "e-eve");
        }

        // Mock LLM returns 4 consolidated facts
        let mock_response = r#"[
            {"content": "Eve is a software engineer", "entities": ["Eve"]},
            {"content": "Eve works on backend systems", "entities": ["Eve"]},
            {"content": "Eve has extensive engineering experience", "entities": ["Eve"]},
            {"content": "Eve focuses on system reliability", "entities": ["Eve"]}
        ]"#;
        let provider = MockProvider {
            response: mock_response.to_owned(),
        };

        let config = ConsolidationConfig::default();
        let candidates = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates");
        assert_eq!(candidates.len(), 1);

        let result = store
            .execute_consolidation(&provider, &candidates[0], nous_id, &config, false)
            .expect("execute consolidation");

        assert_eq!(result.original_count, 10);
        assert_eq!(result.consolidated_count, 4);
        assert_eq!(result.superseded_fact_ids.len(), 10);
        assert_eq!(result.consolidated_facts.len(), 4);
        assert!((result.consolidated_facts[0].confidence - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn dry_run_does_not_mutate() {
        let store = test_store();
        let nous_id = "test-nous";

        let entity = crate::knowledge::Entity {
            id: EntityId::from("e-frank"),
            name: "Frank".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        for i in 0..12 {
            let fact = make_fact(
                &format!("f-frank-{i}"),
                nous_id,
                &format!("Frank fact {i}"),
                EpistemicTier::Inferred,
                14,
            );
            insert_fact_with_entity(&store, &fact, "e-frank");
        }

        let mock_response = r#"[{"content": "Frank consolidated", "entities": ["Frank"]}]"#;
        let provider = MockProvider {
            response: mock_response.to_owned(),
        };

        let config = ConsolidationConfig::default();
        let candidates = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates");

        let result = store
            .execute_consolidation(&provider, &candidates[0], nous_id, &config, true)
            .expect("dry run");

        assert_eq!(result.original_count, 12);
        assert_eq!(result.consolidated_count, 1);

        // Verify no audit record was created
        let last = store.last_consolidation_time(nous_id).expect("query audit");
        assert!(last.is_none(), "dry run must not create audit records");

        // Verify originals still exist (check that a second candidate search yields same result)
        let candidates_after = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates after dry run");
        assert_eq!(candidates_after.len(), 1, "dry run must not supersede facts");
        assert_eq!(candidates_after[0].fact_count, 12);
    }

    #[test]
    fn supersession_sets_valid_to_and_superseded_by() {
        let store = test_store();
        let nous_id = "test-nous";

        let entity = crate::knowledge::Entity {
            id: EntityId::from("e-grace"),
            name: "Grace".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        for i in 0..11 {
            let fact = make_fact(
                &format!("f-grace-{i}"),
                nous_id,
                &format!("Grace fact {i}"),
                EpistemicTier::Inferred,
                14,
            );
            insert_fact_with_entity(&store, &fact, "e-grace");
        }

        let mock_response = r#"[{"content": "Grace consolidated fact", "entities": ["Grace"]}]"#;
        let provider = MockProvider {
            response: mock_response.to_owned(),
        };

        let config = ConsolidationConfig::default();
        let candidates = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates");

        store
            .execute_consolidation(&provider, &candidates[0], nous_id, &config, false)
            .expect("execute");

        // After consolidation, the originals should no longer be candidates
        let candidates_after = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find after");
        assert!(
            candidates_after.is_empty(),
            "superseded facts should no longer appear as candidates"
        );
    }

    #[test]
    fn audit_trail_created_after_consolidation() {
        let store = test_store();
        let nous_id = "test-nous";

        let entity = crate::knowledge::Entity {
            id: EntityId::from("e-heidi"),
            name: "Heidi".to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        };
        store.insert_entity(&entity).expect("insert entity");

        for i in 0..10 {
            let fact = make_fact(
                &format!("f-heidi-{i}"),
                nous_id,
                &format!("Heidi fact {i}"),
                EpistemicTier::Inferred,
                14,
            );
            insert_fact_with_entity(&store, &fact, "e-heidi");
        }

        let mock_response = r#"[{"content": "Heidi summary", "entities": ["Heidi"]}]"#;
        let provider = MockProvider {
            response: mock_response.to_owned(),
        };

        let config = ConsolidationConfig::default();
        let candidates = store
            .find_entity_overflow_candidates(nous_id, &config)
            .expect("find candidates");

        store
            .execute_consolidation(&provider, &candidates[0], nous_id, &config, false)
            .expect("execute");

        let last_time = store
            .last_consolidation_time(nous_id)
            .expect("query audit");
        assert!(last_time.is_some(), "audit record must be created");
    }

    #[test]
    fn batch_limit_splits_large_clusters() {
        // Test that 60 facts get split into batches of 50 + 10
        let facts: Vec<(FactId, String, f64, String)> = (0..60)
            .map(|i| {
                (
                    FactId::from(format!("f-{i}")),
                    format!("fact {i}"),
                    0.8,
                    "2026-01-01T00:00:00Z".to_owned(),
                )
            })
            .collect();

        let batches = batch_facts(&facts, 50);
        assert_eq!(batches.len(), 2, "60 facts should be split into 2 batches");
        assert_eq!(batches[0].len(), 50);
        assert_eq!(batches[1].len(), 10);
    }
}
