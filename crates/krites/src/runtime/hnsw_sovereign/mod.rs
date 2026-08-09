//! Sovereign Hierarchical Navigable Small World (HNSW) vector index.
//!
//! Fresh implementation of the same capability as `super::hnsw`: approximate
//! nearest-neighbour search over a multi-layer navigable small-world graph,
//! stored as rows of an ordinary index relation (store-resident and
//! transactional — this is deliberately not a self-owned in-process index;
//! callers reach vectors through a `SessionTx`, same as every other stored
//! index in this crate).
//!
//! Selected in place of `super::hnsw` via the `krites_sovereign_hnsw`
//! feature (see the `#[path]` swap in `runtime/mod.rs`). Byte-compatible
//! with the on-disk index-relation encoding `super::hnsw` produces: same key
//! layout, same value layout, same reserved-level convention for the
//! entry-point marker. A database written under one implementation reads
//! back correctly under the other.
//!
//! ## Module layout
//!
//! - [`types`]: manifest, vector cache, unified clamped distance, shared
//!   key-tuple builders
//! - [`graph`]: level search, neighbour-selection heuristic, connection
//!   pruning
//! - [`put`]: vector insertion, capacity enforcement, orphan-consistency
//!   check
//! - [`remove`]: vector removal and edge cleanup
//! - [`search`]: KNN search entry point
//! - [`adaptive`]: exact-vs-approximate strategy selection and brute-force
//!   flat search (the exact-kNN oracle)
//! - [`visited_pool`]: pooled visited-set for search traversal. Sourced
//!   directly from the sibling `hnsw` tree's file (`#[path]` below), not a
//!   second copy — the pool has no CozoDB lineage of its own (an original
//!   perf addition to this crate, not an extraction), so there is nothing
//!   here to reimplement independently.
//! - `close_reopen_tests` (test-only, `storage-fjall`): E05 close/reopen
//!   recall and the open-time cost assertion

pub(crate) mod adaptive;
#[cfg(all(test, feature = "storage-fjall"))]
mod close_reopen_tests;
mod graph;
mod put;
mod remove;
mod search;
mod types;
// WHY: `super::hnsw` cannot name the sibling tree here — under the
// `krites_sovereign_hnsw` feature, that path resolves to THIS module
// (runtime/mod.rs's own `#[path]` swap), not to the physical `hnsw/`
// directory. Pointing this module declaration's own `#[path]` at the
// sibling file is the only way to reach it, one level down from the same
// mechanism runtime/mod.rs already uses.
#[path = "../hnsw/visited_pool.rs"]
pub(crate) mod visited_pool;

pub(crate) use types::HnswIndexManifest;

/// Convert an HNSW `CompoundKey` index (usize) to the i64 representation
/// stored inside index-key `DataValue`s.
///
/// HNSW indices and sub-indices are non-negative and bounded by the number
/// of tuples in the underlying relation, so they always fit in i64 on every
/// supported target; the saturating fallback exists only to make that
/// bound explicit rather than relying on it silently.
#[inline]
pub(super) fn idx_to_i64(idx: usize) -> i64 {
    i64::try_from(idx).unwrap_or(i64::MAX)
}
