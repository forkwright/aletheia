/// Datalog field reference. Implemented by per-relation field enums.
pub trait Field: Copy {
    /// Return the Datalog column name for this field.
    fn name(self) -> &'static str;
}

/// Knowledge graph relations stored in the Krites engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Relation {
    /// Temporal facts with validity windows and confidence scores.
    Facts,
    /// Named entities (people, places, concepts).
    Entities,
    /// Directed edges between entities with typed relations.
    Relationships,
    /// Vector embeddings for semantic search.
    Embeddings,
    /// Fact-to-entity membership mapping.
    FactEntities,
    /// Audit log of completed entity merges.
    MergeAudit,
    /// Queue of candidate entity merges awaiting review.
    PendingMerges,
    /// Operator review flags attached to entities.
    EntityFlags,
    /// Directed causal edges between fact nodes.
    CausalEdges,
    /// IS-A edges between entity types (schema v8).
    TypeHierarchy,
    /// Materialized output of derived-rule families (schema v8).
    DerivedFacts,
    /// Defeasible default assertions per entity+tag (schema v8).
    Defaults,
    /// Facts published for multi-agent verification (schema v10).
    PublishedFacts,
    /// Per-contributor provenance for published facts (schema v10).
    Provenance,
    /// Embedding model/dimension metadata for the vector schema.
    EmbeddingMeta,
    /// Monotonic source revision for derived-rule invalidation (schema v19).
    DerivedSourceRevision,
    /// Per-rule-family materialization watermarks (schema v19).
    DerivedRuleWatermarks,
    /// Audit log of completed consolidations (schema v5).
    ConsolidationAudit,
    /// Convergence-strength side-index for consolidated facts (schema v9).
    FactMultiplicity,
    /// Source fact/session side-index for consolidated facts (schema v19).
    ConsolidationProvenance,
    /// Graph-algorithm scores per entity (PageRank, community cluster).
    GraphScores,
    /// Applied schema version and per-migration stamps.
    SchemaVersion,
}

impl Relation {
    /// Every relation the knowledge store creates at the current schema
    /// version. The schema-coverage test compares this set against a live
    /// store's `::relations` listing so a DDL added without typed metadata
    /// fails the test suite.
    #[cfg(test)]
    pub(crate) const ALL: &[Self] = &[
        Self::Facts,
        Self::Entities,
        Self::Relationships,
        Self::Embeddings,
        Self::FactEntities,
        Self::MergeAudit,
        Self::PendingMerges,
        Self::EntityFlags,
        Self::CausalEdges,
        Self::TypeHierarchy,
        Self::DerivedFacts,
        Self::Defaults,
        Self::PublishedFacts,
        Self::Provenance,
        Self::EmbeddingMeta,
        Self::DerivedSourceRevision,
        Self::DerivedRuleWatermarks,
        Self::ConsolidationAudit,
        Self::FactMultiplicity,
        Self::ConsolidationProvenance,
        Self::GraphScores,
        Self::SchemaVersion,
    ];

    /// Return the relation name used in Datalog queries.
    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Facts => "facts",
            Self::Entities => "entities",
            Self::Relationships => "relationships",
            Self::Embeddings => "embeddings",
            Self::FactEntities => "fact_entities",
            Self::MergeAudit => "merge_audit",
            Self::PendingMerges => "pending_merges",
            Self::EntityFlags => "entity_flags",
            Self::CausalEdges => "causal_edges",
            Self::TypeHierarchy => "type_hierarchy",
            Self::DerivedFacts => "derived_facts",
            Self::Defaults => "defaults",
            Self::PublishedFacts => "published_facts",
            Self::Provenance => "provenance",
            Self::EmbeddingMeta => "embedding_meta",
            Self::DerivedSourceRevision => "derived_source_revision",
            Self::DerivedRuleWatermarks => "derived_rule_watermarks",
            Self::ConsolidationAudit => "consolidation_audit",
            Self::FactMultiplicity => "fact_multiplicity",
            Self::ConsolidationProvenance => "consolidation_provenance",
            Self::GraphScores => "graph_scores",
            Self::SchemaVersion => "schema_version",
        }
    }
}

/// Fields in the `facts` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum FactsField {
    Id,
    ValidFrom,
    Content,
    NousId,
    Confidence,
    Tier,
    ValidTo,
    SupersededBy,
    SourceSessionId,
    RecordedAt,
    AccessCount,
    LastAccessedAt,
    StabilityHours,
    FactType,
    IsForgotten,
    ForgottenAt,
    ForgetReason,
    Scope,
    ProjectId,
    Visibility,
    Sensitivity,
}

/// Fields in the `entities` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum EntitiesField {
    Id,
    Name,
    EntityType,
    Aliases,
    CreatedAt,
    UpdatedAt,
    /// Nullable embedding of [`Self::Name`]; populated by the dedup pipeline
    /// (#4165) when an `EmbeddingProvider` is in scope. NULL for entities
    /// inserted in degraded mode or before the v13 schema migration.
    NameEmbedding,
}

/// Fields in the `relationships` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum RelationshipsField {
    Src,
    Dst,
    Relation,
    Weight,
    CreatedAt,
}

/// Fields in the `embeddings` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum EmbeddingsField {
    Id,
    Content,
    SourceType,
    SourceId,
    NousId,
    Embedding,
    CreatedAt,
}

/// Fields in the `fact_entities` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum FactEntitiesField {
    FactId,
    EntityId,
    CreatedAt,
}

/// Fields in the `merge_audit` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum MergeAuditField {
    CanonicalId,
    MergedId,
    MergedName,
    MergeScore,
    FactsTransferred,
    RelationshipsRedirected,
    MergedAt,
}

/// Fields in the `pending_merges` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum PendingMergesField {
    EntityA,
    EntityB,
    NameA,
    NameB,
    NameSimilarity,
    EmbedSimilarity,
    TypeMatch,
    AliasOverlap,
    MergeScore,
    CreatedAt,
}

/// Fields in the `entity_flags` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum EntityFlagsField {
    EntityId,
    Reason,
    Severity,
    FlaggedBy,
    FlaggedAt,
}

/// Fields in the `causal_edges` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum CausalEdgesField {
    Cause,
    Effect,
    Id,
    Ordering,
    RelationshipType,
    Confidence,
    EvidenceSessionId,
    CreatedAt,
}

/// Fields in the `type_hierarchy` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum TypeHierarchyField {
    ChildType,
    ParentType,
    CreatedAt,
}

/// Fields in the `derived_facts` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum DerivedFactsField {
    EntityId,
    RuleId,
    DerivedContent,
    Confidence,
    MaterializedAt,
}

/// Fields in the `defaults` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum DefaultsField {
    EntityId,
    Tag,
    DefaultContent,
    Confidence,
    CreatedAt,
}

/// Fields in the `published_facts` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum PublishedFactsField {
    Id,
    OriginalFactId,
    PublishedBy,
    PublishedAt,
    VerificationCount,
    ContestedBy,
    ContestReason,
}

/// Fields in the `provenance` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum ProvenanceField {
    PublishedFactId,
    Contributor,
    ContributionType,
    Confidence,
    ContributedAt,
}

/// Fields in the `embedding_meta` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum EmbeddingMetaField {
    Model,
    Dim,
}

/// Fields in the `derived_source_revision` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum DerivedSourceRevisionField {
    Key,
    Revision,
}

/// Fields in the `derived_rule_watermarks` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum DerivedRuleWatermarksField {
    RuleId,
    SourceRevision,
    MaterializedAt,
    Dirty,
}

/// Fields in the `consolidation_audit` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum ConsolidationAuditField {
    Id,
    NousId,
    TriggerType,
    TriggerId,
    OriginalCount,
    ConsolidatedCount,
    OriginalFactIds,
    ConsolidatedFactIds,
    ConsolidatedAt,
}

/// Fields in the `fact_multiplicity` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum FactMultiplicityField {
    FactId,
    SourceCount,
    FirstObserved,
    LastObserved,
    TimeSpreadSeconds,
    RecordedAt,
}

/// Fields in the `consolidation_provenance` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum ConsolidationProvenanceField {
    ConsolidatedFactId,
    SourceFactIds,
    SourceSessionIds,
}

/// Fields in the `graph_scores` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum GraphScoresField {
    EntityId,
    ScoreType,
    Score,
    ClusterId,
    UpdatedAt,
}

/// Fields in the `schema_version` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
#[non_exhaustive]
pub enum SchemaVersionField {
    Key,
    Version,
}

// WHY: trait implementations live in a separate module to avoid trait-impl
// colocation.
mod field_impl;
