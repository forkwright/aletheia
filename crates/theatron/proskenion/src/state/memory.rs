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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

impl<'de> serde::Deserialize<'de> for EntityType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_raw(String::deserialize(deserializer)?))
    }
}

impl EntityType {
    /// Classify a raw server string, case-insensitively.
    ///
    /// WHY: the backend stores `entity_type` as free-form text; a
    /// non-canonical value must map to `Other` instead of failing the
    /// deserialization of every entity in the payload.
    fn from_raw(raw: String) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "person" => Self::Person,
            "concept" => Self::Concept,
            "project" => Self::Project,
            "tool" => Self::Tool,
            "location" => Self::Location,
            "organization" => Self::Organization,
            "event" => Self::Event,
            _ => Self::Other(raw),
        }
    }

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
            Self::Project => "var(--status-success)",
            Self::Tool => "var(--status-warning)",
            Self::Location => "#06b6d4",
            Self::Organization => "#ec4899",
            Self::Event => "var(--status-error)",
            Self::Other(_) => "var(--text-secondary)",
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of public API for future flag severity display"
        )
    )]
    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Low => "var(--status-info)",
            Self::Medium => "var(--status-warning)",
            Self::High => "var(--status-error)",
        }
    }
}

/// An entity from the knowledge graph.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(from = "EntityRaw")]
pub(crate) struct Entity {
    /// Unique identifier.
    // kanon:ignore RUST/primitive-for-domain-id — Entity/Relationship memory state mirrors server-side string IDs from the knowledge API
    pub id: String,
    /// Primary display name.
    pub name: String,
    /// Entity classification.
    pub entity_type: EntityType,
    /// Confidence score (0.0--1.0).
    pub confidence: f64,
    /// PageRank importance score.
    pub page_rank: f64,
    /// Number of associated memories.
    pub memory_count: u32,
    /// Number of relationships.
    pub relationship_count: u32,
    /// Key-value properties.
    pub properties: Vec<EntityProperty>,
    /// Last updated timestamp (ISO 8601).
    pub updated_at: Option<String>,
    /// Creating agent ID.
    pub created_by: Option<String>,
    /// Creation timestamp.
    pub created_at: Option<String>,
    /// Whether this entity is flagged for review.
    pub flagged: bool,
}

/// Raw deserialization type for [`Entity`].
#[derive(Debug, Clone, serde::Deserialize)]
struct EntityRaw {
    id: String,
    name: String,
    #[serde(default = "default_entity_type")]
    entity_type: EntityType,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    page_rank: f64,
    #[serde(default)]
    memory_count: u32,
    #[serde(default)]
    relationship_count: u32,
    #[serde(default)]
    properties: Vec<EntityProperty>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    created_by: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    flagged: bool,
}

impl From<EntityRaw> for Entity {
    fn from(raw: EntityRaw) -> Self {
        Self {
            id: raw.id,
            name: raw.name,
            entity_type: raw.entity_type,
            confidence: raw.confidence,
            page_rank: raw.page_rank,
            memory_count: raw.memory_count,
            relationship_count: raw.relationship_count,
            properties: raw.properties,
            updated_at: raw.updated_at,
            created_by: raw.created_by,
            created_at: raw.created_at,
            flagged: raw.flagged,
        }
    }
}

fn default_entity_type() -> EntityType {
    EntityType::Other("Unknown".to_string())
}

/// A key-value property on an entity.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct EntityProperty {
    pub key: String, // kanon:ignore RUST/plain-string-secret -- metadata property name, not credential material (#3988)
    pub value: String,
}

/// A relationship between two entities.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Relationship {
    /// Relationship ID.
    // kanon:ignore RUST/primitive-for-domain-id — Entity/Relationship memory state mirrors server-side string IDs from the knowledge API
    pub id: String,
    /// Related entity ID.
    // kanon:ignore RUST/primitive-for-domain-id — Entity/Relationship memory state mirrors server-side string IDs from the knowledge API
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
    // kanon:ignore RUST/primitive-for-domain-id — Entity/Relationship memory state mirrors server-side string IDs from the knowledge API
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "available for filter chip count display")
    )]
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

    /// Go back one step. Returns the entity ID to move to, if available.
    #[must_use]
    pub(crate) fn back(&mut self) -> Option<&str> {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.stack.get(self.cursor).map(String::as_str)
        } else {
            None
        }
    }

    /// Go forward one step. Returns the entity ID to move to, if available.
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used in tests; available for navigation display")
    )]
    pub(crate) fn current(&self) -> Option<&str> {
        self.stack.get(self.cursor).map(String::as_str)
    }

    /// Breadcrumb trail from start to current position.
    #[must_use]
    pub(crate) fn breadcrumbs(&self) -> &[String] {
        if self.stack.is_empty() {
            &[]
        } else {
            let end = self.cursor.saturating_add(1).min(self.stack.len());
            self.stack.get(..end).unwrap_or(&[])
        }
    }

    /// Number of entries in the history.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used in tests; available for navigation display")
    )]
    pub(crate) fn len(&self) -> usize {
        self.stack.len()
    }

    /// Whether the history is empty.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used in tests; available for navigation display")
    )]
    pub(crate) fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

/// Confidence level thresholds for color coding.
#[must_use]
pub(crate) fn confidence_color(value: f64) -> &'static str {
    if value > 0.7 {
        "var(--status-success)"
    } else if value >= 0.4 {
        "var(--status-warning)"
    } else {
        "var(--status-error)"
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

// ── Facts: the default memory surface (Direction B) ──
//
// WHY: the operator's live store is ~104 facts / ~0 entities, so the
// entity-first browser lands on "No entities found". Facts are what the
// agent actually remembers; this model mirrors the flat JSON the
// `/api/v1/knowledge/facts` route serializes from `mneme::knowledge::Fact`
// (its `temporal`/`provenance`/`lifecycle`/`access` sub-structs are
// `#[serde(flatten)]`, so the wire fields are flat at the top level).

/// Classification of a remembered fact (drives decay + a readable badge).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum FactType {
    Identity,
    Preference,
    Skill,
    Relationship,
    Event,
    Task,
    Observation,
    Audit,
    Verification,
    Operational,
    Other(String),
}

impl FactType {
    /// Classify a raw server string, case-insensitively.
    ///
    /// WHY: `fact_type` is free-form text server-side; an unknown value maps
    /// to `Other` so a single odd row never fails the whole list parse.
    #[must_use]
    pub(crate) fn from_raw(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "identity" => Self::Identity,
            "preference" => Self::Preference,
            "skill" => Self::Skill,
            "relationship" => Self::Relationship,
            "event" => Self::Event,
            "task" => Self::Task,
            "observation" => Self::Observation,
            "audit" => Self::Audit,
            "verification" => Self::Verification,
            "operational" => Self::Operational,
            _ => Self::Other(raw.to_string()),
        }
    }

    /// Fixed fact types offered as filter chips, in display order.
    pub(crate) const FILTERABLE: &[Self] = &[
        Self::Preference,
        Self::Skill,
        Self::Identity,
        Self::Event,
        Self::Task,
        Self::Observation,
    ];

    /// Wire value sent to the `fact_type` query parameter.
    #[must_use]
    pub(crate) fn wire(&self) -> &str {
        match self {
            Self::Identity => "identity",
            Self::Preference => "preference",
            Self::Skill => "skill",
            Self::Relationship => "relationship",
            Self::Event => "event",
            Self::Task => "task",
            Self::Observation => "observation",
            Self::Audit => "audit",
            Self::Verification => "verification",
            Self::Operational => "operational",
            Self::Other(s) => s.as_str(),
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Identity => "Identity",
            Self::Preference => "Preference",
            Self::Skill => "Skill",
            Self::Relationship => "Relationship",
            Self::Event => "Event",
            Self::Task => "Task",
            Self::Observation => "Observation",
            Self::Audit => "Audit",
            Self::Verification => "Verification",
            Self::Operational => "Operational",
            Self::Other(s) => s.as_str(),
        }
    }

    /// Badge accent color token for this fact type.
    #[must_use]
    pub(crate) fn color(&self) -> &'static str {
        match self {
            Self::Identity => "var(--accent)",
            Self::Preference => "var(--status-info)",
            Self::Skill => "var(--status-success)",
            Self::Relationship => "#9A7BD0",
            Self::Event => "var(--status-warning)",
            Self::Task => "#C08A4A",
            Self::Observation => "var(--text-muted)",
            Self::Audit => "#7A8AA0",
            Self::Verification => "#5A9A8A",
            Self::Operational => "var(--text-secondary)",
            Self::Other(_) => "var(--text-secondary)",
        }
    }
}

/// Epistemic trust tier — how a fact was established.
///
/// WHY: this is the stated-vs-inferred axis. What the operator *told* the
/// agent (`Verified`) must visually outrank what it *guessed* (`Assumed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FactTier {
    Verified,
    Reflected,
    Inferred,
    Assumed,
    Training,
    Unknown,
}

impl FactTier {
    /// Classify a raw server string, case-insensitively.
    #[must_use]
    pub(crate) fn from_raw(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "verified" => Self::Verified,
            "reflected" => Self::Reflected,
            "inferred" => Self::Inferred,
            "assumed" => Self::Assumed,
            "training" => Self::Training,
            _ => Self::Unknown,
        }
    }

    /// Tiers offered as filter chips. Only `verified`/`inferred`/`assumed`
    /// are accepted by the route's `tier` parameter.
    pub(crate) const FILTERABLE: &[Self] = &[Self::Verified, Self::Inferred, Self::Assumed];

    /// Wire value sent to the `tier` query parameter.
    #[must_use]
    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Reflected => "reflected",
            Self::Inferred => "inferred",
            Self::Assumed => "assumed",
            Self::Training => "training",
            Self::Unknown => "",
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Verified => "Verified",
            Self::Reflected => "Reflected",
            Self::Inferred => "Inferred",
            Self::Assumed => "Assumed",
            Self::Training => "Training",
            Self::Unknown => "Unknown",
        }
    }

    /// Badge color token.
    #[must_use]
    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Verified => "var(--status-success)",
            Self::Reflected => "var(--status-info)",
            Self::Inferred => "var(--status-warning)",
            Self::Assumed => "var(--status-error)",
            Self::Training => "var(--text-muted)",
            Self::Unknown => "var(--text-muted)",
        }
    }

    /// Whether the operator (or ground truth) stated this fact, as opposed to
    /// the agent inferring it. Drives the accent border + solid glyph.
    #[must_use]
    pub(crate) fn is_stated(self) -> bool {
        matches!(self, Self::Verified | Self::Reflected)
    }

    /// Provenance glyph: solid for stated facts, dotted-ring for guessed.
    #[must_use]
    pub(crate) fn glyph(self) -> &'static str {
        if self.is_stated() {
            "\u{25cf}"
        } else {
            "\u{25cc}"
        }
    }
}

/// Data-sovereignty sensitivity — gates which providers may receive a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FactSensitivity {
    Public,
    Internal,
    Confidential,
}

impl FactSensitivity {
    /// Classify a raw server string; unknown values fall back to `Public`
    /// (the serde default on the server side).
    #[must_use]
    pub(crate) fn from_raw(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "internal" => Self::Internal,
            "confidential" => Self::Confidential,
            _ => Self::Public,
        }
    }

    /// All variants in escalating-restriction order, for the change dialog.
    pub(crate) const ALL: &[Self] = &[Self::Public, Self::Internal, Self::Confidential];

    /// Wire value for the sensitivity mutation body.
    #[must_use]
    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Confidential => "confidential",
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Public => "Public",
            Self::Internal => "Internal",
            Self::Confidential => "Confidential",
        }
    }

    /// Badge color token — more restrictive reads more urgent.
    #[must_use]
    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Public => "var(--text-muted)",
            Self::Internal => "var(--status-warning)",
            Self::Confidential => "var(--status-error)",
        }
    }
}

/// Visibility — how broadly a fact may be shared across agents/sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FactVisibility {
    Private,
    Shared,
    Restricted,
    Published,
}

impl FactVisibility {
    /// Classify a raw server string; unknown values fall back to `Private`.
    #[must_use]
    pub(crate) fn from_raw(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "shared" => Self::Shared,
            "restricted" => Self::Restricted,
            "published" => Self::Published,
            _ => Self::Private,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Private => "Private",
            Self::Shared => "Shared",
            Self::Restricted => "Restricted",
            Self::Published => "Published",
        }
    }

    /// Badge color token — broader sharing reads more notable.
    #[must_use]
    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Private => "var(--text-muted)",
            Self::Shared => "var(--status-info)",
            Self::Restricted => "var(--status-warning)",
            Self::Published => "var(--status-error)",
        }
    }
}

/// Team-memory scope for a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MemoryScope {
    User,
    Feedback,
    Project,
    Reference,
}

impl MemoryScope {
    /// Classify a raw server string; unknown values fall back to `User`.
    #[must_use]
    pub(crate) fn from_raw(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "feedback" => Self::Feedback,
            "project" => Self::Project,
            "reference" => Self::Reference,
            _ => Self::User,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Feedback => "Feedback",
            Self::Project => "Project",
            Self::Reference => "Reference",
        }
    }
}

/// Reason a fact was forgotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ForgetReason {
    UserRequested,
    Outdated,
    Incorrect,
    Privacy,
    Stale,
    Superseded,
    Contradicted,
}

impl ForgetReason {
    /// Classify a raw server string; unknown values fall back to `UserRequested`.
    #[must_use]
    pub(crate) fn from_raw(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "outdated" => Self::Outdated,
            "incorrect" => Self::Incorrect,
            "privacy" => Self::Privacy,
            "stale" => Self::Stale,
            "superseded" => Self::Superseded,
            "contradicted" => Self::Contradicted,
            _ => Self::UserRequested,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::UserRequested => "user requested",
            Self::Outdated => "outdated",
            Self::Incorrect => "incorrect",
            Self::Privacy => "privacy",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
            Self::Contradicted => "contradicted",
        }
    }
}

/// A remembered fact. Mirrors the flat JSON served by `/knowledge/facts`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(from = "FactRaw")]
pub(crate) struct Fact {
    /// Stable fact identifier.
    // kanon:ignore RUST/primitive-for-domain-id — mirrors the server-side string FactId from the knowledge API
    pub id: String,
    /// Agent (nous) that owns this fact.
    pub nous_id: String,
    /// Human-readable statement — the row headline.
    pub content: String,
    /// Classification.
    pub fact_type: FactType,
    /// Epistemic trust tier.
    pub tier: FactTier,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f64,
    /// Data-sovereignty classification.
    pub sensitivity: FactSensitivity,
    /// Sharing visibility.
    pub visibility: FactVisibility,
    /// System recording time (ISO 8601).
    pub recorded_at: String,
    /// Number of recalls.
    pub access_count: u32,
    /// Whether the fact has been forgotten (soft-deleted).
    pub is_forgotten: bool,

    // -- Provenance / lifecycle fields preserved from the backend Fact --
    /// Session that produced this fact, if known.
    pub source_session_id: Option<String>,
    /// When this fact became valid in the domain.
    pub valid_from: String,
    /// When this fact ceases to be valid in the domain.
    pub valid_to: String,
    /// Base FSRS stability in hours.
    pub stability_hours: f64,

    // -- Lifecycle --
    /// ID of the fact that replaced this one, if any.
    pub superseded_by: Option<String>,
    /// When the fact was forgotten, if it has been.
    pub forgotten_at: Option<String>,
    /// Why the fact was forgotten, if applicable.
    pub forget_reason: Option<ForgetReason>,

    // -- Access --
    /// Timestamp of the most recent recall, if any.
    pub last_accessed_at: Option<String>,

    // -- Scope / project --
    /// Team-memory scope, if the server provided one.
    pub scope: Option<MemoryScope>,
    /// Project partition, if the server provided one.
    pub project_id: Option<String>,
}

/// Raw wire shape for [`Fact`]; tolerant of missing fields.
#[derive(Debug, Clone, serde::Deserialize)]
struct FactRaw {
    id: String,
    #[serde(default)]
    nous_id: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    fact_type: String,
    #[serde(default)]
    tier: String,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    sensitivity: String,
    #[serde(default)]
    visibility: String,
    #[serde(default)]
    recorded_at: String,
    #[serde(default)]
    access_count: u32,
    #[serde(default)]
    is_forgotten: bool,
    #[serde(default)]
    source_session_id: Option<String>,
    #[serde(default)]
    valid_from: String,
    #[serde(default)]
    valid_to: String,
    #[serde(default)]
    stability_hours: f64,
    #[serde(default)]
    superseded_by: Option<String>,
    #[serde(default)]
    forgotten_at: Option<String>,
    #[serde(default)]
    forget_reason: String,
    #[serde(default)]
    last_accessed_at: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
}

impl From<FactRaw> for Fact {
    fn from(raw: FactRaw) -> Self {
        Self {
            fact_type: FactType::from_raw(&raw.fact_type),
            tier: FactTier::from_raw(&raw.tier),
            sensitivity: FactSensitivity::from_raw(&raw.sensitivity),
            visibility: FactVisibility::from_raw(&raw.visibility),
            forget_reason: if raw.forget_reason.is_empty() {
                None
            } else {
                Some(ForgetReason::from_raw(&raw.forget_reason))
            },
            scope: raw.scope.as_deref().map(MemoryScope::from_raw),
            id: raw.id,
            nous_id: raw.nous_id,
            content: raw.content,
            confidence: raw.confidence,
            recorded_at: raw.recorded_at,
            access_count: raw.access_count,
            is_forgotten: raw.is_forgotten,
            source_session_id: raw.source_session_id,
            valid_from: raw.valid_from,
            valid_to: raw.valid_to,
            stability_hours: raw.stability_hours,
            superseded_by: raw.superseded_by,
            forgotten_at: raw.forgotten_at,
            last_accessed_at: raw.last_accessed_at,
            project_id: raw.project_id,
        }
    }
}

/// Sort field for the fact list. Maps directly to the route's `sort` param.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FactSort {
    /// Highest confidence first.
    #[default]
    Confidence,
    /// Most recently recorded first.
    Recency,
    /// Most-recalled first.
    AccessCount,
}

impl FactSort {
    /// All sort options in display order.
    pub(crate) const ALL: &[Self] = &[Self::Confidence, Self::Recency, Self::AccessCount];

    /// Wire value for the `sort` query parameter.
    #[must_use]
    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::Confidence => "confidence",
            Self::Recency => "recency",
            Self::AccessCount => "access_count",
        }
    }

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Confidence => "Confidence",
            Self::Recency => "Most recent",
            Self::AccessCount => "Most recalled",
        }
    }
}

/// Recency window filter applied client-side over `recorded_at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FactRecency {
    /// No recency constraint.
    #[default]
    All,
    /// Recorded within the last 7 days.
    Week,
    /// Recorded within the last 30 days.
    Month,
    /// Older than 30 days (stale).
    Stale,
}

impl FactRecency {
    /// All windows in display order.
    pub(crate) const ALL: &[Self] = &[Self::All, Self::Week, Self::Month, Self::Stale];

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "Any age",
            Self::Week => "Last 7d",
            Self::Month => "Last 30d",
            Self::Stale => "Stale >30d",
        }
    }

    /// Whether a fact aged `age_days` satisfies this window.
    #[must_use]
    pub(crate) fn matches(self, age_days: u64) -> bool {
        match self {
            Self::All => true,
            Self::Week => age_days <= 7,
            Self::Month => age_days <= 30,
            Self::Stale => age_days > 30,
        }
    }
}

/// Number of seconds in a day, for age computation.
const SECS_PER_DAY: u64 = 86_400;

/// Age of an ISO timestamp in whole days from now. Returns `None` when the
/// timestamp is empty or unparseable.
#[must_use]
pub(crate) fn age_in_days(recorded_at: &str) -> Option<u64> {
    let ts = crate::state::sessions::parse_iso_to_unix(recorded_at)?;
    if ts == 0 {
        return None;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(now.saturating_sub(ts) / SECS_PER_DAY)
}

/// At-a-glance health of the memory store, computed from the loaded facts.
///
/// WHY: folds the stranded `/meta` "Memory Health" view into an always-on
/// strip above the list, so "is my memory healthy?" is answered in place.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct FactHealth {
    /// Active (non-forgotten) facts.
    pub active: usize,
    /// All facts including forgotten ones.
    pub total: usize,
    /// Active facts older than 30 days.
    pub stale: usize,
    /// Active facts with confidence below 0.4.
    pub low_confidence: usize,
    /// Facts marked forgotten (soft-deleted but recoverable).
    pub forgotten: usize,
    /// Mean confidence across active facts.
    pub avg_confidence: f64,
}

impl FactHealth {
    /// Compute the strip from the currently loaded facts.
    ///
    /// `backend_total` is the authoritative server-reported total; the loaded
    /// subset may be partial, so other counts are scoped to what we currently
    /// hold. Callers should label the strip as "loaded view" when the subset
    /// is known to be partial.
    #[must_use]
    pub(crate) fn compute(facts: &[Fact], backend_total: usize) -> Self {
        let loaded_total = facts.len();
        let active: Vec<&Fact> = facts.iter().filter(|f| !f.is_forgotten).collect();
        let active_count = active.len();
        let forgotten = loaded_total - active_count;
        let stale = active
            .iter()
            .filter(|f| age_in_days(&f.recorded_at).is_some_and(|d| d > 30))
            .count();
        let low_confidence = active.iter().filter(|f| f.confidence < 0.4).count();
        let avg_confidence = if active_count > 0 {
            active.iter().map(|f| f.confidence).sum::<f64>() / active_count as f64
        } else {
            0.0
        };
        Self {
            active: active_count,
            total: backend_total,
            stale,
            low_confidence,
            forgotten,
            avg_confidence,
        }
    }
}

/// Which facts the operator wants to review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FactReviewMode {
    /// Only active (non-forgotten) facts.
    #[default]
    Active,
    /// Only forgotten facts.
    Forgotten,
    /// Both active and forgotten facts.
    All,
}

impl FactReviewMode {
    /// All review modes in display order.
    pub(crate) const ALL: &[Self] = &[Self::Active, Self::Forgotten, Self::All];

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Forgotten => "Forgotten",
            Self::All => "All",
        }
    }

    /// Whether forgotten facts should be requested from the server.
    #[must_use]
    pub(crate) fn include_forgotten(self) -> bool {
        matches!(self, Self::Forgotten | Self::All)
    }

    /// Whether an active (non-forgotten) fact passes this mode.
    #[must_use]
    pub(crate) fn active_passes(self, is_forgotten: bool) -> bool {
        match self {
            Self::Active => !is_forgotten,
            Self::Forgotten => is_forgotten,
            Self::All => true,
        }
    }
}

/// Authoritative payload returned by a successful facts fetch.
#[derive(Debug, Clone)]
pub(crate) struct FactListData {
    /// Loaded facts (already client-side filtered by type/tier if applicable).
    pub facts: Vec<Fact>,
    /// Active facts among the loaded subset.
    pub active_count: usize,
    /// Authoritative total reported by the server.
    pub total_count: usize,
    /// `true` when the payload came from the legacy bare-array wire shape.
    ///
    /// WHY: the backend originally returned `[Fact]` directly; this path is
    /// preserved for compatibility but is visibly distinct from a successful
    /// envelope parse and from a decode failure.
    pub legacy_array: bool,
}

/// Classification of a facts fetch failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FactListErrorKind {
    /// Network/transport failure (no HTTP response arrived).
    Connection,
    /// HTTP response with a non-2xx status code.
    Non2xx(u16),
    /// Response body could not be decoded as the expected envelope or legacy array.
    Decode,
    /// Data is stale/unavailable (e.g., previous load exists but refresh failed).
    Unavailable,
}

/// Structured error surfaced in the memory UI.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FactListError {
    pub kind: FactListErrorKind,
    pub message: String,
}

/// Lifecycle state for the memory fact list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FactListState {
    /// Fetch in progress; previous data, if any, is shown dimmed/stale.
    Loading,
    /// Fresh data loaded from the `{facts, total}` envelope.
    Loaded,
    /// Server returned success with zero facts.
    Empty,
    /// Fetch or decode failed.
    Error(FactListError),
}

/// Paginated fact list with sort and filter state (default memory surface).
#[derive(Debug, Clone)]
pub(crate) struct FactListStore {
    /// All loaded facts (the strip + filters derive from these).
    pub facts: Vec<Fact>,
    /// Active facts among the loaded subset.
    pub active_count: usize,
    /// Authoritative total including forgotten facts reported by the server.
    pub total_count: usize,
    /// Current lifecycle state of the fetch.
    pub state: FactListState,
    /// `true` if the current payload came from the legacy bare-array shape.
    pub legacy_array: bool,
    /// Current sort field.
    pub sort: FactSort,
    /// Free-text content filter.
    pub search_query: String,
    /// Active fact-type chips (empty = all types).
    pub type_filter: Vec<FactType>,
    /// Active trust-tier chips (empty = all tiers).
    pub tier_filter: Vec<FactTier>,
    /// Recency window.
    pub recency: FactRecency,
    /// Review mode controlling whether forgotten facts are fetched/shown.
    pub review_mode: FactReviewMode,
}

impl Default for FactListStore {
    fn default() -> Self {
        Self {
            facts: Vec::new(),
            active_count: 0,
            total_count: 0,
            state: FactListState::Empty,
            legacy_array: false,
            sort: FactSort::default(),
            search_query: String::new(),
            type_filter: Vec::new(),
            tier_filter: Vec::new(),
            recency: FactRecency::All,
            review_mode: FactReviewMode::default(),
        }
    }
}

impl FactListStore {
    /// Maximum facts fetched per request (the store is ~hundreds of facts).
    pub(crate) const FETCH_LIMIT: usize = 500;

    /// Mark the store as loading, preserving any existing data as stale.
    pub(crate) fn set_loading(&mut self) {
        self.state = FactListState::Loading;
    }

    /// Replace the fact list with fresh data and update lifecycle state.
    pub(crate) fn load(&mut self, data: FactListData) {
        self.facts = data.facts;
        self.active_count = data.active_count;
        self.total_count = data.total_count;
        self.legacy_array = data.legacy_array;
        self.state = if self.facts.is_empty() && self.total_count == 0 {
            FactListState::Empty
        } else {
            FactListState::Loaded
        };
    }

    /// Record a fetch or decode failure, preserving existing data as stale.
    pub(crate) fn set_error(&mut self, error: FactListError) {
        self.state = FactListState::Error(error);
    }

    /// Reset all filters and search; review mode is preserved.
    pub(crate) fn clear_filters(&mut self) {
        self.search_query.clear();
        self.type_filter.clear();
        self.tier_filter.clear();
        self.recency = FactRecency::All;
    }

    /// Whether any filter or search is active.
    #[must_use]
    pub(crate) fn has_active_filters(&self) -> bool {
        !self.search_query.is_empty()
            || !self.type_filter.is_empty()
            || !self.tier_filter.is_empty()
            || self.recency != FactRecency::All
    }

    /// Facts passing the in-memory review-mode and recency windows.
    #[must_use]
    pub(crate) fn visible(&self) -> Vec<&Fact> {
        self.facts
            .iter()
            .filter(|f| self.review_mode.active_passes(f.is_forgotten))
            .filter(|f| match age_in_days(&f.recorded_at) {
                Some(age) => self.recency.matches(age),
                // WHY: an unparseable timestamp only fails a constrained window.
                None => self.recency == FactRecency::All,
            })
            .collect()
    }

    /// Health strip derived from the loaded facts.
    ///
    /// Uses the authoritative server-reported total so the strip does not
    /// pretend a partial loaded subset is the entire memory store.
    #[must_use]
    pub(crate) fn health(&self) -> FactHealth {
        FactHealth::compute(&self.facts, self.total_count)
    }

    /// Server-facing value for the `include_forgotten` query parameter.
    #[must_use]
    pub(crate) fn include_forgotten(&self) -> bool {
        self.review_mode.include_forgotten()
    }
}

// ── Conversions from Skene typed DTOs into local view models (#4870) ──

use skene::api::types as skene_types;

impl From<skene_types::Fact> for Fact {
    fn from(dto: skene_types::Fact) -> Self {
        Self {
            id: dto.id,
            nous_id: dto.nous_id,
            content: dto.content,
            fact_type: FactType::from_raw(&dto.fact_type),
            tier: FactTier::from_raw(&dto.tier),
            confidence: dto.confidence,
            sensitivity: FactSensitivity::from_raw(&dto.sensitivity),
            visibility: FactVisibility::from_raw(&dto.visibility),
            recorded_at: dto.recorded_at,
            access_count: dto.access_count,
            is_forgotten: dto.is_forgotten,
            source_session_id: dto.source_session_id,
            valid_from: dto.valid_from,
            valid_to: dto.valid_to,
            stability_hours: dto.stability_hours,
            superseded_by: dto.superseded_by,
            forgotten_at: dto.forgotten_at,
            forget_reason: dto.forget_reason.as_deref().map(ForgetReason::from_raw),
            last_accessed_at: dto.last_accessed_at,
            scope: dto.scope.as_deref().map(MemoryScope::from_raw),
            project_id: dto.project_id,
        }
    }
}

impl From<skene_types::EntityListItem> for Entity {
    fn from(dto: skene_types::EntityListItem) -> Self {
        Self {
            id: dto.id,
            name: dto.name,
            entity_type: EntityType::from_raw(dto.entity_type),
            confidence: dto.confidence,
            page_rank: dto.page_rank,
            memory_count: dto.memory_count,
            relationship_count: dto.relationship_count,
            properties: Vec::new(),
            updated_at: Some(dto.updated_at).filter(|s| !s.is_empty()),
            created_by: None,
            created_at: Some(dto.created_at).filter(|s| !s.is_empty()),
            flagged: false,
        }
    }
}

impl From<skene_types::Entity> for Entity {
    fn from(dto: skene_types::Entity) -> Self {
        Self {
            id: dto.id,
            name: dto.name,
            entity_type: EntityType::from_raw(dto.entity_type),
            confidence: 0.0,
            page_rank: 0.0,
            memory_count: 0,
            relationship_count: 0,
            properties: Vec::new(),
            updated_at: Some(dto.updated_at).filter(|s| !s.is_empty()),
            created_by: None,
            created_at: Some(dto.created_at).filter(|s| !s.is_empty()),
            flagged: false,
        }
    }
}

impl From<skene_types::EntityRelationship> for Relationship {
    fn from(dto: skene_types::EntityRelationship) -> Self {
        Self {
            id: dto.id,
            entity_id: dto.entity_id,
            entity_name: dto.entity_name,
            relationship_type: dto.relationship_type,
            direction: match dto.direction {
                skene_types::RelationshipDirection::Outgoing => RelationshipDirection::Outgoing,
                skene_types::RelationshipDirection::Incoming => RelationshipDirection::Incoming,
            },
            confidence: dto.confidence,
        }
    }
}

impl From<skene_types::EntityMemory> for EntityMemory {
    fn from(dto: skene_types::EntityMemory) -> Self {
        Self {
            id: dto.id,
            content: dto.content,
            agent: dto.agent,
            session: dto.session,
            confidence: dto.confidence,
            created_at: dto.created_at,
        }
    }
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

    // ── Conversions from Skene typed DTOs (#4870) ──

    #[test]
    fn fact_from_skene_dto_preserves_lifecycle_fields() {
        let dto: skene_types::Fact = serde_json::from_str(
            r#"{
                "id": "fact_01",
                "nous_id": "agent-1",
                "content": "Tabs > spaces",
                "fact_type": "preference",
                "tier": "verified",
                "confidence": 0.92,
                "sensitivity": "internal",
                "visibility": "private",
                "scope": "user",
                "project_id": "acme.corp/website",
                "valid_from": "2026-06-01T00:00:00Z",
                "valid_to": "9999-01-01T00:00:00Z",
                "recorded_at": "2026-06-01T12:00:00Z",
                "source_session_id": "session-7",
                "stability_hours": 8760.0,
                "access_count": 4,
                "last_accessed_at": "2026-06-10T08:00:00Z",
                "is_forgotten": true,
                "forgotten_at": "2026-06-11T00:00:00Z",
                "forget_reason": "outdated",
                "superseded_by": "fact_03"
            }"#,
        )
        .expect("valid skene Fact json");
        let fact = Fact::from(dto);
        assert_eq!(fact.id, "fact_01");
        assert_eq!(fact.fact_type, FactType::Preference);
        assert_eq!(fact.tier, FactTier::Verified);
        assert!((fact.confidence - 0.92).abs() < f64::EPSILON);
        assert!(fact.is_forgotten);
        assert_eq!(fact.forget_reason, Some(ForgetReason::Outdated));
        assert_eq!(fact.superseded_by.as_deref(), Some("fact_03"));
        assert_eq!(fact.scope, Some(MemoryScope::User));
        assert_eq!(fact.project_id.as_deref(), Some("acme.corp/website"));
    }

    #[test]
    fn entity_from_skene_list_item_preserves_pagination_fields() {
        let dto: skene_types::EntityListItem = serde_json::from_str(
            r#"{
                "id": "e1",
                "name": "Alpha",
                "entity_type": "concept",
                "aliases": ["a"],
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-02-01T00:00:00Z",
                "confidence": 0.9,
                "page_rank": 0.12,
                "memory_count": 3,
                "relationship_count": 2
            }"#,
        )
        .expect("valid skene EntityListItem json");
        let entity = Entity::from(dto);
        assert_eq!(entity.id, "e1");
        assert_eq!(entity.name, "Alpha");
        assert_eq!(entity.entity_type, EntityType::Concept);
        assert!((entity.page_rank - 0.12).abs() < f64::EPSILON);
        assert_eq!(entity.memory_count, 3);
        assert_eq!(entity.relationship_count, 2);
    }

    #[test]
    fn relationship_from_skene_dto_preserves_direction() {
        let dto: skene_types::EntityRelationship = serde_json::from_str(
            r#"{
                "id": "e1:e2:depends_on:ts",
                "entity_id": "e2",
                "entity_name": "Beta",
                "relationship_type": "depends_on",
                "direction": "Outgoing",
                "confidence": 0.85
            }"#,
        )
        .expect("valid skene EntityRelationship json");
        let rel = Relationship::from(dto);
        assert_eq!(rel.id, "e1:e2:depends_on:ts");
        assert_eq!(rel.entity_id, "e2");
        assert_eq!(rel.direction, RelationshipDirection::Outgoing);
        assert!((rel.confidence - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn entity_memory_from_skene_dto_preserves_optional_fields() {
        let dto: skene_types::EntityMemory = serde_json::from_str(
            r#"{
                "id": "m1",
                "content": "memory content",
                "agent": "agent-1",
                "session": "session-2",
                "confidence": 0.77,
                "created_at": "2026-01-01T00:00:00Z"
            }"#,
        )
        .expect("valid skene EntityMemory json");
        let mem = EntityMemory::from(dto);
        assert_eq!(mem.id, "m1");
        assert_eq!(mem.content, "memory content");
        assert_eq!(mem.agent.as_deref(), Some("agent-1"));
        assert_eq!(mem.session.as_deref(), Some("session-2"));
        assert!((mem.confidence - 0.77).abs() < f64::EPSILON);
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

        // Push new -- should truncate e2 and e3
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
        assert_eq!(confidence_color(0.8), "var(--status-success)");
        assert_eq!(confidence_color(0.71), "var(--status-success)");
        assert_eq!(confidence_color(0.7), "var(--status-warning)");
        assert_eq!(confidence_color(0.5), "var(--status-warning)");
        assert_eq!(confidence_color(0.4), "var(--status-warning)");
        assert_eq!(confidence_color(0.39), "var(--status-error)");
        assert_eq!(confidence_color(0.0), "var(--status-error)");
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
            assert!(!et.color().is_empty());
        }
        let other = EntityType::Other("Custom".to_string());
        assert_eq!(other.label(), "Custom");
    }

    #[test]
    fn entity_type_deserialize_is_case_insensitive_with_other_fallback() {
        let person: EntityType = serde_json::from_str(r#""person""#).expect("lowercase parses");
        assert_eq!(person, EntityType::Person);

        let shouty: EntityType = serde_json::from_str(r#""ORGANIZATION""#).expect("upper parses");
        assert_eq!(shouty, EntityType::Organization);

        let unknown: EntityType = serde_json::from_str(r#""depends_on""#).expect("unknown parses");
        assert_eq!(unknown, EntityType::Other("depends_on".to_string()));
    }

    #[test]
    fn entity_with_non_canonical_type_still_deserializes() {
        let entity: Entity =
            serde_json::from_str(r#"{"id":"e1","name":"Alpha","entity_type":"weird_type"}"#)
                .expect("entity parses despite unknown type");
        assert_eq!(
            entity.entity_type,
            EntityType::Other("weird_type".to_string())
        );
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
            assert!(!sev.color().is_empty());
        }
    }

    #[test]
    fn relationship_direction_arrows() {
        assert_eq!(RelationshipDirection::Outgoing.arrow(), "→");
        assert_eq!(RelationshipDirection::Incoming.arrow(), "←");
    }

    // ── Facts ──

    #[test]
    fn fact_deserializes_from_flat_wire_shape() {
        // WHY: mirror the flattened JSON the /knowledge/facts route emits.
        let json = r#"{
            "id": "fact_01",
            "nous_id": "agent-1",
            "content": "The operator prefers tabs over spaces",
            "fact_type": "preference",
            "tier": "verified",
            "confidence": 0.92,
            "sensitivity": "internal",
            "visibility": "private",
            "recorded_at": "2026-06-01T00:00:00Z",
            "access_count": 4,
            "is_forgotten": false
        }"#;
        let fact: Fact = serde_json::from_str(json).expect("fact parses");
        assert_eq!(fact.id, "fact_01");
        assert_eq!(fact.fact_type, FactType::Preference);
        assert_eq!(fact.tier, FactTier::Verified);
        assert_eq!(fact.sensitivity, FactSensitivity::Internal);
        assert_eq!(fact.visibility, FactVisibility::Private);
        assert!((fact.confidence - 0.92).abs() < f64::EPSILON);
        assert_eq!(fact.access_count, 4);
        assert!(!fact.is_forgotten);
    }

    #[test]
    fn fact_tolerates_unknown_and_missing_fields() {
        let fact: Fact =
            serde_json::from_str(r#"{"id":"f","fact_type":"weird"}"#).expect("partial fact parses");
        assert_eq!(fact.fact_type, FactType::Other("weird".to_string()));
        assert_eq!(fact.tier, FactTier::Unknown);
        // Server-side serde defaults: sensitivity=public, visibility=private.
        assert_eq!(fact.sensitivity, FactSensitivity::Public);
        assert_eq!(fact.visibility, FactVisibility::Private);
        assert!(fact.content.is_empty());
    }

    #[test]
    fn fact_tier_stated_vs_inferred_rank() {
        assert!(FactTier::Verified.is_stated());
        assert!(FactTier::Reflected.is_stated());
        assert!(!FactTier::Inferred.is_stated());
        assert!(!FactTier::Assumed.is_stated());
        assert_eq!(FactTier::Verified.glyph(), "\u{25cf}");
        assert_eq!(FactTier::Assumed.glyph(), "\u{25cc}");
    }

    #[test]
    fn fact_type_filter_chips_have_labels_colors_and_wire() {
        for ft in FactType::FILTERABLE {
            assert!(!ft.label().is_empty());
            assert!(!ft.color().is_empty());
            assert!(!ft.wire().is_empty());
        }
    }

    #[test]
    fn fact_tier_filter_chips_map_to_route_params() {
        for tier in FactTier::FILTERABLE {
            assert!(!tier.wire().is_empty());
        }
    }

    #[test]
    fn fact_sort_and_recency_labels() {
        for s in FactSort::ALL {
            assert!(!s.label().is_empty());
            assert!(!s.wire().is_empty());
        }
        for r in FactRecency::ALL {
            assert!(!r.label().is_empty());
        }
    }

    #[test]
    fn fact_recency_window_matches() {
        assert!(FactRecency::All.matches(999));
        assert!(FactRecency::Week.matches(7));
        assert!(!FactRecency::Week.matches(8));
        assert!(FactRecency::Month.matches(30));
        assert!(!FactRecency::Month.matches(31));
        assert!(FactRecency::Stale.matches(31));
        assert!(!FactRecency::Stale.matches(30));
    }

    fn make_fact(id: &str, confidence: f64, forgotten: bool) -> Fact {
        Fact {
            id: id.to_string(),
            nous_id: "agent".to_string(),
            content: format!("fact {id}"),
            fact_type: FactType::Observation,
            tier: FactTier::Inferred,
            confidence,
            sensitivity: FactSensitivity::Public,
            visibility: FactVisibility::Private,
            recorded_at: "2026-06-10T00:00:00Z".to_string(),
            access_count: 0,
            is_forgotten: forgotten,
            source_session_id: None,
            valid_from: "2026-06-10T00:00:00Z".to_string(),
            valid_to: "9999-01-01T00:00:00Z".to_string(),
            stability_hours: 72.0,
            superseded_by: None,
            forgotten_at: None,
            forget_reason: None,
            last_accessed_at: None,
            scope: None,
            project_id: None,
        }
    }

    #[test]
    fn fact_health_counts_and_average() {
        let facts = vec![
            make_fact("a", 0.9, false),
            make_fact("b", 0.2, false),
            make_fact("c", 0.5, true),
        ];
        let health = FactHealth::compute(&facts, facts.len());
        assert_eq!(health.active, 2);
        assert_eq!(health.total, 3);
        assert_eq!(health.forgotten, 1);
        assert_eq!(health.low_confidence, 1);
        assert!((health.avg_confidence - 0.55).abs() < 1e-9);
    }

    #[test]
    fn fact_health_uses_authoritative_backend_total() {
        let facts = vec![make_fact("a", 0.9, false)];
        let health = FactHealth::compute(&facts, 100);
        assert_eq!(health.active, 1);
        assert_eq!(health.total, 100);
    }

    #[test]
    fn fact_list_store_filters_and_clear() {
        let mut store = FactListStore::default();
        store.load(FactListData {
            facts: vec![make_fact("a", 0.9, false)],
            active_count: 1,
            total_count: 1,
            legacy_array: false,
        });
        assert!(!store.has_active_filters());

        store.search_query = "x".to_string();
        store.type_filter = vec![FactType::Skill];
        store.tier_filter = vec![FactTier::Verified];
        store.recency = FactRecency::Week;
        assert!(store.has_active_filters());

        store.clear_filters();
        assert!(!store.has_active_filters());
        assert_eq!(store.recency, FactRecency::All);
    }

    #[test]
    fn fact_review_mode_filters_visible_facts() {
        let mut store = FactListStore::default();
        store.load(FactListData {
            facts: vec![
                make_fact("active", 0.9, false),
                make_fact("forgotten", 0.5, true),
            ],
            active_count: 1,
            total_count: 2,
            legacy_array: false,
        });

        store.review_mode = FactReviewMode::Active;
        assert_eq!(store.visible().len(), 1);
        assert!(!store.visible()[0].is_forgotten);

        store.review_mode = FactReviewMode::Forgotten;
        assert_eq!(store.visible().len(), 1);
        assert!(store.visible()[0].is_forgotten);

        store.review_mode = FactReviewMode::All;
        assert_eq!(store.visible().len(), 2);
    }

    #[test]
    fn fact_include_forgotten_matches_review_mode() {
        let mut store = FactListStore::default();
        assert!(!store.include_forgotten());

        store.review_mode = FactReviewMode::Forgotten;
        assert!(store.include_forgotten());

        store.review_mode = FactReviewMode::All;
        assert!(store.include_forgotten());
    }

    #[test]
    fn fact_deserializes_from_full_nested_wire_shape() {
        // WHY: pylon returns the full mneme::knowledge::Fact with flattened
        // temporal/provenance/lifecycle/access sub-structs; ensure we preserve
        // every field that reaches the desktop instead of silently narrowing.
        let json = r#"{
            "id": "fact_02",
            "nous_id": "agent-1",
            "content": "The operator prefers tabs over spaces",
            "fact_type": "preference",
            "tier": "verified",
            "confidence": 0.92,
            "sensitivity": "internal",
            "visibility": "private",
            "scope": "project",
            "project_id": "acme.corp/website",
            "valid_from": "2026-06-01T00:00:00Z",
            "valid_to": "9999-01-01T00:00:00Z",
            "recorded_at": "2026-06-01T12:00:00Z",
            "source_session_id": "session-7",
            "stability_hours": 8760.0,
            "access_count": 4,
            "last_accessed_at": "2026-06-10T08:00:00Z",
            "is_forgotten": true,
            "forgotten_at": "2026-06-11T00:00:00Z",
            "forget_reason": "outdated",
            "superseded_by": "fact_03"
        }"#;
        let fact: Fact = serde_json::from_str(json).expect("full fact parses");
        assert_eq!(fact.id, "fact_02");
        assert_eq!(fact.source_session_id.as_deref(), Some("session-7"));
        assert_eq!(fact.valid_from, "2026-06-01T00:00:00Z");
        assert_eq!(fact.valid_to, "9999-01-01T00:00:00Z");
        assert!((fact.stability_hours - 8760.0).abs() < f64::EPSILON);
        assert_eq!(
            fact.last_accessed_at.as_deref(),
            Some("2026-06-10T08:00:00Z")
        );
        assert_eq!(fact.scope, Some(MemoryScope::Project));
        assert_eq!(fact.project_id.as_deref(), Some("acme.corp/website"));
        assert!(fact.is_forgotten);
        assert_eq!(fact.forgotten_at.as_deref(), Some("2026-06-11T00:00:00Z"));
        assert_eq!(fact.forget_reason, Some(ForgetReason::Outdated));
        assert_eq!(fact.superseded_by.as_deref(), Some("fact_03"));
    }

    #[test]
    fn fact_scope_and_forget_reason_use_safe_fallbacks() {
        let fact: Fact = serde_json::from_str(
            r#"{"id":"f","fact_type":"weird","scope":"unknown","forget_reason":"unknown"}"#,
        )
        .expect("partial fact parses");
        assert_eq!(fact.scope, Some(MemoryScope::User));
        assert_eq!(fact.forget_reason, Some(ForgetReason::UserRequested));
    }

    // ── Fact list fetch state + parsing ──

    #[test]
    fn fact_list_store_load_sets_loaded_state() {
        let mut store = FactListStore::default();
        store.load(FactListData {
            facts: vec![make_fact("a", 0.9, false)],
            active_count: 1,
            total_count: 1,
            legacy_array: false,
        });
        assert_eq!(store.state, FactListState::Loaded);
        assert!(!store.legacy_array);
    }

    #[test]
    fn fact_list_store_load_empty_sets_empty_state() {
        let mut store = FactListStore::default();
        store.load(FactListData {
            facts: Vec::new(),
            active_count: 0,
            total_count: 0,
            legacy_array: false,
        });
        assert_eq!(store.state, FactListState::Empty);
    }

    #[test]
    fn fact_list_store_set_error_preserves_stale_data() {
        let mut store = FactListStore::default();
        store.load(FactListData {
            facts: vec![make_fact("a", 0.9, false)],
            active_count: 1,
            total_count: 1,
            legacy_array: false,
        });
        store.set_error(FactListError {
            kind: FactListErrorKind::Non2xx(503),
            message: "service unavailable".to_string(),
        });
        assert_eq!(store.facts.len(), 1);
        assert_eq!(
            store.state,
            FactListState::Error(FactListError {
                kind: FactListErrorKind::Non2xx(503),
                message: "service unavailable".to_string(),
            })
        );
    }

    #[test]
    fn fact_list_store_set_error_connection_kind_round_trips() {
        let mut store = FactListStore::default();
        store.set_error(FactListError {
            kind: FactListErrorKind::Connection,
            message: "offline".to_string(),
        });
        assert_eq!(store.state, FactListState::Error(FactListError {
            kind: FactListErrorKind::Connection,
            message: "offline".to_string(),
        }));
    }
}
