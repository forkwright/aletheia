// Serializable data model types for the memory inspector.

use serde::{Deserialize, Serialize};

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
    pub(crate) id: String,
    #[serde(default)]
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
    pub(crate) fact_id: String,
    #[serde(default)]
    pub(crate) confidence: Option<f64>,
}

/// A similar fact result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SimilarFact {
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
