//! Typed Datalog query builder: compile-time schema validation for KnowledgeStore.

mod builders;
pub mod queries;
mod schema;

pub use builders::{PutBuilder, QueryBuilder, RmBuilder, ScanBuilder};
pub use schema::{
    CausalEdgesField, ConsolidationAuditField, ConsolidationProvenanceField, DefaultsField,
    DerivedFactsField, DerivedRuleWatermarksField, DerivedSourceRevisionField, EmbeddingMetaField,
    EmbeddingsField, EntitiesField, EntityFlagsField, FactEntitiesField, FactMultiplicityField,
    FactsField, Field, GraphScoresField, MergeAuditField, PendingMergesField, ProvenanceField,
    PublishedFactsField, Relation, RelationshipsField, SchemaVersionField, TypeHierarchyField,
};

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests;
