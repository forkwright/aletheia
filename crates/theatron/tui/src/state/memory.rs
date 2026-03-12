//! State for the memory inspector panel.

use serde::{Deserialize, Serialize};

/// Sort options for the fact browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactSort {
    Confidence,
    Recency,
    Created,
    AccessCount,
    FsrsReview,
}

impl FactSort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confidence => "confidence",
            Self::Recency => "recency",
            Self::Created => "created",
            Self::AccessCount => "access_count",
            Self::FsrsReview => "fsrs_review",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Confidence => "Confidence",
            Self::Recency => "Last Seen",
            Self::Created => "Created",
            Self::AccessCount => "Accesses",
            Self::FsrsReview => "FSRS Review",
        }
    }

    pub fn next(self) -> Self {
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
pub enum MemoryTab {
    Facts,
    Graph,
    Timeline,
}

impl MemoryTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Facts => "Facts",
            Self::Graph => "Graph",
            Self::Timeline => "Timeline",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Facts => Self::Graph,
            Self::Graph => Self::Timeline,
            Self::Timeline => Self::Facts,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Facts => Self::Timeline,
            Self::Graph => Self::Facts,
            Self::Timeline => Self::Graph,
        }
    }
}

/// A fact as displayed in the TUI (deserialized from API).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFact {
    pub id: String,
    #[serde(default)]
    pub nous_id: String,
    pub content: String,
    pub confidence: f64,
    pub tier: String,
    #[serde(default)]
    pub valid_from: String,
    #[serde(default)]
    pub valid_to: String,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub source_session_id: Option<String>,
    #[serde(default)]
    pub recorded_at: String,
    #[serde(default)]
    pub access_count: u32,
    #[serde(default)]
    pub last_accessed_at: String,
    #[serde(default)]
    pub stability_hours: f64,
    #[serde(default)]
    pub fact_type: String,
    #[serde(default)]
    pub is_forgotten: bool,
    #[serde(default)]
    pub forgotten_at: Option<String>,
    #[serde(default)]
    pub forget_reason: Option<String>,
}

/// An entity in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntity {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// A relationship between entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRelationship {
    pub src: String,
    pub dst: String,
    pub relation: String,
    #[serde(default)]
    pub weight: f64,
    #[serde(default)]
    pub created_at: String,
}

/// A timeline event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryTimelineEvent {
    pub timestamp: String,
    pub event_type: String,
    pub description: String,
    pub fact_id: String,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// A similar fact result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarFact {
    pub id: String,
    pub content: String,
    pub similarity: f64,
}

/// Search result from the knowledge API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchResult {
    pub id: String,
    pub content: String,
    pub confidence: f64,
    pub tier: String,
    pub fact_type: String,
    pub score: f64,
}

/// Fact detail with relationships and similar facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactDetail {
    pub fact: MemoryFact,
    #[serde(default)]
    pub relationships: Vec<MemoryRelationship>,
    #[serde(default)]
    pub similar: Vec<SimilarFact>,
}

/// Full state for the memory inspector panel.
#[derive(Debug)]
pub struct MemoryInspectorState {
    /// Current active tab.
    pub tab: MemoryTab,
    /// Facts loaded from the API.
    pub facts: Vec<MemoryFact>,
    /// Total count (may exceed loaded slice for pagination).
    pub total_facts: usize,
    /// Currently selected fact index in the table.
    pub selected: usize,
    /// Current sort column.
    pub sort: FactSort,
    /// Sort ascending?
    pub sort_asc: bool,
    /// Active text filter.
    pub filter_text: String,
    /// Whether we're in filter editing mode.
    pub filter_editing: bool,
    /// Fact type filter (None = all).
    pub type_filter: Option<String>,
    /// Tier filter (None = all).
    pub tier_filter: Option<String>,
    /// Detail view for selected fact.
    pub detail: Option<FactDetail>,
    /// Entities loaded for graph view.
    pub entities: Vec<MemoryEntity>,
    /// Relationships loaded for graph view.
    pub relationships: Vec<MemoryRelationship>,
    /// Timeline events.
    pub timeline_events: Vec<MemoryTimelineEvent>,
    /// Search results.
    pub search_results: Vec<MemorySearchResult>,
    /// Whether search mode is active.
    pub search_active: bool,
    /// Search query text.
    pub search_query: String,
    /// Whether data is being loaded.
    pub loading: bool,
    /// Scroll offset for the fact list.
    pub scroll_offset: usize,
    /// Whether the confidence edit dialog is active.
    pub editing_confidence: bool,
    /// Buffer for confidence editing.
    pub confidence_buffer: String,
}

impl MemoryInspectorState {
    pub fn new() -> Self {
        Self {
            tab: MemoryTab::Facts,
            facts: Vec::new(),
            total_facts: 0,
            selected: 0,
            sort: FactSort::Confidence,
            sort_asc: false,
            filter_text: String::new(),
            filter_editing: false,
            type_filter: None,
            tier_filter: None,
            detail: None,
            entities: Vec::new(),
            relationships: Vec::new(),
            timeline_events: Vec::new(),
            search_results: Vec::new(),
            search_active: false,
            search_query: String::new(),
            loading: false,
            scroll_offset: 0,
            editing_confidence: false,
            confidence_buffer: String::new(),
        }
    }

    /// Returns the currently selected fact, if any.
    pub fn selected_fact(&self) -> Option<&MemoryFact> {
        self.facts.get(self.selected)
    }

    /// Tier label abbreviation for table display.
    pub fn tier_abbrev(tier: &str) -> &'static str {
        match tier {
            "verified" => "Ver",
            "inferred" => "Inf",
            "assumed" => "Asm",
            _ => "???",
        }
    }

    /// Format a timestamp as relative time for compact display.
    pub fn relative_time(iso: &str) -> String {
        if iso.is_empty() {
            return "never".to_string();
        }
        // Simple approach: just show date portion for now
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
mod tests {
    use super::*;

    #[test]
    fn new_state_defaults() {
        let state = MemoryInspectorState::new();
        assert_eq!(state.tab, MemoryTab::Facts);
        assert!(state.facts.is_empty());
        assert_eq!(state.selected, 0);
        assert_eq!(state.sort, FactSort::Confidence);
        assert!(!state.sort_asc);
        assert!(state.filter_text.is_empty());
        assert!(!state.filter_editing);
        assert!(!state.loading);
        assert!(!state.search_active);
        assert!(!state.editing_confidence);
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
        assert_eq!(MemoryTab::Graph.next(), MemoryTab::Timeline);
        assert_eq!(MemoryTab::Timeline.next(), MemoryTab::Facts);
    }

    #[test]
    fn memory_tab_cycle_prev() {
        assert_eq!(MemoryTab::Facts.prev(), MemoryTab::Timeline);
        assert_eq!(MemoryTab::Timeline.prev(), MemoryTab::Graph);
        assert_eq!(MemoryTab::Graph.prev(), MemoryTab::Facts);
    }

    #[test]
    fn memory_tab_labels() {
        assert_eq!(MemoryTab::Facts.label(), "Facts");
        assert_eq!(MemoryTab::Graph.label(), "Graph");
        assert_eq!(MemoryTab::Timeline.label(), "Timeline");
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
        state.facts.push(MemoryFact {
            id: "f1".into(),
            nous_id: "syn".into(),
            content: "test fact".into(),
            confidence: 0.9,
            tier: "verified".into(),
            valid_from: String::new(),
            valid_to: String::new(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: String::new(),
            access_count: 0,
            last_accessed_at: String::new(),
            stability_hours: 720.0,
            fact_type: "knowledge".into(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        });
        state.selected = 0;
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
        assert_eq!(fact.access_count, 10);
    }

    #[test]
    fn memory_fact_defaults() {
        let json = r#"{"id": "f1", "content": "test", "confidence": 0.5, "tier": "assumed"}"#;
        let fact: MemoryFact = serde_json::from_str(json).unwrap();
        assert_eq!(fact.access_count, 0);
        assert!(fact.nous_id.is_empty());
        assert!(!fact.is_forgotten);
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
        assert_eq!(a.selected, b.selected);
        assert_eq!(a.sort, b.sort);
    }
}
