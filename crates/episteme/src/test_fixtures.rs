//! Shared test fixtures for episteme knowledge store tests.
//!
//! Re-exports the common type builders from `eidos::test_fixtures` and adds
//! episteme-specific helpers (`make_store`) for knowledge store tests.

use std::sync::Arc;

use crate::knowledge_store::{KnowledgeConfig, KnowledgeStore};

// Re-export type builders from eidos so test modules can import everything
// from a single location.
pub(crate) use eidos::test_fixtures::{make_entity, make_fact, make_relationship, test_ts};

/// Default embedding dimension for test stores (small to keep tests fast).
pub(crate) const DIM: usize = 4;

/// Open an in-memory `KnowledgeStore` with test defaults.
pub(crate) fn make_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: DIM,
        ..Default::default()
    })
    .expect("open in-memory knowledge store for test")
}
