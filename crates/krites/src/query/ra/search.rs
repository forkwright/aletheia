//! Index-based search operators: HNSW, FTS, and LSH.
//!
//! These operators perform approximate or exact searches using specialized
//! index structures attached to stored relations:
//! - **HNSW**: approximate nearest neighbor via hierarchical navigable small world graph
//! - **FTS**: full-text search with BM25 scoring
//! - **LSH**: locality-sensitive hashing for fuzzy text matching
#![expect(
    clippy::indexing_slicing,
    clippy::iter_not_returning_iterator,
    clippy::result_large_err,
    reason = "engine-internal search RA -- indexing on bind_idx validated during compilation"
)]

use std::collections::BTreeMap;
use std::fmt::Write;

use compact_str::CompactString;
use itertools::Itertools;

use super::{RelAlgebra, flatten_err};
use crate::data::expr::Bytecode;
use crate::data::program::{FtsSearch, HnswSearch, MagicSymbol};
use crate::data::symb::Symbol;
use crate::data::tuple::TupleIter;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;
use crate::query::error::*;
use crate::runtime::db::Poison;
use crate::runtime::minhash_lsh::LshSearch;
use crate::runtime::temp_store::EpochStore;
use crate::runtime::transact::SessionTx;

fn len_to_work_units(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// HNSW approximate nearest neighbor search operator.
///
/// For each parent tuple, extracts the query vector from the bound variable
/// and performs a k-NN search on the HNSW index. Returns (key, distance) pairs.
#[derive(Debug)]
pub(crate) struct HnswSearchRA {
    pub(crate) parent: Box<RelAlgebra>,
    pub(crate) hnsw_search: HnswSearch,
    pub(crate) filter_bytecode: Option<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) own_bindings: Vec<Symbol>,
}

impl HnswSearchRA {
    /// Compile filter expressions and fill binding indices.
    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        self.parent.fill_binding_indices_and_compile()?;
        if let Some(filter) = self.hnsw_search.filter.as_mut() {
            let bindings: BTreeMap<_, _> = self
                .own_bindings
                .iter()
                .cloned()
                .enumerate()
                .map(|(a, b)| (b, a))
                .collect();
            filter.fill_binding_indices(&bindings)?;
            self.filter_bytecode = Some((filter.compile()?, filter.span()));
        }
        Ok(())
    }
    /// Iterate over HNSW search results.
    ///
    /// # Complexity
    ///
    /// O(P * log N * ef) where P is parent tuples, N is index size, ef is beam width.
    /// Each parent tuple triggers one HNSW search.
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let bindings = self.parent.bindings_after_eliminate();
        let mut bind_idx = usize::MAX;
        for (i, b) in bindings.iter().enumerate() {
            if *b == self.hnsw_search.query {
                bind_idx = i;
                break;
            }
        }
        let config = self.hnsw_search.clone();
        let filter_code = self.filter_bytecode.clone();
        let mut stack = vec![];
        let it = self
            .parent
            .iter(tx, delta_rule, stores, poison.clone())?
            .map_ok(move |tuple| -> Result<_> {
                poison.account_work(1)?;
                // SAFETY: `bind_idx` is validated during compilation to be within `tuple` bounds.
                let v = match tuple[bind_idx].clone() {
                    DataValue::Vec(v) => v,
                    d => {
                        return Err(TypeSnafu {
                            expected: "vector",
                            got: format!("{d:?}"),
                            context: "HNSW search",
                        }
                        .build()
                        .into());
                    }
                };

                let res = tx.hnsw_knn(v, &config, &filter_code, &mut stack)?;
                poison.account_work(len_to_work_units(res.len()))?;
                Ok(res.into_iter().map(move |t| {
                    let mut r = tuple.clone();
                    r.extend(t);
                    r
                }))
            })
            .map(flatten_err)
            .flatten_ok();
        Ok(Box::new(it))
    }
}

/// Full-text search operator with BM25 scoring.
///
/// Tokenizes the query string, scores matching documents using BM25,
/// and returns results ranked by relevance.
#[derive(Debug)]
pub(crate) struct FtsSearchRA {
    pub(crate) parent: Box<RelAlgebra>,
    pub(crate) fts_search: FtsSearch,
    pub(crate) filter_bytecode: Option<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) own_bindings: Vec<Symbol>,
}

impl FtsSearchRA {
    /// Compile filter expressions and fill binding indices.
    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        self.parent.fill_binding_indices_and_compile()?;
        if let Some(filter) = self.fts_search.filter.as_mut() {
            let bindings: BTreeMap<_, _> = self
                .own_bindings
                .iter()
                .cloned()
                .enumerate()
                .map(|(a, b)| (b, a))
                .collect();
            filter.fill_binding_indices(&bindings)?;
            self.filter_bytecode = Some((filter.compile()?, filter.span()));
        }
        Ok(())
    }

    /// Iterate over full-text search results.
    ///
    /// # Complexity
    ///
    /// O(P * (T + D)) where P is parent tuples, T is tokenization cost,
    /// D is matching documents. BM25 scoring adds O(D log D) for ranking.
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let bindings = self.parent.bindings_after_eliminate();
        let mut bind_idx = usize::MAX;
        for (i, b) in bindings.iter().enumerate() {
            if *b == self.fts_search.query {
                bind_idx = i;
                break;
            }
        }
        let config = self.fts_search.clone();
        let filter_code = self.filter_bytecode.clone();
        let mut stack = vec![];
        let tokenizer = tx.tokenizers.get(
            &config.idx_handle.name,
            &config.manifest.tokenizer,
            &config.manifest.filters,
        )?;
        let it = self
            .parent
            .iter(tx, delta_rule, stores, poison.clone())?
            .map_ok(move |tuple| -> Result<_> {
                poison.account_work(1)?;
                // SAFETY: `bind_idx` is validated during compilation to be within `tuple` bounds.
                let q = match tuple[bind_idx].clone() {
                    DataValue::Str(s) => s,
                    DataValue::List(l) => {
                        let mut coll = CompactString::default();
                        for d in l {
                            match d {
                                DataValue::Str(s) => {
                                    if !coll.is_empty() {
                                        // INVARIANT: CompactString::write_str is infallible
                                        let _ = coll.write_str(" OR ");
                                    }
                                    // INVARIANT: CompactString::write_str is infallible
                                    let _ = coll.write_str(&s);
                                }
                                d => {
                                    return Err(TypeSnafu {
                                        expected: "string",
                                        got: format!("{d:?}"),
                                        context: "FTS search",
                                    }
                                    .build()
                                    .into());
                                }
                            }
                        }
                        coll
                    }
                    d => {
                        return Err(TypeSnafu {
                            expected: "string",
                            got: format!("{d:?}"),
                            context: "FTS search",
                        }
                        .build()
                        .into());
                    }
                };

                let res = tx.fts_search(&q, &config, &filter_code, &tokenizer, &mut stack)?;
                poison.account_work(len_to_work_units(res.len()))?;
                Ok(res.into_iter().map(move |t| {
                    let mut r = tuple.clone();
                    r.extend(t);
                    r
                }))
            })
            .map(flatten_err)
            .flatten_ok();
        Ok(Box::new(it))
    }
}

/// Locality-sensitive hashing search operator.
///
/// Uses `MinHash` signatures and banded LSH to find approximately similar
/// text documents. Similarity is based on token overlap (Jaccard-like).
#[derive(Debug)]
pub(crate) struct LshSearchRA {
    pub(crate) parent: Box<RelAlgebra>,
    pub(crate) lsh_search: LshSearch,
    pub(crate) filter_bytecode: Option<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) own_bindings: Vec<Symbol>,
}

impl LshSearchRA {
    /// Compile filter expressions and fill binding indices.
    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        self.parent.fill_binding_indices_and_compile()?;
        if let Some(filter) = self.lsh_search.filter.as_mut() {
            let bindings: BTreeMap<_, _> = self
                .own_bindings
                .iter()
                .cloned()
                .enumerate()
                .map(|(a, b)| (b, a))
                .collect();
            filter.fill_binding_indices(&bindings)?;
            self.filter_bytecode = Some((filter.compile()?, filter.span()));
        }
        Ok(())
    }

    /// Iterate over LSH (locality-sensitive hashing) search results.
    ///
    /// # Complexity
    ///
    /// O(P * b) where P is parent tuples and b is number of hash bands.
    /// Each band requires a prefix scan of the LSH index.
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let bindings = self.parent.bindings_after_eliminate();
        let mut bind_idx = usize::MAX;
        for (i, b) in bindings.iter().enumerate() {
            if *b == self.lsh_search.query {
                bind_idx = i;
                break;
            }
        }
        let config = self.lsh_search.clone();
        let filter_code = self.filter_bytecode.clone();
        let mut stack = vec![];
        let perms = config.manifest.get_hash_perms()?;
        let tokenizer = tx.tokenizers.get(
            &config.idx_handle.name,
            &config.manifest.tokenizer,
            &config.manifest.filters,
        )?;

        let it = self
            .parent
            .iter(tx, delta_rule, stores, poison.clone())?
            .map_ok(move |tuple| -> Result<_> {
                poison.account_work(1)?;
                let res = tx.lsh_search(
                    &tuple[bind_idx],
                    &config,
                    &mut stack,
                    &filter_code,
                    &perms,
                    &tokenizer,
                )?;
                poison.account_work(len_to_work_units(res.len()))?;
                Ok(res.into_iter().map(move |t| {
                    let mut r = tuple.clone();
                    r.extend(t);
                    r
                }))
            })
            .map(flatten_err)
            .flatten_ok();
        Ok(Box::new(it))
    }
}
