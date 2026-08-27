//! Field trait implementations for knowledge graph relations.

use super::{
    CausalEdgesField, ConsolidationAuditField, ConsolidationProvenanceField, DefaultsField,
    DerivedFactsField, DerivedRuleWatermarksField, DerivedSourceRevisionField, EmbeddingMetaField,
    EmbeddingsField, EntitiesField, EntityFlagsField, FactEntitiesField, FactMultiplicityField,
    FactsField, Field, GraphScoresField, MergeAuditField, PendingMergesField, ProvenanceField,
    PublishedFactsField, RelationshipsField, SchemaVersionField, TypeHierarchyField,
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
            Self::Scope => "scope",
            Self::ProjectId => "project_id",
            Self::Visibility => "visibility",
            Self::Sensitivity => "sensitivity",
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
            Self::NameEmbedding => "name_embedding",
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

impl Field for EntityFlagsField {
    fn name(self) -> &'static str {
        match self {
            Self::EntityId => "entity_id",
            Self::Reason => "reason",
            Self::Severity => "severity",
            Self::FlaggedBy => "flagged_by",
            Self::FlaggedAt => "flagged_at",
        }
    }
}

impl Field for CausalEdgesField {
    fn name(self) -> &'static str {
        match self {
            Self::Cause => "cause",
            Self::Effect => "effect",
            Self::Id => "id",
            Self::Ordering => "ordering",
            Self::RelationshipType => "relationship_type",
            Self::Confidence => "confidence",
            Self::EvidenceSessionId => "evidence_session_id",
            Self::CreatedAt => "created_at",
        }
    }
}

impl Field for TypeHierarchyField {
    fn name(self) -> &'static str {
        match self {
            Self::ChildType => "child_type",
            Self::ParentType => "parent_type",
            Self::CreatedAt => "created_at",
        }
    }
}

impl Field for DerivedFactsField {
    fn name(self) -> &'static str {
        match self {
            Self::EntityId => "entity_id",
            Self::RuleId => "rule_id",
            Self::DerivedContent => "derived_content",
            Self::Confidence => "confidence",
            Self::MaterializedAt => "materialized_at",
        }
    }
}

impl Field for DefaultsField {
    fn name(self) -> &'static str {
        match self {
            Self::EntityId => "entity_id",
            Self::Tag => "tag",
            Self::DefaultContent => "default_content",
            Self::Confidence => "confidence",
            Self::CreatedAt => "created_at",
        }
    }
}

impl Field for PublishedFactsField {
    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::OriginalFactId => "original_fact_id",
            Self::PublishedBy => "published_by",
            Self::PublishedAt => "published_at",
            Self::VerificationCount => "verification_count",
            Self::ContestedBy => "contested_by",
            Self::ContestReason => "contest_reason",
        }
    }
}

impl Field for ProvenanceField {
    fn name(self) -> &'static str {
        match self {
            Self::PublishedFactId => "published_fact_id",
            Self::Contributor => "contributor",
            Self::ContributionType => "contribution_type",
            Self::Confidence => "confidence",
            Self::ContributedAt => "contributed_at",
        }
    }
}

impl Field for EmbeddingMetaField {
    fn name(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Dim => "dim",
        }
    }
}

impl Field for DerivedSourceRevisionField {
    fn name(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Revision => "revision",
        }
    }
}

impl Field for DerivedRuleWatermarksField {
    fn name(self) -> &'static str {
        match self {
            Self::RuleId => "rule_id",
            Self::SourceRevision => "source_revision",
            Self::MaterializedAt => "materialized_at",
            Self::Dirty => "dirty",
        }
    }
}

impl Field for ConsolidationAuditField {
    fn name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::NousId => "nous_id",
            Self::TriggerType => "trigger_type",
            Self::TriggerId => "trigger_id",
            Self::OriginalCount => "original_count",
            Self::ConsolidatedCount => "consolidated_count",
            Self::OriginalFactIds => "original_fact_ids",
            Self::ConsolidatedFactIds => "consolidated_fact_ids",
            Self::ConsolidatedAt => "consolidated_at",
        }
    }
}

impl Field for FactMultiplicityField {
    fn name(self) -> &'static str {
        match self {
            Self::FactId => "fact_id",
            Self::SourceCount => "source_count",
            Self::FirstObserved => "first_observed",
            Self::LastObserved => "last_observed",
            Self::TimeSpreadSeconds => "time_spread_seconds",
            Self::RecordedAt => "recorded_at",
        }
    }
}

impl Field for ConsolidationProvenanceField {
    fn name(self) -> &'static str {
        match self {
            Self::ConsolidatedFactId => "consolidated_fact_id",
            Self::SourceFactIds => "source_fact_ids",
            Self::SourceSessionIds => "source_session_ids",
        }
    }
}

impl Field for GraphScoresField {
    fn name(self) -> &'static str {
        match self {
            Self::EntityId => "entity_id",
            Self::ScoreType => "score_type",
            Self::Score => "score",
            Self::ClusterId => "cluster_id",
            Self::UpdatedAt => "updated_at",
        }
    }
}

impl Field for SchemaVersionField {
    fn name(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Version => "version",
        }
    }
}
