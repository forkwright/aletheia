//! Utility fixed rules.
//!
//! `constant` and `reorder_sort` are landed **dual** (PLAN.md §2): the
//! derived implementation stays default, with a fresh sovereign rewrite
//! behind the `krites_sovereign_utilities` feature. `rrf` has a live
//! episteme consumer and is out of scope for this wave.
#[cfg(not(feature = "krites_sovereign_utilities"))]
pub(crate) mod constant;
#[cfg(feature = "krites_sovereign_utilities")]
#[path = "constant_native.rs"]
pub(crate) mod constant;

#[cfg(not(feature = "krites_sovereign_utilities"))]
pub(crate) mod reorder_sort;
#[cfg(feature = "krites_sovereign_utilities")]
#[path = "reorder_sort_native.rs"]
pub(crate) mod reorder_sort;

pub(crate) mod rrf;

pub(crate) use constant::Constant;
pub(crate) use reorder_sort::ReorderSort;
pub(crate) use rrf::ReciprocalRankFusion;
