// Serializable data model types for the memory inspector.

use serde::{Deserialize, Serialize};
use skene::api::types as skene_types;

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

impl From<skene_types::Fact> for MemoryFact {
    fn from(dto: skene_types::Fact) -> Self {
        Self {
            id: dto.id,
            nous_id: dto.nous_id,
            content: dto.content,
            confidence: dto.confidence,
            tier: dto.tier,
            fact_type: dto.fact_type,
            temporal: FactTemporalMeta {
                valid_from: dto.valid_from,
                valid_to: dto.valid_to,
                recorded_at: dto.recorded_at,
                access_count: dto.access_count,
                last_accessed_at: dto.last_accessed_at.unwrap_or_default(),
                stability_hours: dto.stability_hours,
            },
            lifecycle: FactLifecycleMeta {
                superseded_by: dto.superseded_by,
                source_session_id: dto.source_session_id,
                is_forgotten: dto.is_forgotten,
                forgotten_at: dto.forgotten_at,
                forget_reason: dto.forget_reason,
            },
        }
    }
}

impl From<skene_types::EntityListItem> for MemoryEntity {
    fn from(dto: skene_types::EntityListItem) -> Self {
        Self {
            id: dto.id,
            name: dto.name,
            entity_type: dto.entity_type,
            aliases: dto.aliases,
            created_at: dto.created_at,
            updated_at: dto.updated_at,
        }
    }
}

impl From<skene_types::Relationship> for MemoryRelationship {
    fn from(dto: skene_types::Relationship) -> Self {
        Self {
            src: dto.src,
            dst: dto.dst,
            relation: dto.relation,
            weight: dto.weight,
            created_at: dto.created_at,
        }
    }
}

impl From<(skene_types::EntityRelationship, String)> for MemoryRelationship {
    fn from((dto, entity_id): (skene_types::EntityRelationship, String)) -> Self {
        let (src, dst) = match dto.direction {
            skene_types::RelationshipDirection::Outgoing => (entity_id, dto.entity_id),
            skene_types::RelationshipDirection::Incoming => (dto.entity_id, entity_id),
        };
        Self {
            src,
            dst,
            relation: dto.relationship_type,
            weight: dto.confidence,
            created_at: String::new(),
        }
    }
}

impl From<skene_types::TimelineEvent> for MemoryTimelineEvent {
    fn from(dto: skene_types::TimelineEvent) -> Self {
        Self {
            timestamp: dto.timestamp,
            event_type: dto.event_type,
            description: dto.description,
            fact_id: dto.fact_id,
            confidence: dto.confidence,
        }
    }
}

impl From<skene_types::SimilarFact> for SimilarFact {
    fn from(dto: skene_types::SimilarFact) -> Self {
        Self {
            id: dto.id,
            content: dto.content,
            similarity: dto.similarity,
        }
    }
}

impl From<skene_types::FactDetailResponse> for FactDetail {
    fn from(dto: skene_types::FactDetailResponse) -> Self {
        Self {
            fact: MemoryFact::from(dto.fact),
            relationships: dto
                .relationships
                .into_iter()
                .map(MemoryRelationship::from)
                .collect(),
            similar: dto.similar.into_iter().map(SimilarFact::from).collect(),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn memory_fact_preserves_lifecycle_and_temporal_fields() {
        let dto = skene_types::Fact {
            id: "fact_01".to_string(),
            nous_id: "agent-1".to_string(),
            content: "content".to_string(),
            fact_type: "preference".to_string(),
            scope: None,
            project_id: None,
            sensitivity: "internal".to_string(),
            visibility: "private".to_string(),
            valid_from: "2026-06-01T00:00:00Z".to_string(),
            valid_to: "9999-01-01T00:00:00Z".to_string(),
            recorded_at: "2026-06-01T12:00:00Z".to_string(),
            confidence: 0.92,
            tier: "verified".to_string(),
            source_session_id: Some("session-7".to_string()),
            stability_hours: 8760.0,
            superseded_by: Some("fact_03".to_string()),
            is_forgotten: true,
            forgotten_at: Some("2026-06-11T00:00:00Z".to_string()),
            forget_reason: Some("outdated".to_string()),
            access_count: 4,
            last_accessed_at: Some("2026-06-10T08:00:00Z".to_string()),
        };
        let fact = MemoryFact::from(dto);
        assert_eq!(fact.id, "fact_01");
        assert_eq!(fact.confidence, 0.92);
        assert!(fact.lifecycle.is_forgotten);
        assert_eq!(fact.lifecycle.forget_reason.as_deref(), Some("outdated"));
        assert_eq!(fact.temporal.access_count, 4);
        assert_eq!(fact.temporal.stability_hours, 8760.0);
    }

    #[test]
    fn memory_relationship_resolves_entity_direction() {
        let dto = skene_types::EntityRelationship {
            id: "e1:e2:depends_on:ts".to_string(),
            entity_id: "e2".to_string(),
            entity_name: "Beta".to_string(),
            relationship_type: "depends_on".to_string(),
            direction: skene_types::RelationshipDirection::Outgoing,
            confidence: 0.85,
        };
        let rel = MemoryRelationship::from((dto, "e1".to_string()));
        assert_eq!(rel.src, "e1");
        assert_eq!(rel.dst, "e2");
        assert_eq!(rel.relation, "depends_on");
        assert!((rel.weight - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn fact_detail_converts_relationships_and_similar() {
        let dto = skene_types::FactDetailResponse {
            fact: skene_types::Fact {
                id: "f1".to_string(),
                nous_id: String::new(),
                content: "fact".to_string(),
                fact_type: "observation".to_string(),
                scope: None,
                project_id: None,
                sensitivity: String::new(),
                visibility: String::new(),
                valid_from: String::new(),
                valid_to: String::new(),
                recorded_at: String::new(),
                confidence: 0.8,
                tier: "verified".to_string(),
                source_session_id: None,
                stability_hours: 0.0,
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
                access_count: 0,
                last_accessed_at: None,
            },
            relationships: vec![skene_types::Relationship {
                src: "e1".to_string(),
                dst: "e2".to_string(),
                relation: "uses".to_string(),
                weight: 0.7,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }],
            similar: vec![skene_types::SimilarFact {
                id: "f2".to_string(),
                content: "similar".to_string(),
                similarity: 0.6,
            }],
        };
        let detail = FactDetail::from(dto);
        assert_eq!(detail.fact.id, "f1");
        assert_eq!(detail.relationships.len(), 1);
        assert_eq!(detail.relationships[0].src, "e1");
        assert_eq!(detail.similar.len(), 1);
        assert_eq!(detail.similar[0].id, "f2");
    }
}
