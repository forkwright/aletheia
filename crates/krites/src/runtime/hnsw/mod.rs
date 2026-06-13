//! Hierarchical Navigable Small World (HNSW) vector index.
//!
//! Implements the HNSW algorithm for approximate nearest-neighbor search on
//! high-dimensional vectors. The index is stored as a multi-layer navigable
//! small-world graph where upper layers provide logarithmic skip-connections
//! and the bottom layer forms a Delaunay-like proximity graph.
//!
//! ## Module layout
//!
//! - [`types`]: Core types (`HnswIndexManifest`, `VectorCache`, `CompoundKey`)
//! - [`graph`]: Graph traversal — level search, neighbor selection, connection pruning
//! - [`put`]: Vector insertion into the graph
//! - [`remove`]: Vector removal and edge cleanup
//! - [`search`]: KNN search entry point
//! - [`adaptive`]: Exact vs. approximate search strategy selection
//! - [`visited_pool`]: Lock-free visited-set pool for search traversal
//! - [`atomic_save`]: Crash-safe persistence (write-fsync-rename)
//! - [`mmap_storage`]: Memory-mapped dense vector storage

pub(crate) mod adaptive;
pub(crate) mod atomic_save;
mod graph;
pub(crate) mod mmap_storage;
mod put;
mod remove;
mod search;
mod types;
pub(crate) mod visited_pool;

pub(crate) use types::HnswIndexManifest;

/// Convert an HNSW `CompoundKey` index (usize) to the i64 representation stored
/// inside index key `DataValue`s.
///
/// HNSW indices and sub-indices are non-negative and bounded by the number of
/// tuples in the underlying relation. They are serialised as i64 DataValues in
/// index keys. Saturating to `i64::MAX` handles the theoretical
/// `usize::MAX > i64::MAX` case, which is unreachable on any supported target
/// because relation tuple counts are bounded by available address space.
#[inline]
pub(super) fn idx_to_i64(idx: usize) -> i64 {
    i64::try_from(idx).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
