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
