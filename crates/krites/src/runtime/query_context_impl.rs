//! `SessionTx` satisfies the query engine's [`QueryContext`] requirement.
//!
//! Every method here forwards. The value is not in the bodies but in the
//! direction of the dependency: `query/` names an interface it needs, and this
//! file is the only place that knows a `SessionTx` can serve it. Nothing under
//! `query/` mentions `SessionTx` any more.

use std::sync::Arc;

use crate::data::expr::Bytecode;
use crate::data::program::{FtsSearch, HnswSearch};
use crate::data::tuple::{Tuple, TupleIter};
use crate::data::value::{DataValue, ValidityTs, Vector};
use crate::error::InternalResult as Result;
use crate::fts::TokenizerCache;
use crate::fts::tokenizer::TextAnalyzer;
use crate::parse::SourceSpan;
use crate::query::context::QueryContext;
use crate::runtime::minhash_lsh::{HashPermutations, LshSearch};
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;

impl QueryContext for SessionTx<'_> {
    fn get_relation(&self, name: &str, lock: bool) -> Result<RelationHandle> {
        SessionTx::get_relation(self, name, lock)
    }

    fn tokenizers(&self) -> &Arc<TokenizerCache> {
        &self.tokenizers
    }

    fn hnsw_knn(
        &self,
        q: Vector,
        config: &HnswSearch,
        filter_bytecode: &Option<(Vec<Bytecode>, SourceSpan)>,
        stack: &mut Vec<DataValue>,
    ) -> Result<Vec<Tuple>> {
        SessionTx::hnsw_knn(self, q, config, filter_bytecode, stack)
    }

    fn fts_search(
        &self,
        q: &str,
        config: &FtsSearch,
        filter_code: &Option<(Vec<Bytecode>, SourceSpan)>,
        tokenizer: &TextAnalyzer,
        stack: &mut Vec<DataValue>,
    ) -> Result<Vec<Tuple>> {
        SessionTx::fts_search(self, q, config, filter_code, tokenizer, stack)
    }

    fn lsh_search(
        &self,
        q: &DataValue,
        config: &LshSearch,
        stack: &mut Vec<DataValue>,
        filter_code: &Option<(Vec<Bytecode>, SourceSpan)>,
        perms: &HashPermutations,
        tokenizer: &TextAnalyzer,
    ) -> Result<Vec<Tuple>> {
        SessionTx::lsh_search(self, q, config, stack, filter_code, perms, tokenizer)
    }

    fn relation_get(&self, handle: &RelationHandle, key: &[DataValue]) -> Result<Option<Tuple>> {
        handle.get(self, key)
    }

    // WHY each scan is boxed here: RelationHandle's scans return an opaque
    // `impl Iterator`, which cannot cross an object-safe trait boundary. The
    // box costs nothing new -- `ra/stored.rs` boxed every one of these on the
    // following line already, because TupleIter *is* Box<dyn Iterator<..>>.
    fn relation_scan_all<'a>(&'a self, handle: &'a RelationHandle) -> TupleIter<'a> {
        Box::new(handle.scan_all(self))
    }

    fn relation_skip_scan_all<'a>(
        &'a self,
        handle: &'a RelationHandle,
        valid_at: ValidityTs,
    ) -> TupleIter<'a> {
        Box::new(handle.skip_scan_all(self, valid_at))
    }

    fn relation_scan_prefix<'a>(
        &'a self,
        handle: &'a RelationHandle,
        prefix: &Tuple,
    ) -> TupleIter<'a> {
        Box::new(handle.scan_prefix(self, prefix))
    }

    fn relation_skip_scan_prefix<'a>(
        &'a self,
        handle: &'a RelationHandle,
        prefix: &Tuple,
        valid_at: ValidityTs,
    ) -> TupleIter<'a> {
        Box::new(handle.skip_scan_prefix(self, prefix, valid_at))
    }

    fn relation_scan_bounded_prefix<'a>(
        &'a self,
        handle: &'a RelationHandle,
        prefix: &[DataValue],
        lower: &[DataValue],
        upper: &[DataValue],
    ) -> TupleIter<'a> {
        Box::new(handle.scan_bounded_prefix(self, prefix, lower, upper))
    }

    fn relation_skip_scan_bounded_prefix<'a>(
        &'a self,
        handle: &'a RelationHandle,
        prefix: &Tuple,
        lower: &[DataValue],
        upper: &[DataValue],
        valid_at: ValidityTs,
    ) -> TupleIter<'a> {
        Box::new(handle.skip_scan_bounded_prefix(self, prefix, lower, upper, valid_at))
    }
}
