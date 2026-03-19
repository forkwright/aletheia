//! Typed Datalog query builder: compile-time schema validation for KnowledgeStore.

mod builders;
pub mod queries;
mod schema;

pub use builders::{PutBuilder, QueryBuilder, RmBuilder, ScanBuilder};
pub use schema::{
    EmbeddingsField, EntitiesField, FactEntitiesField, FactsField, Field, MergeAuditField,
    PendingMergesField, Relation, RelationshipsField,
};

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests;
