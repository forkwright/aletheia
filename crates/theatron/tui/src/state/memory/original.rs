//! State for the memory inspector panel.

use serde::{Deserialize, Serialize};

/// Sort options for the fact browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum FactSort {
    Confidence,
    Recency,
    Created,
    AccessCount,
    FsrsReview,
}

impl FactSort {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Confidence => "confidence",
            Self::Recency => "recency",
            Self::Created => "created",
            Self::AccessCount => "access_count",
            Self::FsrsReview => "fsrs_review",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Confidence => "Confidence",
            Self::Recency => "Last Seen",
            Self::Created => "Created",
            Self::AccessCount => "Accesses",
            Self::FsrsReview => "FSRS Review",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Confidence => Self::Recency,
            Self::Recency => Self::Created,
            Self::Created => Self::AccessCount,
            Self::AccessCount => Self::FsrsReview,
            Self::FsrsReview => Self::Confidence,
        }
    }
}

/// Which sub-view of the memory inspector is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum MemoryTab {
    Facts,
    Graph,
    Drift,
    Timeline,
}

impl MemoryTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Facts => "Facts",
            Self::Graph => "Graph",
            Self::Drift => "Drift",
            Self::Timeline => "Timeline",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Facts => Self::Graph,
            Self::Graph => Self::Drift,
            Self::Drift => Self::Timeline,
            Self::Timeline => Self::Facts,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Facts => Self::Timeline,
            Self::Graph => Self::Facts,
            Self::Drift => Self::Graph,
            Self::Timeline => Self::Drift,
        }
    }
}

/// Sub-tabs within the Drift panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum DriftTab {
    Suggestions,
    Orphans,
    Stale,
    Isolated,
}

impl DriftTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Suggestions => "Suggestions",
            Self::Orphans => "Orphans",
            Self::Stale => "Stale",
            Self::Isolated => "Isolated",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Suggestions => Self::Orphans,
            Self::Orphans => Self::Stale,
            Self::Stale => Self::Isolated,
            Self::Isolated => Self::Suggestions,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::Suggestions => Self::Isolated,
            Self::Orphans => Self::Suggestions,
            Self::Stale => Self::Orphans,
            Self::Isolated => Self::Stale,
        }
    }
}

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

/// State for the fact list: loaded facts, selection, sorting, detail view.
#[derive(Debug)]
pub(crate) struct FactListState {
    /// Facts loaded from the API.
    pub(crate) facts: Vec<MemoryFact>,
    /// Total count (may exceed loaded slice for pagination).
    pub(crate) total_facts: usize,
    /// Currently selected fact index in the table.
    pub(crate) selected: usize,
    /// Scroll offset for the fact list.
    pub(crate) scroll_offset: usize,
    /// Current sort column.
    pub(crate) sort: FactSort,
    /// Sort ascending?
    pub(crate) sort_asc: bool,
    /// Detail view for selected fact.
    pub(crate) detail: Option<FactDetail>,
    /// Whether the confidence edit dialog is active.
    pub(crate) editing_confidence: bool,
    /// Buffer for confidence editing.
    pub(crate) confidence_buffer: String,
}

impl FactListState {
    pub(crate) fn new() -> Self {
        Self {
            facts: Vec::new(),
            total_facts: 0,
            selected: 0,
            scroll_offset: 0,
            sort: FactSort::Confidence,
            sort_asc: false,
            detail: None,
            editing_confidence: false,
            confidence_buffer: String::new(),
        }
    }
}

impl Default for FactListState {
    fn default() -> Self {
        Self::new()
    }
}

/// State for text and type/tier filters applied to the fact list.
#[derive(Debug)]
pub(crate) struct MemoryFilterState {
    /// Active text filter.
    pub(crate) filter_text: String,
    /// Whether we're in filter editing mode.
    pub(crate) filter_editing: bool,
    /// Fact type filter (None = all).
    pub(crate) type_filter: Option<String>,
    /// Tier filter (None = all).
    pub(crate) tier_filter: Option<String>,
}

impl MemoryFilterState {
    pub(crate) fn new() -> Self {
        Self {
            filter_text: String::new(),
            filter_editing: false,
            type_filter: None,
            tier_filter: None,
        }
    }
}

impl Default for MemoryFilterState {
    fn default() -> Self {
        Self::new()
    }
}

/// State for the memory search overlay.
#[derive(Debug)]
pub(crate) struct MemorySearchState {
    /// Search results.
    pub(crate) search_results: Vec<MemorySearchResult>,
    /// Whether search mode is active.
    pub(crate) search_active: bool,
    /// Search query text.
    pub(crate) search_query: String,
}

impl MemorySearchState {
    pub(crate) fn new() -> Self {
        Self {
            search_results: Vec::new(),
            search_active: false,
            search_query: String::new(),
        }
    }
}

impl Default for MemorySearchState {
    fn default() -> Self {
        Self::new()
    }
}

/// State for the knowledge graph, drift analysis, and timeline views.
#[derive(Debug)]
pub(crate) struct MemoryGraphState {
    /// Entities loaded for graph view.
    pub(crate) entities: Vec<MemoryEntity>,
    /// Relationships loaded for graph view.
    pub(crate) relationships: Vec<MemoryRelationship>,
    /// Timeline events.
    pub(crate) timeline_events: Vec<MemoryTimelineEvent>,
    /// Entities with computed stats (PageRank, community, relationship count).
    pub(crate) entity_stats: Vec<GraphEntityStat>,
    /// Aggregate graph health metrics.
    pub(crate) health: GraphHealthMetrics,
    /// Selected entity index in the entity list.
    pub(crate) selected_entity: usize,
    /// Scroll offset for the entity list.
    pub(crate) entity_scroll_offset: usize,
    /// Current drift sub-tab.
    pub(crate) drift_tab: DriftTab,
    /// Drift suggestions (delete/review/merge).
    pub(crate) drift_suggestions: Vec<DriftSuggestion>,
    /// Names of orphaned entities (0 relationships).
    pub(crate) orphaned_entities: Vec<String>,
    /// Names of stale entities (>30d since update).
    pub(crate) stale_entities: Vec<String>,
    /// Clusters with fewer than 3 members.
    pub(crate) isolated_clusters: Vec<IsolatedCluster>,
    /// Selected item index in the current drift list.
    pub(crate) drift_selected: usize,
    /// Scroll offset for drift lists.
    pub(crate) drift_scroll_offset: usize,
    /// Loaded node card for entity detail view.
    pub(crate) node_card: Option<GraphNodeCard>,
}

impl MemoryGraphState {
    pub(crate) fn new() -> Self {
        Self {
            entities: Vec::new(),
            relationships: Vec::new(),
            timeline_events: Vec::new(),
            entity_stats: Vec::new(),
            health: GraphHealthMetrics::default(),
            selected_entity: 0,
            entity_scroll_offset: 0,
            drift_tab: DriftTab::Suggestions,
            drift_suggestions: Vec::new(),
            orphaned_entities: Vec::new(),
            stale_entities: Vec::new(),
            isolated_clusters: Vec::new(),
            drift_selected: 0,
            drift_scroll_offset: 0,
            node_card: None,
        }
    }
}

impl Default for MemoryGraphState {
    fn default() -> Self {
        Self::new()
    }
}

/// Full state for the memory inspector panel.
#[derive(Debug)]
pub struct MemoryInspectorState {
    // kanon:ignore RUST/pub-visibility
    /// Current active tab.
    pub(crate) tab: MemoryTab,
    /// Whether data is being loaded.
    pub(crate) loading: bool,
    /// Fact list state: facts, selection, sorting, detail.
    pub(crate) fact_list: FactListState,
    /// Filter state: text filter, type/tier filters.
    pub(crate) filters: MemoryFilterState,
    /// Search state: query, results, active flag.
    pub(crate) search: MemorySearchState,
    /// Graph state: entities, relationships, timeline events.
    pub(crate) graph: MemoryGraphState,
}

impl MemoryInspectorState {
    pub(crate) fn new() -> Self {
        Self {
            tab: MemoryTab::Facts,
            loading: false,
            fact_list: FactListState::new(),
            filters: MemoryFilterState::new(),
            search: MemorySearchState::new(),
            graph: MemoryGraphState::new(),
        }
    }

    /// Returns the currently selected fact, if any.
    pub(crate) fn selected_fact(&self) -> Option<&MemoryFact> {
        self.fact_list.facts.get(self.fact_list.selected)
    }

    /// Tier label abbreviation for table display.
    pub(crate) fn tier_abbrev(tier: &str) -> &'static str {
        match tier {
            "verified" => "Ver",
            "inferred" => "Inf",
            "assumed" => "Asm",
            _ => "???",
        }
    }

    /// Format a timestamp as relative time for compact display.
    pub(crate) fn relative_time(iso: &str) -> String {
        if iso.is_empty() {
            return "never".to_string();
        }
        if let Some(date) = iso.split('T').next() {
            date.to_string()
        } else {
            iso.to_string()
        }
    }
}

impl Default for MemoryInspectorState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn new_state_defaults() {
        let state = MemoryInspectorState::new();
        assert_eq!(state.tab, MemoryTab::Facts);
        assert!(state.fact_list.facts.is_empty());
        assert_eq!(state.fact_list.selected, 0);
        assert_eq!(state.fact_list.sort, FactSort::Confidence);
        assert!(!state.fact_list.sort_asc);
        assert!(state.filters.filter_text.is_empty());
        assert!(!state.filters.filter_editing);
        assert!(!state.loading);
        assert!(!state.search.search_active);
        assert!(!state.fact_list.editing_confidence);
    }

    #[test]
    fn fact_sort_cycle() {
        let mut sort = FactSort::Confidence;
        let expected = [
            FactSort::Recency,
            FactSort::Created,
            FactSort::AccessCount,
            FactSort::FsrsReview,
            FactSort::Confidence,
        ];
        for exp in expected {
            sort = sort.next();
            assert_eq!(sort, exp);
        }
    }

    #[test]
    fn fact_sort_labels() {
        assert_eq!(FactSort::Confidence.label(), "Confidence");
        assert_eq!(FactSort::Recency.label(), "Last Seen");
        assert_eq!(FactSort::Created.label(), "Created");
        assert_eq!(FactSort::AccessCount.label(), "Accesses");
        assert_eq!(FactSort::FsrsReview.label(), "FSRS Review");
    }

    #[test]
    fn fact_sort_as_str() {
        assert_eq!(FactSort::Confidence.as_str(), "confidence");
        assert_eq!(FactSort::Recency.as_str(), "recency");
    }

    #[test]
    fn memory_tab_cycle() {
        assert_eq!(MemoryTab::Facts.next(), MemoryTab::Graph);
        assert_eq!(MemoryTab::Graph.next(), MemoryTab::Drift);
        assert_eq!(MemoryTab::Drift.next(), MemoryTab::Timeline);
        assert_eq!(MemoryTab::Timeline.next(), MemoryTab::Facts);
    }

    #[test]
    fn memory_tab_cycle_prev() {
        assert_eq!(MemoryTab::Facts.prev(), MemoryTab::Timeline);
        assert_eq!(MemoryTab::Timeline.prev(), MemoryTab::Drift);
        assert_eq!(MemoryTab::Drift.prev(), MemoryTab::Graph);
        assert_eq!(MemoryTab::Graph.prev(), MemoryTab::Facts);
    }

    #[test]
    fn memory_tab_labels() {
        assert_eq!(MemoryTab::Facts.label(), "Facts");
        assert_eq!(MemoryTab::Graph.label(), "Graph");
        assert_eq!(MemoryTab::Drift.label(), "Drift");
        assert_eq!(MemoryTab::Timeline.label(), "Timeline");
    }

    #[test]
    fn drift_tab_cycle() {
        assert_eq!(DriftTab::Suggestions.next(), DriftTab::Orphans);
        assert_eq!(DriftTab::Orphans.next(), DriftTab::Stale);
        assert_eq!(DriftTab::Stale.next(), DriftTab::Isolated);
        assert_eq!(DriftTab::Isolated.next(), DriftTab::Suggestions);
    }

    #[test]
    fn drift_tab_cycle_prev() {
        assert_eq!(DriftTab::Suggestions.prev(), DriftTab::Isolated);
        assert_eq!(DriftTab::Isolated.prev(), DriftTab::Stale);
        assert_eq!(DriftTab::Stale.prev(), DriftTab::Orphans);
        assert_eq!(DriftTab::Orphans.prev(), DriftTab::Suggestions);
    }

    #[test]
    fn drift_tab_labels() {
        assert_eq!(DriftTab::Suggestions.label(), "Suggestions");
        assert_eq!(DriftTab::Orphans.label(), "Orphans");
        assert_eq!(DriftTab::Stale.label(), "Stale");
        assert_eq!(DriftTab::Isolated.label(), "Isolated");
    }

    #[test]
    fn graph_health_metrics_default() {
        let health = GraphHealthMetrics::default();
        assert_eq!(health.total_entities, 0);
        assert_eq!(health.orphan_count, 0);
        assert_eq!(health.avg_cluster_size, 0.0);
    }

    #[test]
    fn tier_abbrev() {
        assert_eq!(MemoryInspectorState::tier_abbrev("verified"), "Ver");
        assert_eq!(MemoryInspectorState::tier_abbrev("inferred"), "Inf");
        assert_eq!(MemoryInspectorState::tier_abbrev("assumed"), "Asm");
        assert_eq!(MemoryInspectorState::tier_abbrev("unknown"), "???");
    }

    #[test]
    fn relative_time_empty() {
        assert_eq!(MemoryInspectorState::relative_time(""), "never");
    }

    #[test]
    fn relative_time_iso() {
        assert_eq!(
            MemoryInspectorState::relative_time("2026-03-09T12:00:00Z"),
            "2026-03-09"
        );
    }

    #[test]
    fn relative_time_date_only() {
        assert_eq!(
            MemoryInspectorState::relative_time("2026-03-09"),
            "2026-03-09"
        );
    }

    #[test]
    fn selected_fact_empty() {
        let state = MemoryInspectorState::new();
        assert!(state.selected_fact().is_none());
    }

    #[test]
    fn selected_fact_with_data() {
        let mut state = MemoryInspectorState::new();
        state.fact_list.facts.push(MemoryFact {
            id: "f1".into(),
            nous_id: "syn".into(),
            content: "test fact".into(),
            confidence: 0.9,
            tier: "verified".into(),
            fact_type: "knowledge".into(),
            temporal: FactTemporalMeta {
                stability_hours: 720.0,
                ..FactTemporalMeta::default()
            },
            lifecycle: FactLifecycleMeta::default(),
        });
        state.fact_list.selected = 0;
        assert!(state.selected_fact().is_some());
        assert_eq!(state.selected_fact().unwrap().content, "test fact");
    }

    #[test]
    fn memory_fact_deserialization() {
        let json = r#"{
            "id": "f1",
            "content": "snafu is the error library",
            "confidence": 0.95,
            "tier": "verified",
            "factType": "knowledge",
            "accessCount": 10,
            "stabilityHours": 720.0
        }"#;
        let fact: MemoryFact = serde_json::from_str(json).unwrap();
        assert_eq!(fact.id, "f1");
        assert_eq!(fact.confidence, 0.95);
        assert_eq!(fact.fact_type, "knowledge");
        assert_eq!(fact.temporal.access_count, 10);
    }

    #[test]
    fn memory_fact_defaults() {
        let json = r#"{"id": "f1", "content": "test", "confidence": 0.5, "tier": "assumed"}"#;
        let fact: MemoryFact = serde_json::from_str(json).unwrap();
        assert_eq!(fact.temporal.access_count, 0);
        assert!(fact.nous_id.is_empty());
        assert!(!fact.lifecycle.is_forgotten);
        assert!(fact.fact_type.is_empty());
    }

    #[test]
    fn memory_entity_deserialization() {
        let json = r#"{"id": "e1", "name": "snafu", "entity_type": "tool"}"#;
        let entity: MemoryEntity = serde_json::from_str(json).unwrap();
        assert_eq!(entity.name, "snafu");
        assert_eq!(entity.entity_type, "tool");
    }

    #[test]
    fn memory_relationship_deserialization() {
        let json = r#"{"src": "e1", "dst": "e2", "relation": "USED_BY", "weight": 0.8}"#;
        let rel: MemoryRelationship = serde_json::from_str(json).unwrap();
        assert_eq!(rel.relation, "USED_BY");
        assert!((rel.weight - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn search_result_deserialization() {
        let json = r#"{
            "id": "f1",
            "content": "test",
            "confidence": 0.9,
            "tier": "verified",
            "factType": "knowledge",
            "score": 1.5
        }"#;
        let result: MemorySearchResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.score, 1.5);
    }

    #[test]
    fn fact_detail_deserialization() {
        let json = r#"{
            "fact": {
                "id": "f1",
                "content": "test",
                "confidence": 0.9,
                "tier": "verified"
            },
            "relationships": [],
            "similar": []
        }"#;
        let detail: FactDetail = serde_json::from_str(json).unwrap();
        assert_eq!(detail.fact.id, "f1");
        assert!(detail.relationships.is_empty());
        assert!(detail.similar.is_empty());
    }

    #[test]
    fn timeline_event_deserialization() {
        let json = r#"{
            "timestamp": "2026-03-09T12:00:00Z",
            "eventType": "created",
            "description": "test fact",
            "factId": "f1",
            "confidence": 0.9
        }"#;
        let event: MemoryTimelineEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, "created");
        assert_eq!(event.fact_id, "f1");
    }

    #[test]
    fn default_creates_same_as_new() {
        let a = MemoryInspectorState::new();
        let b = MemoryInspectorState::default();
        assert_eq!(a.tab, b.tab);
        assert_eq!(a.fact_list.selected, b.fact_list.selected);
        assert_eq!(a.fact_list.sort, b.fact_list.sort);
    }
}
