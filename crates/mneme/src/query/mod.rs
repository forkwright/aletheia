//! Typed Datalog query builder: compile-time schema validation for KnowledgeStore.

mod builders;
pub mod queries;
mod schema;

pub use builders::*;
pub use schema::*;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests;
