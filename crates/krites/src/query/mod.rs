//! Query planning, optimization, and evaluation.
pub(crate) mod compile;
pub(crate) mod error;
pub(crate) mod eval;
pub(crate) mod graph;
pub(crate) mod logical;
pub(crate) mod magic;
pub(crate) mod ra;
pub(crate) mod reorder;
pub(crate) mod sort;
pub(crate) mod stored;
pub(crate) mod stratify;
