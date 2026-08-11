//! Utility fixed rules.
//!
//! `constant` and `reorder_sort` are sovereign; the `_native.rs` filenames they
//! were authored under are retired along with the derived siblings that
//! justified them. `rrf` has a live episteme consumer and was never part of the
//! land-dark scheme.
pub(crate) mod constant;
pub(crate) mod reorder_sort;
pub(crate) mod rrf;

pub(crate) use constant::Constant;
pub(crate) use reorder_sort::ReorderSort;
pub(crate) use rrf::ReciprocalRankFusion;
