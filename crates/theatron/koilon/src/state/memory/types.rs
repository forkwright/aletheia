// Serializable data model types for the memory inspector.

use serde::{Deserialize, Serialize};

use crate::api::types as skene_types;

/// Temporal metadata for a memory fact (timestamps, access tracking).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FactTemporalMeta {
    #[serde(default)]
    pub(crate) valid_from: String,
    #[serde(default)]
    pub(crate) valid_to: String,
    #[serde(default)]
    pub(crate) recorded_at: String,
    #[serde(default)]
    pub(crate) access_count: u32,
    #[serde(default)]
    pub(crate) last_accessed_at: String,
    #[serde(default)]
    pub(crate) stability_hours: f64,
}

/// Lifecycle metadata for a memory fact (supersession, forgetting).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FactLifecycleMeta {
    #[serde(default)]
    pub(crate) superseded_by: Option<String>,
    #[serde(default)]
    pub(crate) source_session_id: Option<String>,
    #[serde(default)]
    pub(crate) is_forgotten: bool,
    #[serde(default)]
    pub(crate) forgotten_at: Option<String>,
    #[serde(default)]
    pub(crate) forget_reason: Option<String>,
}

/// A fact as displayed in the TUI (deserialized from API).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryFact {
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde/external-id field from API response; newtype out of scope
    pub(crate) id: String,
    #[serde(default)]
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde/external-id field from API response; newtype out of scope
    pub(crate) nous_id: String,
    pub(crate) content: String,
    pub(crate) confidence: f64,
    pub(crate) tier: String,
    #[serde(default)]
    pub(crate) fact_type: String,
    #[serde(flatten)]
    pub(crate) temporal: FactTemporalMeta,
    #[serde(flatten)]
    pub(crate) lifecycle: FactLifecycleMeta,
}

/// An entity in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MemoryEntity {
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde/external-id field from API response; newtype out of scope
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) entity_type: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    #[serde(default)]
    pub(crate) created_at: String,
    #[serde(default)]
    pub(crate) updated_at: String,
}

/// A relationship between entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MemoryRelationship {
    pub(crate) src: String,
    pub(crate) dst: String,
    pub(crate) relation: String,
    #[serde(default)]
    pub(crate) weight: f64,
    #[serde(default)]
    pub(crate) created_at: String,
}

/// A timeline event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryTimelineEvent {
    pub(crate) timestamp: String,
    pub(crate) event_type: String,
    pub(crate) description: String,
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde/external-id field from API response; newtype out of scope
    pub(crate) fact_id: String,
    #[serde(default)]
    pub(crate) confidence: Option<f64>,
}

/// A similar fact result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SimilarFact {
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde/external-id field from API response; newtype out of scope
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) similarity: f64,
}

/// An entity with computed graph statistics for the summary view.
#[derive(Debug, Clone)]
pub(crate) struct GraphEntityStat {
    pub(crate) entity: MemoryEntity,
    pub(crate) relationship_count: usize,
    pub(crate) community_id: Option<u32>,
    pub(crate) pagerank: f64,
}

/// A drift analysis suggestion (delete, review, or merge).
#[derive(Debug, Clone)]
pub(crate) struct DriftSuggestion {
    pub(crate) action: String,
    pub(crate) entity_name: String,
    pub(crate) reason: String,
}

/// A cluster of entities identified as isolated (<3 members).
#[derive(Debug, Clone)]
pub(crate) struct IsolatedCluster {
    pub(crate) entity_names: Vec<String>,
    pub(crate) size: usize,
}

/// Aggregate health metrics for the knowledge graph.
#[derive(Debug, Clone)]
pub(crate) struct GraphHealthMetrics {
    pub(crate) total_entities: usize,
    pub(crate) total_relationships: usize,
    pub(crate) orphan_count: usize,
    pub(crate) stale_count: usize,
    pub(crate) avg_cluster_size: f64,
    pub(crate) community_count: usize,
    pub(crate) isolated_cluster_count: usize,
}

impl Default for GraphHealthMetrics {
    fn default() -> Self {
        Self {
            total_entities: 0,
            total_relationships: 0,
            orphan_count: 0,
            stale_count: 0,
            avg_cluster_size: 0.0,
            community_count: 0,
            isolated_cluster_count: 0,
        }
    }
}

/// A fact related to a graph entity (for the node card).
#[derive(Debug, Clone)]
pub(crate) struct NodeCardFact {
    pub(crate) content: String,
    pub(crate) confidence: f64,
    pub(crate) tier: String,
}

/// Full detail for a selected entity (node card view).
#[derive(Debug, Clone)]
pub(crate) struct GraphNodeCard {
    pub(crate) entity: MemoryEntity,
    pub(crate) pagerank: f64,
    pub(crate) community_id: Option<u32>,
    pub(crate) relationships_grouped: Vec<(String, Vec<MemoryRelationship>)>,
    pub(crate) related_facts: Vec<NodeCardFact>,
}

/// Search result from the knowledge API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemorySearchResult {
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde/external-id field from API response; newtype out of scope
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) confidence: f64,
    pub(crate) tier: String,
    pub(crate) fact_type: String,
    pub(crate) score: f64,
}

/// Fact detail with relationships and similar facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FactDetail {
    pub(crate) fact: MemoryFact,
    #[serde(default)]
    pub(crate) relationships: Vec<MemoryRelationship>,
    #[serde(default)]
    pub(crate) similar: Vec<SimilarFact>,
}

/// Payload of a completed background graph fetch (#6815).
///
/// Carried by `Msg::MemoryGraphLoaded` from the background task back into
/// `update()`, where the graph analyses run against `&mut App`.
#[derive(Debug)]
pub(crate) struct MemoryGraphLoad {
    pub(crate) entities: Vec<MemoryEntity>,
    pub(crate) relationships: Vec<MemoryRelationship>,
    pub(crate) timeline_events: Vec<MemoryTimelineEvent>,
    /// First fetch error encountered, if any -- surfaced as a toast so a
    /// failed load is distinguishable from an empty store.
    pub(crate) error: Option<String>,
}

// WHY(#4870): explicit conversions from Skene's typed knowledge DTOs (which
// deserialize correctly against Pylon's actual wire shape) into these local
// view models, instead of deserializing the wire JSON directly into structs
// whose `#[serde(rename_all = "camelCase")]` never matched Pylon's snake_case
// fields — that mismatch silently defaulted every temporal/lifecycle field
// (timestamps always empty, `is_forgotten` always false) because every
// affected field carries `#[serde(default)]`.

impl From<skene_types::Fact> for MemoryFact {
    fn from(fact: skene_types::Fact) -> Self {
        Self {
            id: fact.id,
            nous_id: fact.nous_id,
            content: fact.content,
            confidence: fact.confidence,
            tier: fact.tier.as_str().to_string(),
            fact_type: fact.fact_type,
            temporal: FactTemporalMeta {
                valid_from: fact.valid_from,
                valid_to: fact.valid_to,
                recorded_at: fact.recorded_at,
                access_count: fact.access_count,
                last_accessed_at: fact.last_accessed_at.unwrap_or_default(),
                stability_hours: fact.stability_hours,
            },
            lifecycle: FactLifecycleMeta {
                superseded_by: fact.superseded_by,
                source_session_id: fact.source_session_id,
                is_forgotten: fact.is_forgotten,
                forgotten_at: fact.forgotten_at,
                forget_reason: fact.forget_reason,
            },
        }
    }
}

impl From<skene_types::EntityListItem> for MemoryEntity {
    fn from(entity: skene_types::EntityListItem) -> Self {
        Self {
            id: entity.id,
            name: entity.name,
            entity_type: entity.entity_type,
            aliases: entity.aliases,
            created_at: entity.created_at,
            updated_at: entity.updated_at,
        }
    }
}

impl From<skene_types::Relationship> for MemoryRelationship {
    fn from(rel: skene_types::Relationship) -> Self {
        Self {
            src: rel.src,
            dst: rel.dst,
            relation: rel.relation,
            weight: rel.weight,
            created_at: rel.created_at,
        }
    }
}

/// Reconstruct a global (src, dst) graph edge from Pylon's entity-relative
/// relationship shape. The per-entity endpoint reports the relationship from
/// the viewed entity's perspective (`direction` + the *other* entity's id) —
/// not a free-standing edge — so the viewed entity id must be supplied by
/// the caller (it already has it: it's the id used to make the request).
pub(crate) fn relationship_from_entity_relative(
    viewed_entity_id: &str,
    rel: skene_types::EntityRelationship,
) -> MemoryRelationship {
    let (src, dst) = match rel.direction {
        skene_types::RelationshipDirection::Outgoing => {
            (viewed_entity_id.to_string(), rel.entity_id)
        }
        skene_types::RelationshipDirection::Incoming => {
            (rel.entity_id, viewed_entity_id.to_string())
        }
    };
    MemoryRelationship {
        src,
        dst,
        relation: rel.relationship_type,
        weight: rel.confidence,
        created_at: String::new(),
    }
}

impl From<skene_types::TimelineEvent> for MemoryTimelineEvent {
    fn from(event: skene_types::TimelineEvent) -> Self {
        Self {
            timestamp: event.timestamp,
            event_type: event.event_type,
            description: event.description,
            fact_id: event.fact_id,
            confidence: event.confidence,
        }
    }
}

impl From<skene_types::SimilarFact> for SimilarFact {
    fn from(similar: skene_types::SimilarFact) -> Self {
        Self {
            id: similar.id,
            content: similar.content,
            similarity: similar.similarity,
        }
    }
}

impl From<skene_types::FactDetailResponse> for FactDetail {
    fn from(detail: skene_types::FactDetailResponse) -> Self {
        Self {
            fact: detail.fact.into(),
            relationships: detail.relationships.into_iter().map(Into::into).collect(),
            similar: detail.similar.into_iter().map(Into::into).collect(),
        }
    }
}
