#![deny(missing_docs)]
//! aletheia-eidos: shared knowledge types for the Aletheia memory layer
//!
//! Eidos (εἶδος): "form, essence." Pure data types with zero internal
//! dependencies — the foundational shapes that the rest of the knowledge
//! pipeline builds upon.

/// Newtype wrappers for knowledge-domain identifiers.
pub mod id;
/// Knowledge graph domain types: facts, entities, relationships, embeddings.
pub mod knowledge;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_accessible() {
        let _ = id::FactId::new("f-1").expect("valid");
        let _ = id::EntityId::new("e-1").expect("valid");
        let _ = id::EmbeddingId::new("emb-1").expect("valid");
        let _ = knowledge::EpistemicTier::Verified;
    }
}
