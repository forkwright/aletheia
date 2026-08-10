//! Utility fixed rules.
//!
//! `constant` and `reorder_sort` are sovereign; the `#[path]` attributes point
//! at the `_native.rs` filenames they were authored under while their derived
//! counterparts still held the plain names. `rrf` has a live episteme consumer
//! and was never part of the land-dark scheme.
#[path = "constant_native.rs"]
pub(crate) mod constant;

#[path = "reorder_sort_native.rs"]
pub(crate) mod reorder_sort;

pub(crate) mod rrf;

pub(crate) use constant::Constant;
pub(crate) use reorder_sort::ReorderSort;
pub(crate) use rrf::ReciprocalRankFusion;
