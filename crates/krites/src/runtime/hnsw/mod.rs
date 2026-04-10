//! Hierarchical Navigable Small World vector index.

pub(crate) mod adaptive;
pub(crate) mod atomic_save;
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
