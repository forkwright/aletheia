//! LLM-driven fact consolidation for knowledge maintenance.
//!
//! When an entity has 10+ facts or a community cluster exceeds 20 facts,
//! the system sends them to an LLM for summarization into fewer, higher-quality
//! facts. Originals are superseded, not deleted: full provenance is preserved
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

#[cfg(feature = "mneme-engine")]
mod engine;

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

    /// Rate limit exceeded: too soon since the last consolidation cycle.
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

/// Why a consolidation was triggered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConsolidationTrigger {
    /// An entity accumulated more than the threshold of active facts.
    EntityOverflow {
        entity_id: EntityId,
        fact_count: usize,
    },
    /// A Louvain community cluster accumulated more than the threshold of active facts.
    CommunityOverflow { cluster_id: i64, fact_count: usize },
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

/// Minimal LLM interface for fact consolidation.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the configured LLM provider.
pub trait ConsolidationProvider: Send + Sync {
    /// Send a consolidation prompt and return the raw response.
    fn consolidate(&self, system: &str, user_message: &str) -> Result<String, ConsolidationError>;
}

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
            // NOTE: non-bracket character, no depth change
            _ => {}
        }
    }
    None
}

/// Datalog query: find entities with more than N active facts older than the age gate.
///
/// Parameters: `$min_count` (Int), `$cutoff` (String: ISO 8601 timestamp),
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
/// Parameters: `$min_count` (Int), `$cutoff` (String: ISO 8601 timestamp),
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

/// Compute the age cutoff timestamp (now - `min_age_days`).
#[cfg(feature = "mneme-engine")]
pub(crate) fn age_cutoff(min_age_days: u32) -> String {
    let now = jiff::Timestamp::now();
    #[expect(
        clippy::expect_used,
        reason = "overflow only occurs for extreme min_age_days values beyond practical use"
    )]
    let cutoff = now
        .checked_sub(jiff::SignedDuration::from_hours(
            i64::from(min_age_days) * 24,
        ))
        .expect("age cutoff subtraction overflows only for extreme min_age_days values");
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

#[cfg(test)]
#[path = "consolidation_tests.rs"]
mod tests;
