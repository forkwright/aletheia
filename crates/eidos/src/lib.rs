#![deny(missing_docs)]
//! aletheia-eidos: shared knowledge types for the Aletheia memory layer
//!
//! Eidos (εἶδος): "form, essence." Shared data types and provider contracts
//! with zero internal dependencies -- the foundational shapes that the rest of
//! the knowledge pipeline builds upon.

/// Shared bookkeeping provider contracts and extraction DTOs.
pub mod bookkeeping;
/// Newtype wrappers for knowledge-domain identifiers.
pub mod id;
/// Knowledge graph domain types: facts, entities, relationships, embeddings.
pub mod knowledge;
/// Uniform provenance metadata for fleet artefacts.
pub mod meta;
/// Cross-layer provenance translation between eidos and the mnemosyne JSON shape.
pub mod provenance_adapter;

/// Re-export bookkeeping provider contracts and DTOs at crate root.
pub use bookkeeping::{
    BookkeepingError, BookkeepingProvider, BookkeepingResult, ConversationMessage, EntityType,
    ExtractedEntity, ExtractedFact, ExtractedRelationship, ExtractedToolCall, Extraction,
    ExtractionSchema, Intent,
};
/// Re-export architecture-fact types at crate root.
pub use knowledge::architecture_fact::{ArchitectureFact, FactError, FactScope, FactStore};
/// Re-export canonical finding types at crate root.
pub use knowledge::finding::{EvidenceLevel, Finding};
/// Re-export provenance types at crate root.
pub use meta::{ArtefactMeta, Stamped};
/// Re-export mnemosyne interop types at crate root.
pub use provenance_adapter::{
    MnemosyneAnnotationView, MnemosyneView, from_mnemosyne_annotation, to_mnemosyne_compatible,
};
/// Shared test data builders for `Fact`, `Entity`, and `Relationship`.
#[cfg(any(test, feature = "test-support"))]
#[expect(
    clippy::expect_used,
    reason = "test fixture builders — panics are intentional"
)]
pub mod test_fixtures;
/// Training data capture types.
pub mod training;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_accessible() {
        let _ = id::FactId::new("f-1").expect("valid");
        let _ = id::EntityId::new("e-1").expect("valid");
        let _ = id::EmbeddingId::new("emb-1").expect("valid");
        let _ = id::CausalEdgeId::new("ce-1").expect("valid");
        let _ = knowledge::EpistemicTier::Verified;
        let _ = knowledge::CausalRelationType::Caused;
    }
}
