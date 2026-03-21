#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
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
use crate::runtime::minhash_lsh::LshSearch;
use crate::runtime::temp_store::EpochStore;
use crate::runtime::transact::SessionTx;

#[derive(Debug)]
pub(crate) struct HnswSearchRA {
    pub(crate) parent: Box<RelAlgebra>,
    pub(crate) hnsw_search: HnswSearch,
    pub(crate) filter_bytecode: Option<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) own_bindings: Vec<Symbol>,
}

impl HnswSearchRA {
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
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
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
            .iter(tx, delta_rule, stores)?
            .map_ok(move |tuple| -> Result<_> {
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

#[derive(Debug)]
pub(crate) struct FtsSearchRA {
    pub(crate) parent: Box<RelAlgebra>,
    pub(crate) fts_search: FtsSearch,
    pub(crate) filter_bytecode: Option<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) own_bindings: Vec<Symbol>,
}

impl FtsSearchRA {
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
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
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
        let mut idf_cache = Default::default();
        let tokenizer = tx.tokenizers.get(
            &config.idx_handle.name,
            &config.manifest.tokenizer,
            &config.manifest.filters,
        )?;
        let it = self
            .parent
            .iter(tx, delta_rule, stores)?
            .map_ok(move |tuple| -> Result<_> {
                let q = match tuple[bind_idx].clone() {
                    DataValue::Str(s) => s,
                    DataValue::List(l) => {
                        let mut coll = CompactString::default();
                        for d in l {
                            match d {
                                DataValue::Str(s) => {
                                    if !coll.is_empty() {
                                        coll.write_str(" OR ")
                                            .expect("write to CompactString is infallible");
                                    }
                                    coll.write_str(&s)
                                        .expect("write to CompactString is infallible");
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

                let res = tx.fts_search(
                    &q,
                    &config,
                    &filter_code,
                    &tokenizer,
                    &mut stack,
                    &mut idf_cache,
                )?;
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

#[derive(Debug)]
pub(crate) struct LshSearchRA {
    pub(crate) parent: Box<RelAlgebra>,
    pub(crate) lsh_search: LshSearch,
    pub(crate) filter_bytecode: Option<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) own_bindings: Vec<Symbol>,
}

impl LshSearchRA {
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
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
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
            .iter(tx, delta_rule, stores)?
            .map_ok(move |tuple| -> Result<_> {
                let res = tx.lsh_search(
                    &tuple[bind_idx],
                    &config,
                    &mut stack,
                    &filter_code,
                    &perms,
                    &tokenizer,
                )?;
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
