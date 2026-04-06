use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Formatter};
use std::iter;

use itertools::Itertools;
use tracing::debug;

use super::{RelAlgebra, eliminate_from_tuple, get_eliminate_indices, join_is_prefix};
use crate::data::program::MagicSymbol;
use crate::data::symb::Symbol;
use crate::data::tuple::{Tuple, TupleIter};
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;
use crate::query::error::*;
use crate::runtime::temp_store::EpochStore;
use crate::runtime::transact::SessionTx;
use crate::utils::swap_option_result;

struct BindingFormatter(Vec<Symbol>);

impl Debug for BindingFormatter {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = self.0.iter().map(|f| f.to_string()).join(", ");
        write!(f, "[{s}]")
    }
}

pub(crate) struct Joiner {
    // INVARIANT: these are of the same lengths
    pub(crate) left_keys: Vec<Symbol>,
    pub(crate) right_keys: Vec<Symbol>,
}

impl Debug for Joiner {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let left_bindings = BindingFormatter(self.left_keys.clone());
        let right_bindings = BindingFormatter(self.right_keys.clone());
        write!(f, "{left_bindings:?}<->{right_bindings:?}")
    }
}

impl Joiner {
    pub(crate) fn as_map(&self) -> BTreeMap<&str, &str> {
        self.left_keys
            .iter()
            .zip(self.right_keys.iter())
            .map(|(l, r)| (&l.name as &str, &r.name as &str))
            .collect()
    }
    pub(crate) fn join_indices(
        &self,
        left_bindings: &[Symbol],
        right_bindings: &[Symbol],
    ) -> Result<(Vec<usize>, Vec<usize>)> {
        let left_binding_map = left_bindings
            .iter()
            .enumerate()
            .map(|(k, v)| (v, k))
            .collect::<BTreeMap<_, _>>();
        let right_binding_map = right_bindings
            .iter()
            .enumerate()
            .map(|(k, v)| (v, k))
            .collect::<BTreeMap<_, _>>();
        let mut ret_l = Vec::with_capacity(self.left_keys.len());
        let mut ret_r = Vec::with_capacity(self.left_keys.len());
        for (l, r) in self.left_keys.iter().zip(self.right_keys.iter()) {
            let l_pos = left_binding_map.get(l).unwrap_or_else(|| unreachable!());
            let r_pos = right_binding_map.get(r).unwrap_or_else(|| unreachable!());
            ret_l.push(*l_pos);
            ret_r.push(*r_pos)
        }
        Ok((ret_l, ret_r))
    }
}

#[derive(Debug)]
pub(crate) struct NegJoin {
    pub(crate) left: RelAlgebra,
    pub(crate) right: RelAlgebra,
    pub(crate) joiner: Joiner,
    pub(crate) to_eliminate: BTreeSet<Symbol>,
    pub(crate) span: SourceSpan,
}

impl NegJoin {
    pub(crate) fn do_eliminate_temp_vars(&mut self, used: &BTreeSet<Symbol>) -> Result<()> {
        for binding in self.left.bindings_after_eliminate() {
            if !used.contains(&binding) {
                self.to_eliminate.insert(binding.clone());
            }
        }
        let mut left = used.clone();
        left.extend(self.joiner.left_keys.clone());
        self.left.eliminate_temp_vars(&left)?;
        Ok(())
    }

    pub(crate) fn join_type(&self) -> &str {
        match &self.right {
            RelAlgebra::TempStore(_) => {
                let join_indices = self
                    .joiner
                    .join_indices(
                        &self.left.bindings_after_eliminate(),
                        &self.right.bindings_after_eliminate(),
                    )
                    .unwrap_or_else(|_| unreachable!());
                if join_is_prefix(&join_indices.1) {
                    "mem_neg_prefix_join"
                } else {
                    "mem_neg_mat_join"
                }
            }
            RelAlgebra::Stored(_) => {
                let join_indices = self
                    .joiner
                    .join_indices(
                        &self.left.bindings_after_eliminate(),
                        &self.right.bindings_after_eliminate(),
                    )
                    .unwrap_or_else(|_| unreachable!());
                if join_is_prefix(&join_indices.1) {
                    "stored_neg_prefix_join"
                } else {
                    "stored_neg_mat_join"
                }
            }
            _ => {
                unreachable!()
            }
        }
    }

    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<TupleIter<'a>> {
        let bindings = self.left.bindings_after_eliminate();
        let eliminate_indices = get_eliminate_indices(&bindings, &self.to_eliminate);
        match &self.right {
            RelAlgebra::TempStore(r) => {
                let join_indices = self.joiner.join_indices(
                    &self.left.bindings_after_eliminate(),
                    &self.right.bindings_after_eliminate(),
                )?;
                r.neg_join(
                    self.left.iter(tx, delta_rule, stores)?,
                    join_indices,
                    eliminate_indices,
                    stores,
                )
            }
            RelAlgebra::Stored(v) => {
                let join_indices = self.joiner.join_indices(
                    &self.left.bindings_after_eliminate(),
                    &self.right.bindings_after_eliminate(),
                )?;
                v.neg_join(
                    tx,
                    self.left.iter(tx, delta_rule, stores)?,
                    join_indices,
                    eliminate_indices,
                )
            }
            _ => {
                unreachable!()
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct InnerJoin {
    pub(crate) left: RelAlgebra,
    pub(crate) right: RelAlgebra,
    pub(crate) joiner: Joiner,
    pub(crate) to_eliminate: BTreeSet<Symbol>,
    pub(crate) span: SourceSpan,
}

impl InnerJoin {
    pub(crate) fn do_eliminate_temp_vars(&mut self, used: &BTreeSet<Symbol>) -> Result<()> {
        for binding in self.bindings() {
            if !used.contains(&binding) {
                self.to_eliminate.insert(binding.clone());
            }
        }
        let mut left = used.clone();
        left.extend(self.joiner.left_keys.clone());
        if let Some(filters) = match &self.right {
            RelAlgebra::TempStore(r) => Some(&r.filters),
            _ => None,
        } {
            for filter in filters {
                left.extend(filter.bindings()?);
            }
        }
        self.left.eliminate_temp_vars(&left)?;
        let mut right = used.clone();
        right.extend(self.joiner.right_keys.clone());
        self.right.eliminate_temp_vars(&right)?;
        Ok(())
    }

    pub(crate) fn bindings(&self) -> Vec<Symbol> {
        let mut ret = self.left.bindings_after_eliminate();
        ret.extend(self.right.bindings_after_eliminate());
        debug_assert_eq!(ret.len(), ret.iter().collect::<BTreeSet<_>>().len());
        ret
    }
    pub(crate) fn join_type(&self) -> &str {
        match &self.right {
            RelAlgebra::Fixed(f) => f.join_type(),
            RelAlgebra::TempStore(_) => {
                let join_indices = self
                    .joiner
                    .join_indices(
                        &self.left.bindings_after_eliminate(),
                        &self.right.bindings_after_eliminate(),
                    )
                    .unwrap_or_else(|_| unreachable!());
                if join_is_prefix(&join_indices.1) {
                    "mem_prefix_join"
                } else {
                    "mem_mat_join"
                }
            }
            RelAlgebra::Stored(_) => {
                let join_indices = self
                    .joiner
                    .join_indices(
                        &self.left.bindings_after_eliminate(),
                        &self.right.bindings_after_eliminate(),
                    )
                    .unwrap_or_else(|_| unreachable!());
                if join_is_prefix(&join_indices.1) {
                    "stored_prefix_join"
                } else {
                    "stored_mat_join"
                }
            }
            RelAlgebra::HnswSearch(_) => "hnsw_search_join",
            RelAlgebra::FtsSearch(_) => "fts_search_join",
            RelAlgebra::LshSearch(_) => "lsh_search_join",
            RelAlgebra::StoredWithValidity(_) => {
                let join_indices = self
                    .joiner
                    .join_indices(
                        &self.left.bindings_after_eliminate(),
                        &self.right.bindings_after_eliminate(),
                    )
                    .unwrap_or_else(|_| unreachable!());
                if join_is_prefix(&join_indices.1) {
                    "stored_prefix_join"
                } else {
                    "stored_mat_join"
                }
            }
            RelAlgebra::Join(_) | RelAlgebra::Filter(_) | RelAlgebra::Unification(_) => {
                "generic_mat_join"
            }
            RelAlgebra::Reorder(_) => {
                unreachable!("query plan invariant: Reorder must be resolved before join")
            }
            RelAlgebra::NegJoin(_) => {
                unreachable!("query plan invariant: NegJoin cannot appear as join RHS")
            }
        }
    }
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<TupleIter<'a>> {
        let bindings = self.bindings();
        let eliminate_indices = get_eliminate_indices(&bindings, &self.to_eliminate);
        match &self.right {
            RelAlgebra::Fixed(f) => {
                let join_indices = self.joiner.join_indices(
                    &self.left.bindings_after_eliminate(),
                    &self.right.bindings_after_eliminate(),
                )?;
                f.join(
                    self.left.iter(tx, delta_rule, stores)?,
                    join_indices,
                    eliminate_indices,
                )
            }
            RelAlgebra::TempStore(r) => {
                let join_indices = self.joiner.join_indices(
                    &self.left.bindings_after_eliminate(),
                    &self.right.bindings_after_eliminate(),
                )?;
                if join_is_prefix(&join_indices.1) {
                    r.prefix_join(
                        self.left.iter(tx, delta_rule, stores)?,
                        join_indices,
                        eliminate_indices,
                        delta_rule,
                        stores,
                    )
                } else {
                    self.materialized_join(tx, eliminate_indices, delta_rule, stores)
                }
            }
            RelAlgebra::Stored(r) => {
                let join_indices = self.joiner.join_indices(
                    &self.left.bindings_after_eliminate(),
                    &self.right.bindings_after_eliminate(),
                )?;
                if join_is_prefix(&join_indices.1) {
                    r.prefix_join(
                        tx,
                        self.left.iter(tx, delta_rule, stores)?,
                        join_indices,
                        eliminate_indices,
                    )
                } else {
                    self.materialized_join(tx, eliminate_indices, delta_rule, stores)
                }
            }
            RelAlgebra::StoredWithValidity(r) => {
                let join_indices = self.joiner.join_indices(
                    &self.left.bindings_after_eliminate(),
                    &self.right.bindings_after_eliminate(),
                )?;
                if join_is_prefix(&join_indices.1) {
                    r.prefix_join(
                        tx,
                        self.left.iter(tx, delta_rule, stores)?,
                        join_indices,
                        eliminate_indices,
                    )
                } else {
                    self.materialized_join(tx, eliminate_indices, delta_rule, stores)
                }
            }
            RelAlgebra::Join(_)
            | RelAlgebra::Filter(_)
            | RelAlgebra::Unification(_)
            | RelAlgebra::HnswSearch(_)
            | RelAlgebra::FtsSearch(_)
            | RelAlgebra::LshSearch(_) => {
                self.materialized_join(tx, eliminate_indices, delta_rule, stores)
            }
            RelAlgebra::Reorder(_) => CompilationFailedSnafu {
                message: "joining on reordered algebra is not supported",
            }
            .fail()
            .map_err(|e| e.into()),
            RelAlgebra::NegJoin(_) => CompilationFailedSnafu {
                message: "joining on NegJoin is not supported",
            }
            .fail()
            .map_err(|e| e.into()),
        }
    }
    fn materialized_join<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        eliminate_indices: BTreeSet<usize>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<TupleIter<'a>> {
        debug!("using materialized join");
        let right_bindings = self.right.bindings_after_eliminate();
        let (left_join_indices, right_join_indices) = self
            .joiner
            .join_indices(&self.left.bindings_after_eliminate(), &right_bindings)?;

        let mut left_iter = self.left.iter(tx, delta_rule, stores)?;
        let left_cache = match left_iter.next() {
            None => return Ok(Box::new(iter::empty())),
            Some(Err(err)) => return Err(err),
            Some(Ok(data)) => data,
        };

        let right_join_indices_set = BTreeSet::from_iter(right_join_indices.iter().cloned());
        let mut right_store_indices = right_join_indices;
        for i in 0..right_bindings.len() {
            if !right_join_indices_set.contains(&i) {
                right_store_indices.push(i)
            }
        }

        let right_invert_indices = right_store_indices
            .iter()
            .enumerate()
            .sorted_by_key(|(_, b)| **b)
            .map(|(a, _)| a)
            .collect_vec();
        let cached_data = {
            let mut cache = BTreeSet::new();
            for item in self.right.iter(tx, delta_rule, stores)? {
                match item {
                    Ok(tuple) => {
                        let stored_tuple = right_store_indices
                            .iter()
                            .map(|i| tuple[*i].clone())
                            .collect_vec();
                        cache.insert(stored_tuple);
                    }
                    Err(e) => return Err(e),
                }
            }
            cache.into_iter().collect_vec()
        };

        let (prefix, right_idx) =
            build_mat_range_iter(&cached_data, &left_join_indices, &left_cache);

        let it = CachedMaterializedIterator {
            eliminate_indices,
            left: left_iter,
            left_cache,
            left_join_indices,
            materialized: cached_data,
            right_invert_indices,
            right_idx,
            prefix,
        };
        Ok(Box::new(it))
    }
}

pub(crate) struct CachedMaterializedIterator<'a> {
    materialized: Vec<Tuple>,
    eliminate_indices: BTreeSet<usize>,
    left_join_indices: Vec<usize>,
    right_invert_indices: Vec<usize>,
    right_idx: usize,
    prefix: Tuple,
    left: TupleIter<'a>,
    left_cache: Tuple,
}

impl<'a> CachedMaterializedIterator<'a> {
    fn advance_right(&mut self) -> Option<&Tuple> {
        if self.right_idx == self.materialized.len() {
            None
        } else {
            let ret = &self.materialized[self.right_idx];
            if ret.starts_with(&self.prefix) {
                self.right_idx += 1;
                Some(ret)
            } else {
                None
            }
        }
    }
    fn next_inner(&mut self) -> Result<Option<Tuple>> {
        loop {
            let right_nxt = self.advance_right();
            match right_nxt {
                Some(data) => {
                    let data = data.clone();
                    let mut ret = self.left_cache.clone();
                    for i in &self.right_invert_indices {
                        ret.push(data[*i].clone());
                    }
                    let tuple = eliminate_from_tuple(ret, &self.eliminate_indices);
                    return Ok(Some(tuple));
                }
                None => {
                    let next_left = self.left.next();
                    match next_left {
                        None => return Ok(None),
                        Some(l) => {
                            let left_tuple = l?;
                            let (prefix, idx) = build_mat_range_iter(
                                &self.materialized,
                                &self.left_join_indices,
                                &left_tuple,
                            );
                            self.left_cache = left_tuple;

                            self.right_idx = idx;
                            self.prefix = prefix;
                        }
                    }
                }
            }
        }
    }
}

fn build_mat_range_iter(
    mat: &[Tuple],
    left_join_indices: &[usize],
    left_tuple: &Tuple,
) -> (Tuple, usize) {
    let prefix = left_join_indices
        .iter()
        .map(|i| left_tuple[*i].clone())
        .collect_vec();
    let idx = match mat.binary_search(&prefix) {
        Ok(i) => i,
        Err(i) => i,
    };
    (prefix, idx)
}

impl<'a> Iterator for CachedMaterializedIterator<'a> {
    type Item = Result<Tuple>;

    fn next(&mut self) -> Option<Self::Item> {
        swap_option_result(self.next_inner())
    }
}
