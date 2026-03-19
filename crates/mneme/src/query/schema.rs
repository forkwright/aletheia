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
}

impl Relation {
    /// Return the `CozoDB` relation name used in Datalog queries.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Facts => "facts",
            Self::Entities => "entities",
            Self::Relationships => "relationships",
            Self::Embeddings => "embeddings",
            Self::FactEntities => "fact_entities",
            Self::MergeAudit => "merge_audit",
            Self::PendingMerges => "pending_merges",
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

impl Field for FactsField {
    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::ValidFrom => "valid_from",
            Self::Content => "content",
            Self::NousId => "nous_id",
            Self::Confidence => "confidence",
            Self::Tier => "tier",
            Self::ValidTo => "valid_to",
            Self::SupersededBy => "superseded_by",
            Self::SourceSessionId => "source_session_id",
            Self::RecordedAt => "recorded_at",
            Self::AccessCount => "access_count",
            Self::LastAccessedAt => "last_accessed_at",
            Self::StabilityHours => "stability_hours",
            Self::FactType => "fact_type",
            Self::IsForgotten => "is_forgotten",
            Self::ForgottenAt => "forgotten_at",
            Self::ForgetReason => "forget_reason",
        }
    }
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

impl Field for EntitiesField {
    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::EntityType => "entity_type",
            Self::Aliases => "aliases",
            Self::CreatedAt => "created_at",
            Self::UpdatedAt => "updated_at",
        }
    }
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

impl Field for RelationshipsField {
    fn name(self) -> &'static str {
        match self {
            Self::Src => "src",
            Self::Dst => "dst",
            Self::Relation => "relation",
            Self::Weight => "weight",
            Self::CreatedAt => "created_at",
        }
    }
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

impl Field for EmbeddingsField {
    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Content => "content",
            Self::SourceType => "source_type",
            Self::SourceId => "source_id",
            Self::NousId => "nous_id",
            Self::Embedding => "embedding",
            Self::CreatedAt => "created_at",
        }
    }
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

impl Field for FactEntitiesField {
    fn name(self) -> &'static str {
        match self {
            Self::FactId => "fact_id",
            Self::EntityId => "entity_id",
            Self::CreatedAt => "created_at",
        }
    }
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

impl Field for MergeAuditField {
    fn name(self) -> &'static str {
        match self {
            Self::CanonicalId => "canonical_id",
            Self::MergedId => "merged_id",
            Self::MergedName => "merged_name",
            Self::MergeScore => "merge_score",
            Self::FactsTransferred => "facts_transferred",
            Self::RelationshipsRedirected => "relationships_redirected",
            Self::MergedAt => "merged_at",
        }
    }
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

impl Field for PendingMergesField {
    fn name(self) -> &'static str {
        match self {
            Self::EntityA => "entity_a",
            Self::EntityB => "entity_b",
            Self::NameA => "name_a",
            Self::NameB => "name_b",
            Self::NameSimilarity => "name_similarity",
            Self::EmbedSimilarity => "embed_similarity",
            Self::TypeMatch => "type_match",
            Self::AliasOverlap => "alias_overlap",
            Self::MergeScore => "merge_score",
            Self::CreatedAt => "created_at",
        }
    }
}
