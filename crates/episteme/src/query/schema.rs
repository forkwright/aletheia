/// Datalog field reference. Implemented by per-relation field enums.
pub trait Field: Copy {
    /// Return the Datalog column name for this field.
    fn name(self) -> &'static str;
}

/// Knowledge graph relations stored in the `CozoDB` engine.
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
    /// Directed causal edges between fact nodes.
    CausalEdges,
}

impl Relation {
    /// Return the `CozoDB` relation name used in Datalog queries.
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
            Self::CausalEdges => "causal_edges",
        }
    }
}

/// Fields in the `facts` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
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
}

/// Fields in the `entities` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
pub enum EntitiesField {
    Id,
    Name,
    EntityType,
    Aliases,
    CreatedAt,
    UpdatedAt,
}

/// Fields in the `relationships` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
pub enum RelationshipsField {
    Src,
    Dst,
    Relation,
    Weight,
    CreatedAt,
}

/// Fields in the `embeddings` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
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
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
pub enum FactEntitiesField {
    FactId,
    EntityId,
    CreatedAt,
}

/// Fields in the `merge_audit` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
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
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
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

/// Fields in the `causal_edges` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "field enum variants are self-documenting Datalog column names"
)]
pub enum CausalEdgesField {
    Cause,
    Effect,
    Ordering,
    Confidence,
    CreatedAt,
}

// Trait implementations are in a separate module to avoid trait-impl colocation.
mod field_impl;
