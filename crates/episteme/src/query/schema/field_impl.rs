//! Field trait implementations for knowledge graph relations.

use super::{
    CausalEdgesField, EmbeddingsField, EntitiesField, FactEntitiesField, FactsField, Field,
    MergeAuditField, PendingMergesField, RelationshipsField,
};

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

impl Field for FactEntitiesField {
    fn name(self) -> &'static str {
        match self {
            Self::FactId => "fact_id",
            Self::EntityId => "entity_id",
            Self::CreatedAt => "created_at",
        }
    }
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

impl Field for CausalEdgesField {
    fn name(self) -> &'static str {
        match self {
            Self::Cause => "cause",
            Self::Effect => "effect",
            Self::Ordering => "ordering",
            Self::Confidence => "confidence",
            Self::CreatedAt => "created_at",
        }
    }
}
