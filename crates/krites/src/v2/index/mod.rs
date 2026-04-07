//! Index abstractions for krites v2.
//!
//! Indexes accelerate specific query patterns beyond what full relation
//! scans can achieve. Invoked via the `~relation:index_name{}` syntax
//! in Datalog queries.
//!
//! Two index types:
//! - [`HnswIndex`] — approximate nearest neighbor for embedding vectors
//! - [`FtsIndex`] — full-text search with BM25 scoring

pub mod fts;
pub mod hnsw;

use std::collections::BTreeMap;

use crate::v2::error::Result;
use crate::v2::value::Value;

// ---------------------------------------------------------------------------
// Index trait
// ---------------------------------------------------------------------------

/// An index over a stored relation for accelerated queries.
///
/// Indexes are created by `::hnsw create` or `::fts create` DDL commands
/// and queried via `~relation:index_name{... | query: $q, k: $k}` syntax.
pub trait Index: Send + Sync {
    /// Index name (e.g., "semantic_idx", "content_fts").
    fn name(&self) -> &str;

    /// Insert or update an entry in the index.
    fn upsert(&self, id: &[u8], value: &Value) -> Result<()>;

    /// Remove an entry from the index.
    fn remove(&self, id: &[u8]) -> Result<()>;

    /// Search the index. Returns `(id_bytes, score)` pairs ordered by relevance.
    ///
    /// `query`: the search term (string for FTS, vector for HNSW).
    /// `k`: maximum results to return.
    /// `params`: additional index-specific parameters (e.g., `ef` for HNSW).
    fn search(
        &self,
        query: &Value,
        k: usize,
        params: &BTreeMap<String, Value>,
    ) -> Result<Vec<(Vec<u8>, f64)>>;

    /// Number of entries in the index.
    fn len(&self) -> usize;

    /// Whether the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
