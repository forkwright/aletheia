/// Datalog field reference. Implemented by per-relation field enums.
pub trait Field: Copy {
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
}

impl Relation {
    /// Return the `CozoDB` relation name used in Datalog queries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Facts => "facts",
            Self::Entities => "entities",
            Self::Relationships => "relationships",
            Self::Embeddings => "embeddings",
        }
    }
}

/// Fields in the `facts` relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
