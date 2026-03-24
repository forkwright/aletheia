//! Entity list, detail, and navigation state for the memory explorer.

/// Sort field for entity list ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EntitySort {
    /// Highest PageRank first.
    #[default]
    PageRank,
    /// Highest confidence first.
    Confidence,
    /// Most associated memories first.
    MemoryCount,
    /// Most recently updated first.
    LastUpdated,
    /// Alphabetical by name.
    Alphabetical,
}

impl EntitySort {
    /// All available sort options in display order.
    pub(crate) const ALL: &[Self] = &[
        Self::PageRank,
        Self::Confidence,
        Self::MemoryCount,
        Self::LastUpdated,
        Self::Alphabetical,
    ];

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::PageRank => "PageRank",
            Self::Confidence => "Confidence",
            Self::MemoryCount => "Memories",
            Self::LastUpdated => "Last Updated",
            Self::Alphabetical => "Name",
        }
    }
}

/// Entity type classification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize)]
pub(crate) enum EntityType {
    Person,
    Concept,
    Project,
    Tool,
    Location,
    Organization,
    Event,
    Other(String),
}

impl EntityType {
    /// All fixed entity types for filter UI.
    pub(crate) const FIXED: &[Self] = &[
        Self::Person,
        Self::Concept,
        Self::Project,
        Self::Tool,
        Self::Location,
        Self::Organization,
        Self::Event,
    ];

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Person => "Person",
            Self::Concept => "Concept",
            Self::Project => "Project",
            Self::Tool => "Tool",
            Self::Location => "Location",
            Self::Organization => "Organization",
            Self::Event => "Event",
            Self::Other(s) => s.as_str(),
        }
    }

    /// Badge color for this entity type.
    #[must_use]
    pub(crate) fn color(&self) -> &'static str {
        match self {
            Self::Person => "#7a7aff",
            Self::Concept => "#a855f7",
            Self::Project => "#22c55e",
            Self::Tool => "#f59e0b",
            Self::Location => "#06b6d4",
            Self::Organization => "#ec4899",
            Self::Event => "#ef4444",
            Self::Other(_) => "#888",
        }
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Flag severity for entity review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum FlagSeverity {
    Low,
    Medium,
    High,
}

impl FlagSeverity {
    /// All available severity levels.
    pub(crate) const ALL: &[Self] = &[Self::Low, Self::Medium, Self::High];

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    /// Badge color for severity.
    #[must_use]
    #[expect(
        dead_code,
        reason = "part of public API for future flag severity display"
    )]
    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Low => "#06b6d4",
            Self::Medium => "#f59e0b",
            Self::High => "#ef4444",
        }
    }
}

/// An entity from the knowledge graph.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Entity {
    /// Unique identifier.
    pub id: String,
    /// Primary display name.
    pub name: String,
    /// Entity classification.
    #[serde(default = "default_entity_type")]
    pub entity_type: EntityType,
    /// Confidence score (0.0–1.0).
    #[serde(default)]
    pub confidence: f64,
    /// PageRank importance score.
    #[serde(default)]
    pub page_rank: f64,
    /// Number of associated memories.
    #[serde(default)]
    pub memory_count: u32,
    /// Number of relationships.
    #[serde(default)]
    pub relationship_count: u32,
    /// Key-value properties.
    #[serde(default)]
    pub properties: Vec<EntityProperty>,
    /// Last updated timestamp (ISO 8601).
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Creating agent ID.
    #[serde(default)]
    pub created_by: Option<String>,
    /// Creation timestamp.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Whether this entity is flagged for review.
    #[serde(default)]
    pub flagged: bool,
}

fn default_entity_type() -> EntityType {
    EntityType::Other("Unknown".to_string())
}

/// A key-value property on an entity.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct EntityProperty {
    pub key: String,
    pub value: String,
}

/// A relationship between two entities.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Relationship {
    /// Relationship ID.
    pub id: String,
    /// Related entity ID.
    pub entity_id: String,
    /// Related entity name.
    pub entity_name: String,
    /// Relationship type label (e.g., "depends_on", "authored_by").
    pub relationship_type: String,
    /// Whether this is an outgoing or incoming relationship.
    #[serde(default)]
    pub direction: RelationshipDirection,
    /// Confidence in this relationship.
    #[serde(default)]
    pub confidence: f64,
}

/// Direction of a relationship relative to the viewed entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
pub(crate) enum RelationshipDirection {
    #[default]
    Outgoing,
    Incoming,
}

impl RelationshipDirection {
    /// Arrow indicator for display.
    #[must_use]
    pub(crate) fn arrow(self) -> &'static str {
        match self {
            Self::Outgoing => "→",
            Self::Incoming => "←",
        }
    }
}

/// A memory associated with an entity.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct EntityMemory {
    /// Memory ID.
    pub id: String,
    /// Content text.
    pub content: String,
    /// Source agent.
    #[serde(default)]
    pub agent: Option<String>,
    /// Source session.
    #[serde(default)]
    pub session: Option<String>,
    /// Confidence score.
    #[serde(default)]
    pub confidence: f64,
    /// Creation timestamp.
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Paginated entity list with sort and filter state.
#[derive(Debug, Clone)]
pub(crate) struct EntityListStore {
    /// Currently loaded entities.
    pub entities: Vec<Entity>,
    /// Current sort field.
    pub sort: EntitySort,
    /// Text search query.
    pub search_query: String,
    /// Entity type filters (empty = all types).
    pub type_filter: Vec<EntityType>,
    /// Minimum confidence threshold.
    pub min_confidence: f64,
    /// Agent filter (empty = all agents).
    pub agent_filter: Vec<String>,
    /// Current page (0-indexed).
    pub page: usize,
    /// Whether more pages are available.
    pub has_more: bool,
}

impl Default for EntityListStore {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            sort: EntitySort::default(),
            search_query: String::new(),
            type_filter: Vec::new(),
            min_confidence: 0.0,
            agent_filter: Vec::new(),
            page: 0,
            has_more: false,
        }
    }
}

impl EntityListStore {
    /// Entities per page.
    pub(crate) const PAGE_SIZE: usize = 50;

    /// Create a new empty store.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Replace the entity list with fresh data.
    pub(crate) fn load(&mut self, entities: Vec<Entity>, has_more: bool) {
        self.entities = entities;
        self.has_more = has_more;
    }

    /// Append more entities (next page).
    pub(crate) fn append(&mut self, entities: Vec<Entity>, has_more: bool) {
        self.entities.extend(entities);
        self.has_more = has_more;
    }

    /// Reset all filters and pagination.
    pub(crate) fn clear_filters(&mut self) {
        self.search_query.clear();
        self.type_filter.clear();
        self.min_confidence = 0.0;
        self.agent_filter.clear();
        self.page = 0;
    }

    /// Whether any filter is active.
    #[must_use]
    pub(crate) fn has_active_filters(&self) -> bool {
        !self.search_query.is_empty()
            || !self.type_filter.is_empty()
            || self.min_confidence > 0.0
            || !self.agent_filter.is_empty()
    }

    /// Number of active filter chips to display.
    #[must_use]
    #[expect(dead_code, reason = "available for filter chip count display")]
    pub(crate) fn active_filter_count(&self) -> usize {
        let mut count = 0;
        if !self.search_query.is_empty() {
            count += 1;
        }
        count += self.type_filter.len();
        if self.min_confidence > 0.0 {
            count += 1;
        }
        count += self.agent_filter.len();
        count
    }

    /// Sort entities client-side by the current sort field.
    pub(crate) fn sort_entities(&mut self) {
        match self.sort {
            EntitySort::PageRank => {
                self.entities.sort_by(|a, b| {
                    b.page_rank
                        .partial_cmp(&a.page_rank)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            EntitySort::Confidence => {
                self.entities.sort_by(|a, b| {
                    b.confidence
                        .partial_cmp(&a.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            EntitySort::MemoryCount => {
                self.entities
                    .sort_by(|a, b| b.memory_count.cmp(&a.memory_count));
            }
            EntitySort::LastUpdated => {
                self.entities.sort_by(|a, b| {
                    b.updated_at
                        .as_deref()
                        .unwrap_or("")
                        .cmp(a.updated_at.as_deref().unwrap_or(""))
                });
            }
            EntitySort::Alphabetical => {
                self.entities
                    .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            }
        }
    }
}

/// Detail state for a single entity.
#[derive(Debug, Clone, Default)]
pub(crate) struct EntityDetailStore {
    /// The entity being viewed.
    pub entity: Option<Entity>,
    /// Relationships of this entity.
    pub relationships: Vec<Relationship>,
    /// Memories associated with this entity.
    pub memories: Vec<EntityMemory>,
}

/// Navigation history for entity traversal.
#[derive(Debug, Clone, Default)]
pub(crate) struct EntityNavigationHistory {
    /// Stack of visited entity IDs.
    stack: Vec<String>,
    /// Current position in the stack (index into `stack`).
    cursor: usize,
}

impl EntityNavigationHistory {
    /// Create a new empty history.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Navigate to a new entity. Truncates forward history.
    pub(crate) fn push(&mut self, entity_id: String) {
        // WHY: Navigating to a new entity truncates any forward history,
        // matching browser-style back/forward semantics.
        if !self.stack.is_empty() {
            self.stack.truncate(self.cursor + 1);
        }
        self.stack.push(entity_id);
        self.cursor = self.stack.len().saturating_sub(1);
    }

    /// Go back one step. Returns the entity ID to navigate to, if available.
    #[must_use]
    pub(crate) fn back(&mut self) -> Option<&str> {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.stack.get(self.cursor).map(String::as_str)
        } else {
            None
        }
    }

    /// Go forward one step. Returns the entity ID to navigate to, if available.
    #[must_use]
    pub(crate) fn forward(&mut self) -> Option<&str> {
        if self.cursor + 1 < self.stack.len() {
            self.cursor += 1;
            self.stack.get(self.cursor).map(String::as_str)
        } else {
            None
        }
    }

    /// Whether back navigation is available.
    #[must_use]
    pub(crate) fn can_go_back(&self) -> bool {
        self.cursor > 0
    }

    /// Whether forward navigation is available.
    #[must_use]
    pub(crate) fn can_go_forward(&self) -> bool {
        self.cursor + 1 < self.stack.len()
    }

    /// Current entity ID, if any.
    #[must_use]
    #[expect(dead_code, reason = "used by tests and future navigation display")]
    pub(crate) fn current(&self) -> Option<&str> {
        self.stack.get(self.cursor).map(String::as_str)
    }

    /// Breadcrumb trail from start to current position.
    #[must_use]
    pub(crate) fn breadcrumbs(&self) -> &[String] {
        if self.stack.is_empty() {
            &[]
        } else {
            &self.stack[..=self.cursor]
        }
    }

    /// Number of entries in the history.
    #[must_use]
    #[expect(dead_code, reason = "used by tests and future navigation display")]
    pub(crate) fn len(&self) -> usize {
        self.stack.len()
    }

    /// Whether the history is empty.
    #[must_use]
    #[expect(dead_code, reason = "used by tests and future navigation display")]
    pub(crate) fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

/// Confidence level thresholds for color coding.
#[must_use]
pub(crate) fn confidence_color(value: f64) -> &'static str {
    if value > 0.7 {
        "#22c55e"
    } else if value >= 0.4 {
        "#f59e0b"
    } else {
        "#ef4444"
    }
}

/// Format confidence as a percentage string.
#[must_use]
pub(crate) fn format_confidence(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

/// Format PageRank for display.
#[must_use]
pub(crate) fn format_page_rank(value: f64) -> String {
    format!("{value:.4}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entity(id: &str, name: &str, page_rank: f64, confidence: f64) -> Entity {
        Entity {
            id: id.to_string(),
            name: name.to_string(),
            entity_type: EntityType::Concept,
            confidence,
            page_rank,
            memory_count: 5,
            relationship_count: 3,
            properties: Vec::new(),
            updated_at: Some("2025-06-15T10:00:00Z".to_string()),
            created_by: Some("agent-1".to_string()),
            created_at: Some("2025-06-01T00:00:00Z".to_string()),
            flagged: false,
        }
    }

    #[test]
    fn entity_list_store_defaults() {
        let store = EntityListStore::new();
        assert!(store.entities.is_empty());
        assert_eq!(store.sort, EntitySort::PageRank);
        assert!(!store.has_active_filters());
        assert_eq!(store.active_filter_count(), 0);
    }

    #[test]
    fn entity_list_store_load_and_append() {
        let mut store = EntityListStore::new();
        store.load(vec![make_entity("e1", "Alpha", 0.5, 0.8)], true);
        assert_eq!(store.entities.len(), 1);
        assert!(store.has_more);

        store.append(vec![make_entity("e2", "Beta", 0.3, 0.6)], false);
        assert_eq!(store.entities.len(), 2);
        assert!(!store.has_more);
    }

    #[test]
    fn entity_list_store_clear_filters() {
        let mut store = EntityListStore::new();
        store.search_query = "test".to_string();
        store.type_filter = vec![EntityType::Person];
        store.min_confidence = 0.5;
        store.agent_filter = vec!["agent-1".to_string()];
        store.page = 2;
        assert!(store.has_active_filters());
        assert_eq!(store.active_filter_count(), 4);

        store.clear_filters();
        assert!(!store.has_active_filters());
        assert_eq!(store.page, 0);
    }

    #[test]
    fn entity_list_store_sort_by_page_rank() {
        let mut store = EntityListStore::new();
        store.load(
            vec![
                make_entity("e1", "Low", 0.1, 0.5),
                make_entity("e2", "High", 0.9, 0.5),
            ],
            false,
        );
        store.sort = EntitySort::PageRank;
        store.sort_entities();
        assert_eq!(store.entities[0].name, "High");
        assert_eq!(store.entities[1].name, "Low");
    }

    #[test]
    fn entity_list_store_sort_by_confidence() {
        let mut store = EntityListStore::new();
        store.load(
            vec![
                make_entity("e1", "Weak", 0.5, 0.2),
                make_entity("e2", "Strong", 0.5, 0.95),
            ],
            false,
        );
        store.sort = EntitySort::Confidence;
        store.sort_entities();
        assert_eq!(store.entities[0].name, "Strong");
    }

    #[test]
    fn entity_list_store_sort_alphabetical() {
        let mut store = EntityListStore::new();
        store.load(
            vec![
                make_entity("e1", "Zebra", 0.5, 0.5),
                make_entity("e2", "Apple", 0.5, 0.5),
            ],
            false,
        );
        store.sort = EntitySort::Alphabetical;
        store.sort_entities();
        assert_eq!(store.entities[0].name, "Apple");
        assert_eq!(store.entities[1].name, "Zebra");
    }

    #[test]
    fn navigation_history_push_and_traverse() {
        let mut nav = EntityNavigationHistory::new();
        assert!(nav.is_empty());
        assert!(!nav.can_go_back());
        assert!(!nav.can_go_forward());

        nav.push("e1".to_string());
        assert_eq!(nav.current(), Some("e1"));
        assert_eq!(nav.len(), 1);

        nav.push("e2".to_string());
        nav.push("e3".to_string());
        assert_eq!(nav.current(), Some("e3"));
        assert!(nav.can_go_back());
        assert!(!nav.can_go_forward());

        // Go back
        assert_eq!(nav.back(), Some("e2"));
        assert_eq!(nav.current(), Some("e2"));
        assert!(nav.can_go_forward());

        // Go forward
        assert_eq!(nav.forward(), Some("e3"));
        assert_eq!(nav.current(), Some("e3"));
    }

    #[test]
    fn navigation_history_push_truncates_forward() {
        let mut nav = EntityNavigationHistory::new();
        nav.push("e1".to_string());
        nav.push("e2".to_string());
        nav.push("e3".to_string());

        // Go back to e1
        let _ = nav.back();
        let _ = nav.back();
        assert_eq!(nav.current(), Some("e1"));

        // Push new — should truncate e2 and e3
        nav.push("e4".to_string());
        assert_eq!(nav.current(), Some("e4"));
        assert_eq!(nav.len(), 2);
        assert!(!nav.can_go_forward());
    }

    #[test]
    fn navigation_history_breadcrumbs() {
        let mut nav = EntityNavigationHistory::new();
        nav.push("e1".to_string());
        nav.push("e2".to_string());
        nav.push("e3".to_string());
        assert_eq!(nav.breadcrumbs(), &["e1", "e2", "e3"]);

        let _ = nav.back();
        assert_eq!(nav.breadcrumbs(), &["e1", "e2"]);
    }

    #[test]
    fn navigation_history_empty_back_forward() {
        let mut nav = EntityNavigationHistory::new();
        assert!(nav.back().is_none());
        assert!(nav.forward().is_none());
        assert!(nav.current().is_none());
        assert!(nav.breadcrumbs().is_empty());
    }

    #[test]
    fn confidence_color_thresholds() {
        assert_eq!(confidence_color(0.8), "#22c55e");
        assert_eq!(confidence_color(0.71), "#22c55e");
        assert_eq!(confidence_color(0.7), "#f59e0b");
        assert_eq!(confidence_color(0.5), "#f59e0b");
        assert_eq!(confidence_color(0.4), "#f59e0b");
        assert_eq!(confidence_color(0.39), "#ef4444");
        assert_eq!(confidence_color(0.0), "#ef4444");
    }

    #[test]
    fn format_confidence_percentage() {
        assert_eq!(format_confidence(0.0), "0%");
        assert_eq!(format_confidence(0.5), "50%");
        assert_eq!(format_confidence(1.0), "100%");
        assert_eq!(format_confidence(0.756), "76%");
    }

    #[test]
    fn entity_type_labels_and_colors() {
        for et in EntityType::FIXED {
            assert!(!et.label().is_empty());
            assert!(et.color().starts_with('#'));
        }
        let other = EntityType::Other("Custom".to_string());
        assert_eq!(other.label(), "Custom");
    }

    #[test]
    fn entity_sort_labels() {
        for sort in EntitySort::ALL {
            assert!(!sort.label().is_empty());
        }
    }

    #[test]
    fn flag_severity_labels_and_colors() {
        for sev in FlagSeverity::ALL {
            assert!(!sev.label().is_empty());
            assert!(sev.color().starts_with('#'));
        }
    }

    #[test]
    fn relationship_direction_arrows() {
        assert_eq!(RelationshipDirection::Outgoing.arrow(), "→");
        assert_eq!(RelationshipDirection::Incoming.arrow(), "←");
    }
}
