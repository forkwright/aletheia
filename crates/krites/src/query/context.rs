//! The interface `query/` needs from a transaction, and nothing more.
//!
//! # Why this exists
//!
//! `query/` extended [`SessionTx`](crate::runtime::transact::SessionTx) by
//! inherent impl, which made "replace the query engine" mean "reimplement
//! methods on someone else's struct". The import count understated the coupling
//! badly: `query/` looks independent, but `runtime/exec.rs` called its entry
//! points as methods on a runtime type, and the relational-algebra layer
//! received `&SessionTx` and forwarded it into `RelationHandle`'s scans.
//!
//! Measured, `query/` needs exactly twelve things from a transaction: a
//! relation catalogue, a tokenizer cache, the three index searches, and seven
//! scan shapes. That is the whole surface, and it is what this trait carries.
//!
//! # Why the scans are here rather than left on `RelationHandle`
//!
//! `RelationHandle::scan_*` take `&SessionTx` and read its `store_tx` /
//! `temp_store_tx` fields directly, so `query/` could not stop naming
//! `SessionTx` while calling them. Routing the scans through this trait inverts
//! that: `query/` asks for the tuples of a relation and the implementation
//! decides where they come from. `RelationHandle` is untouched -- the impl on
//! `SessionTx` forwards to it.
//!
//! # Why `&dyn` and not a generic
//!
//! Dispatch here is per-scan and per-search, not per-tuple, so the indirection
//! is not on a hot path. More importantly the scans return
//! [`TupleIter`](crate::data::tuple::TupleIter), which is *already*
//! `Box<dyn Iterator<..>>` -- `ra/stored.rs` boxes every scan result on the
//! line after it calls one. Returning it from an object-safe trait method
//! therefore costs no allocation that was not already happening, and it keeps
//! the trait usable as `&dyn` rather than making every RA type generic.

#![expect(
    clippy::result_large_err,
    reason = "InternalError is large by design and every fallible krites surface returns it; boxing it here alone would make this one interface differ from the engine it fronts"
)]

use std::sync::Arc;

use crate::data::expr::Bytecode;
use crate::data::program::{FtsSearch, HnswSearch};
use crate::data::tuple::{Tuple, TupleIter};
use crate::data::value::{DataValue, ValidityTs, Vector};
use crate::error::InternalResult as Result;
use crate::fts::TokenizerCache;
use crate::fts::tokenizer::TextAnalyzer;
use crate::parse::SourceSpan;
use crate::runtime::minhash_lsh::{HashPermutations, LshSearch};
use crate::runtime::relation::RelationHandle;

/// What the query engine needs from a transaction.
///
/// Implemented for `SessionTx` in `runtime/`: `query/` declares the
/// requirement, `runtime/` satisfies it.
// WHY `: Sync`: semi-naive evaluation hands the context into rayon closures
// (eval.rs's epoch-zero and subsequent-epoch rule runners), which require it to
// be shared across threads. `SessionTx` is already Sync -- `StoreTx` declares
// `: Sync` as a supertrait, so `Box<dyn StoreTx>` carries it and the rest of
// SessionTx's fields are Sync by construction. Without this bound the trait
// object erases a property the concrete type has, and the parallel evaluators
// stop compiling.
pub(crate) trait QueryContext: Sync {
    /// Resolve a stored relation by name.
    fn get_relation(&self, name: &str, lock: bool) -> Result<RelationHandle>;

    /// The shared tokenizer cache, used to build analyzers for FTS and LSH.
    fn tokenizers(&self) -> &Arc<TokenizerCache>;

    fn hnsw_knn(
        &self,
        q: Vector,
        config: &HnswSearch,
        filter_bytecode: &Option<(Vec<Bytecode>, SourceSpan)>,
        stack: &mut Vec<DataValue>,
    ) -> Result<Vec<Tuple>>;

    fn fts_search(
        &self,
        q: &str,
        config: &FtsSearch,
        filter_code: &Option<(Vec<Bytecode>, SourceSpan)>,
        tokenizer: &TextAnalyzer,
        stack: &mut Vec<DataValue>,
    ) -> Result<Vec<Tuple>>;

    fn lsh_search(
        &self,
        q: &DataValue,
        config: &LshSearch,
        stack: &mut Vec<DataValue>,
        filter_code: &Option<(Vec<Bytecode>, SourceSpan)>,
        perms: &HashPermutations,
        tokenizer: &TextAnalyzer,
    ) -> Result<Vec<Tuple>>;

    // WHY the handle borrow is NOT tied to 'a: RelationHandle::scan_* declare
    // `use<'a>` capturing only the transaction, and compute owned key bounds, so
    // the returned iterator never borrows the handle. Tying it to 'a would forbid
    // scanning a handle obtained locally -- which fixed_rule/ does on every input
    // relation, resolving the handle by name immediately before scanning it.
    /// Point lookup by key.
    fn relation_get(&self, handle: &RelationHandle, key: &[DataValue]) -> Result<Option<Tuple>>;

    fn relation_scan_all<'a>(&'a self, handle: &RelationHandle) -> TupleIter<'a>;

    fn relation_skip_scan_all<'a>(
        &'a self,
        handle: &RelationHandle,
        valid_at: ValidityTs,
    ) -> TupleIter<'a>;

    fn relation_scan_prefix<'a>(&'a self, handle: &RelationHandle, prefix: &Tuple)
    -> TupleIter<'a>;

    fn relation_skip_scan_prefix<'a>(
        &'a self,
        handle: &RelationHandle,
        prefix: &Tuple,
        valid_at: ValidityTs,
    ) -> TupleIter<'a>;

    fn relation_scan_bounded_prefix<'a>(
        &'a self,
        handle: &RelationHandle,
        prefix: &[DataValue],
        lower: &[DataValue],
        upper: &[DataValue],
    ) -> TupleIter<'a>;

    fn relation_skip_scan_bounded_prefix<'a>(
        &'a self,
        handle: &RelationHandle,
        prefix: &Tuple,
        lower: &[DataValue],
        upper: &[DataValue],
        valid_at: ValidityTs,
    ) -> TupleIter<'a>;
}
