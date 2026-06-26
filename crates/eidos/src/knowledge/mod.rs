//! Knowledge domain types: facts, entities, relationships, and embeddings.
//!
//! These are the core data structures for the knowledge graph:
//! - **Facts**: extracted from conversations, bi-temporal (`valid_from`/`valid_to`)
//! - **Entities**: people, projects, tools, concepts with typed relationships
//! - **Vectors**: embedding-indexed for semantic recall
//! - **Memory scopes**: team memory sharing model (`User`, `Feedback`, `Project`, `Reference`)
//! - **Path validation layers**: defense-in-depth security for memory path operations

pub mod architecture_fact;
mod causal;
mod entity;
mod fact;
pub mod finding;
mod path;
mod scope;

// ── Re-exports: causal ───────────────────────────────────────────────────

pub use causal::{CausalEdge, CausalRelationType, TemporalOrdering};

// ── Re-exports: entity ───────────────────────────────────────────────────

pub use entity::{EmbeddedChunk, Entity, RecallResult, Relationship};

// ── Re-exports: fact ─────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) use fact::is_far_future;
pub use fact::{
    ConflictResolution, EpistemicTier, Fact, FactAccess, FactAccessGrant, FactDiff, FactLifecycle,
    FactProvenance, FactSensitivity, FactTemporal, FactType, ForgetReason, KnowledgeStage,
    MAX_CONTENT_LENGTH, PublishedFact, PublishedFactId, StageTransition, VerificationProposal,
    VerificationRecord, VerificationSource, VerificationStatus, VerificationVerdict,
    VerificationVote, Visibility, default_stability_hours, far_future, format_timestamp,
    parse_timestamp,
};

// ── Re-exports: path ─────────────────────────────────────────────────────

pub use path::{
    PATH_VALIDATION_FS_LAYERS, PathValidationError, PathValidationLayer, SYMLINK_HOP_LIMIT,
    ValidatedPath, validate_memory_path, validate_memory_path_async,
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
    FactSensitivity,
    VerificationSource,
    VerificationStatus,
    TemporalOrdering,
    MemoryScope,
    PathValidationLayer,
    Visibility,
);

#[cfg(test)]
#[path = "knowledge_tests/mod.rs"]
mod tests;
