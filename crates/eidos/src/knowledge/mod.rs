//! Knowledge domain types: facts, entities, relationships, and embeddings.
//!
//! These are the core data structures for the knowledge graph:
//! - **Facts**: extracted from conversations, bi-temporal (`valid_from`/`valid_to`)
//! - **Entities**: people, projects, tools, concepts with typed relationships
//! - **Vectors**: embedding-indexed for semantic recall
//! - **Memory scopes**: team memory sharing model (`User`, `Feedback`, `Project`, `Reference`)
//! - **Path validation layers**: defense-in-depth security for memory path operations

mod causal;
mod entity;
mod fact;
mod path;
mod scope;

// ── Re-exports: causal ───────────────────────────────────────────────────

pub use causal::{CausalEdge, CausalRelationType, TemporalOrdering};

// ── Re-exports: entity ───────────────────────────────────────────────────

pub use entity::{EmbeddedChunk, Entity, RecallResult, Relationship};

// ── Re-exports: fact ─────────────────────────────────────────────────────

pub use fact::{
    EpistemicTier, Fact, FactAccess, FactDiff, FactLifecycle, FactProvenance, FactTemporal,
    FactType, ForgetReason, KnowledgeStage, StageTransition, VerificationRecord,
    VerificationSource, VerificationStatus, MAX_CONTENT_LENGTH, default_stability_hours,
    far_future, format_timestamp, parse_timestamp,
};
#[cfg(test)]
pub(crate) use fact::is_far_future;

// ── Re-exports: path ─────────────────────────────────────────────────────

pub use path::{
    PathValidationError, PathValidationLayer, ValidatedPath, PATH_VALIDATION_FS_LAYERS,
    SYMLINK_HOP_LIMIT, validate_memory_path,
};

// ── Re-exports: scope ────────────────────────────────────────────────────

pub use scope::{MemoryScope, ScopeAccessPolicy};

// ── Display implementations via macro ────────────────────────────────────

/// Implement `Display` by delegating to `as_str()`.
macro_rules! display_via_as_str {
    ($($ty:ty),+ $(,)?) => {$(
        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    )+};
}

display_via_as_str!(
    EpistemicTier,
    KnowledgeStage,
    ForgetReason,
    FactType,
    VerificationSource,
    VerificationStatus,
    TemporalOrdering,
    MemoryScope,
    PathValidationLayer,
);

#[cfg(test)]
#[path = "knowledge_test.rs"]
mod tests;
