#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::{BTreeMap, BTreeSet};

use either::{Left, Right};
use itertools::Itertools;

use super::{eliminate_from_tuple, filter_iter, flatten_err, invert_option_err, join_is_prefix};
use crate::data::expr::{Bytecode, Expr, compute_bounds, eval_bytecode_pred};
use crate::data::program::MagicSymbol;
use crate::data::symb::Symbol;
use crate::data::tuple::{Tuple, TupleIter};
use crate::data::value::{DataValue, ValidityTs};
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;
use crate::runtime::relation::RelationHandle;
use crate::runtime::temp_store::EpochStore;
use crate::runtime::transact::SessionTx;
use crate::utils::swap_option_result;

#[derive(Debug)]
pub(crate) struct InlineFixedRA {
    pub(crate) bindings: Vec<Symbol>,
    pub(crate) data: Vec<Vec<DataValue>>,
    pub(crate) to_eliminate: BTreeSet<Symbol>,
    pub(crate) span: SourceSpan,
}

impl InlineFixedRA {
    pub(crate) fn unit(span: SourceSpan) -> Self {
        Self {
            bindings: vec![],
            data: vec![vec![]],
            to_eliminate: Default::default(),
            span,
        }
    }
    pub(crate) fn do_eliminate_temp_vars(&mut self, used: &BTreeSet<Symbol>) -> Result<()> {
        for binding in &self.bindings {
            if !used.contains(binding) {
                self.to_eliminate.insert(binding.clone());
            }
        }
        Ok(())
    }
}

impl InlineFixedRA {
    pub(crate) fn join_type(&self) -> &str {
        if self.data.is_empty() {
            "null_join"
        } else if self.data.len() == 1 {
            "singleton_join"
        } else {
            "fixed_join"
        }
    }
    pub(crate) fn join<'a>(
        &'a self,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
    ) -> Result<TupleIter<'a>> {
        use std::iter;
        Ok(if self.data.is_empty() {
            Box::new(iter::empty())
        } else if self.data.len() == 1 {
            let data = self.data[0].clone();
            let right_join_values = right_join_indices
                .into_iter()
                .map(|v| data[v].clone())
                .collect_vec();
            Box::new(left_iter.filter_map_ok(move |tuple| {
                let left_join_values = left_join_indices.iter().map(|v| &tuple[*v]).collect_vec();
                if left_join_values.into_iter().eq(right_join_values.iter()) {
                    let mut ret = tuple;
                    ret.extend_from_slice(&data);
                    let ret = ret;
                    let ret = eliminate_from_tuple(ret, &eliminate_indices);
                    Some(ret)
                } else {
                    None
                }
            }))
        } else {
            let mut right_mapping = BTreeMap::new();
            for data in &self.data {
                let right_join_values = right_join_indices.iter().map(|v| &data[*v]).collect_vec();
                match right_mapping.get_mut(&right_join_values) {
                    None => {
                        right_mapping.insert(right_join_values, vec![data]);
                    }
                    Some(coll) => {
                        coll.push(data);
                    }
                }
            }
            Box::new(
                left_iter
                    .filter_map_ok(move |tuple| {
                        let left_join_values =
                            left_join_indices.iter().map(|v| &tuple[*v]).collect_vec();
                        right_mapping.get(&left_join_values).map(|v| {
                            v.iter()
                                .map(|right_values| {
                                    let mut left_data = tuple.clone();
                                    left_data.extend_from_slice(right_values);
                                    left_data
                                })
                                .collect_vec()
                        })
                    })
                    .flatten_ok(),
            )
        })
    }
}

#[derive(Debug)]
pub(crate) struct StoredRA {
    pub(crate) bindings: Vec<Symbol>,
    pub(crate) storage: RelationHandle,
    pub(crate) filters: Vec<Expr>,
    pub(crate) filters_bytecodes: Vec<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) span: SourceSpan,
}

impl StoredRA {
    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        let bindings: BTreeMap<_, _> = self
            .bindings
            .iter()
            .cloned()
            .enumerate()
            .map(|(a, b)| (b, a))
            .collect();
        for e in self.filters.iter_mut() {
            e.fill_binding_indices(&bindings)?;
            self.filters_bytecodes.push((e.compile()?, e.span()));
        }
        Ok(())
    }

    fn point_lookup_join<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        left_iter: TupleIter<'a>,
        key_len: usize,
        left_to_prefix_indices: Vec<usize>,
        eliminate_indices: BTreeSet<usize>,
        left_join_indices: Vec<usize>,
        right_join_indices: Vec<usize>,
    ) -> Result<TupleIter<'a>> {
        let mut stack = vec![];

        let it = left_iter
            .map_ok(move |tuple| -> Result<Option<Tuple>> {
                let prefix = left_to_prefix_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
                    .collect_vec();
                let key = &prefix[0..key_len];
                match self.storage.get(tx, key)? {
                    None => Ok(None),
                    Some(found) => {
                        for (lk, rk) in left_join_indices.iter().zip(right_join_indices.iter()) {
                            if tuple[*lk] != found[*rk] {
                                return Ok(None);
                            }
                        }
                        for (p, span) in self.filters_bytecodes.iter() {
                            if !eval_bytecode_pred(p, &found, &mut stack, *span)? {
                                return Ok(None);
                            }
                        }
                        let mut ret = tuple;
                        ret.extend(found);
                        Ok(Some(ret))
                    }
                }
            })
            .flatten_ok()
            .filter_map(invert_option_err);
        Ok(if eliminate_indices.is_empty() {
            Box::new(it)
        } else {
            Box::new(it.map_ok(move |t| eliminate_from_tuple(t, &eliminate_indices)))
        })
    }

    pub(crate) fn prefix_join<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
    ) -> Result<TupleIter<'a>> {
        let mut right_invert_indices = right_join_indices.iter().enumerate().collect_vec();
        right_invert_indices.sort_by_key(|(_, b)| **b);
        let left_to_prefix_indices = right_invert_indices
            .into_iter()
            .map(|(a, _)| left_join_indices[a])
            .collect_vec();

        let key_len = self.storage.metadata.keys.len();
        if left_to_prefix_indices.len() >= key_len {
            return self.point_lookup_join(
                tx,
                left_iter,
                key_len,
                left_to_prefix_indices,
                eliminate_indices,
                left_join_indices,
                right_join_indices,
            );
        }

        let mut skip_range_check = false;
        let it = left_iter
            .map_ok(move |tuple| {
                let prefix = left_to_prefix_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
                    .collect_vec();
                let mut stack = vec![];

                if !skip_range_check && !self.filters.is_empty() {
                    let other_bindings =
                        &self.bindings[right_join_indices.len()..self.storage.metadata.keys.len()];
                    let (l_bound, u_bound) =
                        compute_bounds(&self.filters, other_bindings).unwrap_or_default();
                    if !l_bound.iter().all(|v| *v == DataValue::Null)
                        || !u_bound.iter().all(|v| *v == DataValue::Bot)
                    {
                        return Left(
                            self.storage
                                .scan_bounded_prefix(tx, &prefix, &l_bound, &u_bound)
                                .map(move |res_found| -> Result<Option<Tuple>> {
                                    let found = res_found?;
                                    for (p, span) in self.filters_bytecodes.iter() {
                                        if !eval_bytecode_pred(p, &found, &mut stack, *span)? {
                                            return Ok(None);
                                        }
                                    }
                                    let mut ret = tuple.clone();
                                    ret.extend(found);
                                    Ok(Some(ret))
                                })
                                .filter_map(swap_option_result),
                        );
                    }
                }
                skip_range_check = true;
                Right(
                    self.storage
                        .scan_prefix(tx, &prefix)
                        .map(move |res_found| -> Result<Option<Tuple>> {
                            let found = res_found?;
                            for (p, span) in self.filters_bytecodes.iter() {
                                if !eval_bytecode_pred(p, &found, &mut stack, *span)? {
                                    return Ok(None);
                                }
                            }
                            let mut ret = tuple.clone();
                            ret.extend(found);
                            Ok(Some(ret))
                        })
                        .filter_map(swap_option_result),
                )
            })
            .flatten_ok()
            .map(flatten_err);
        Ok(if eliminate_indices.is_empty() {
            Box::new(it)
        } else {
            Box::new(it.map_ok(move |t| eliminate_from_tuple(t, &eliminate_indices)))
        })
    }

    pub(crate) fn neg_join<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
    ) -> Result<TupleIter<'a>> {
        debug_assert!(!right_join_indices.is_empty());
        let mut right_invert_indices = right_join_indices.iter().enumerate().collect_vec();
        right_invert_indices.sort_by_key(|(_, b)| **b);
        let mut left_to_prefix_indices = vec![];
        for (ord, (idx, ord_sorted)) in right_invert_indices.iter().enumerate() {
            if ord != **ord_sorted {
                break;
            }
            left_to_prefix_indices.push(left_join_indices[*idx]);
        }

        if join_is_prefix(&right_join_indices) {
            Ok(Box::new(
                left_iter
                    .map_ok(move |tuple| -> Result<Option<Tuple>> {
                        let prefix = left_to_prefix_indices
                            .iter()
                            .map(|i| tuple[*i].clone())
                            .collect_vec();

                        'outer: for found in self.storage.scan_prefix(tx, &prefix) {
                            let found = found?;
                            for (left_idx, right_idx) in
                                left_join_indices.iter().zip(right_join_indices.iter())
                            {
                                if tuple[*left_idx] != found[*right_idx] {
                                    continue 'outer;
                                }
                            }
                            return Ok(None);
                        }

                        Ok(Some(if !eliminate_indices.is_empty() {
                            tuple
                                .into_iter()
                                .enumerate()
                                .filter_map(|(i, v)| {
                                    if eliminate_indices.contains(&i) {
                                        None
                                    } else {
                                        Some(v)
                                    }
                                })
                                .collect_vec()
                        } else {
                            tuple
                        }))
                    })
                    .map(flatten_err)
                    .filter_map(invert_option_err),
            ))
        } else {
            let mut right_join_vals = BTreeSet::new();

            for tuple in self.storage.scan_all(tx) {
                let tuple = tuple?;
                let to_join: Box<[DataValue]> = right_join_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
                    .collect();
                right_join_vals.insert(to_join);
            }
            Ok(Box::new(
                left_iter
                    .map_ok(move |tuple| -> Result<Option<Tuple>> {
                        let left_join_vals: Box<[DataValue]> = left_join_indices
                            .iter()
                            .map(|i| tuple[*i].clone())
                            .collect();
                        if right_join_vals.contains(&left_join_vals) {
                            return Ok(None);
                        }

                        Ok(Some(if !eliminate_indices.is_empty() {
                            tuple
                                .into_iter()
                                .enumerate()
                                .filter_map(|(i, v)| {
                                    if eliminate_indices.contains(&i) {
                                        None
                                    } else {
                                        Some(v)
                                    }
                                })
                                .collect_vec()
                        } else {
                            tuple
                        }))
                    })
                    .map(flatten_err)
                    .filter_map(invert_option_err),
            ))
        }
    }

    pub(crate) fn iter<'a>(&'a self, tx: &'a SessionTx<'_>) -> Result<TupleIter<'a>> {
        let it = self.storage.scan_all(tx);
        Ok(if self.filters.is_empty() {
            Box::new(it)
        } else {
            Box::new(filter_iter(self.filters_bytecodes.clone(), it))
        })
    }
}

#[derive(Debug)]
pub(crate) struct TempStoreRA {
    pub(crate) bindings: Vec<Symbol>,
    pub(crate) storage_key: MagicSymbol,
    pub(crate) filters: Vec<Expr>,
    pub(crate) filters_bytecodes: Vec<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) span: SourceSpan,
}

impl TempStoreRA {
    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        let bindings: BTreeMap<_, _> = self
            .bindings
            .iter()
            .cloned()
            .enumerate()
            .map(|(a, b)| (b, a))
            .collect();
        for e in self.filters.iter_mut() {
            e.fill_binding_indices(&bindings)?;
            self.filters_bytecodes.push((e.compile()?, e.span()))
        }
        Ok(())
    }

    pub(crate) fn iter<'a>(
        &'a self,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<TupleIter<'a>> {
        let storage = stores
            .get(&self.storage_key)
            .expect("TempStoreRA storage_key always present in stores: inserted by compiler");

        let scan_epoch = match delta_rule {
            None => false,
            Some(name) => *name == self.storage_key,
        };
        let it = if scan_epoch {
            Left(storage.delta_all_iter().map(|t| Ok(t.into_tuple())))
        } else {
            Right(storage.all_iter().map(|t| Ok(t.into_tuple())))
        };
        Ok(if self.filters.is_empty() {
            Box::new(it)
        } else {
            Box::new(filter_iter(self.filters_bytecodes.clone(), it))
        })
    }
    pub(crate) fn neg_join<'a>(
        &'a self,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<TupleIter<'a>> {
        let storage = stores
            .get(&self.storage_key)
            .expect("StoredRA storage_key always present in stores: inserted by compiler");
        debug_assert!(!right_join_indices.is_empty());
        let mut right_invert_indices = right_join_indices.iter().enumerate().collect_vec();
        right_invert_indices.sort_by_key(|(_, b)| **b);
        let mut left_to_prefix_indices = vec![];
        for (ord, (idx, ord_sorted)) in right_invert_indices.iter().enumerate() {
            if ord != **ord_sorted {
                break;
            }
            left_to_prefix_indices.push(left_join_indices[*idx]);
        }
        if join_is_prefix(&right_join_indices) {
            Ok(Box::new(
                left_iter
                    .map_ok(move |tuple| -> Result<Option<Tuple>> {
                        let prefix = left_to_prefix_indices
                            .iter()
                            .map(|i| tuple[*i].clone())
                            .collect_vec();

                        'outer: for found in storage.prefix_iter(&prefix) {
                            for (left_idx, right_idx) in
                                left_join_indices.iter().zip(right_join_indices.iter())
                            {
                                if tuple[*left_idx] != *found.get(*right_idx) {
                                    continue 'outer;
                                }
                            }
                            return Ok(None);
                        }

                        Ok(Some(if !eliminate_indices.is_empty() {
                            tuple
                                .into_iter()
                                .enumerate()
                                .filter_map(|(i, v)| {
                                    if eliminate_indices.contains(&i) {
                                        None
                                    } else {
                                        Some(v)
                                    }
                                })
                                .collect_vec()
                        } else {
                            tuple
                        }))
                    })
                    .map(flatten_err)
                    .filter_map(invert_option_err),
            ))
        } else {
            let mut right_join_vals = BTreeSet::new();
            for tuple in storage.all_iter() {
                let to_join: Box<[DataValue]> = right_join_indices
                    .iter()
                    .map(|i| tuple.get(*i).clone())
                    .collect();
                right_join_vals.insert(to_join);
            }

            Ok(Box::new(
                left_iter
                    .map_ok(move |tuple| -> Result<Option<Tuple>> {
                        let left_join_vals: Box<[DataValue]> = left_join_indices
                            .iter()
                            .map(|i| tuple[*i].clone())
                            .collect();
                        if right_join_vals.contains(&left_join_vals) {
                            return Ok(None);
                        }
                        Ok(Some(if !eliminate_indices.is_empty() {
                            tuple
                                .into_iter()
                                .enumerate()
                                .filter_map(|(i, v)| {
                                    if eliminate_indices.contains(&i) {
                                        None
                                    } else {
                                        Some(v)
                                    }
                                })
                                .collect_vec()
                        } else {
                            tuple
                        }))
                    })
                    .map(flatten_err)
                    .filter_map(invert_option_err),
            ))
        }
    }
    pub(crate) fn prefix_join<'a>(
        &'a self,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<TupleIter<'a>> {
        let storage = stores
            .get(&self.storage_key)
            .expect("StoredRA storage_key always present in stores: inserted by compiler");

        let mut right_invert_indices = right_join_indices.iter().enumerate().collect_vec();
        right_invert_indices.sort_by_key(|(_, b)| **b);
        let left_to_prefix_indices = right_invert_indices
            .into_iter()
            .map(|(a, _)| left_join_indices[a])
            .collect_vec();
        let scan_epoch = match delta_rule {
            None => false,
            Some(name) => *name == self.storage_key,
        };
        let mut skip_range_check = false;
        let it = left_iter
            .map_ok(move |tuple| {
                let prefix = left_to_prefix_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
                    .collect_vec();
                let mut stack = vec![];

                if !skip_range_check && !self.filters.is_empty() {
                    let other_bindings = &self.bindings[right_join_indices.len()..];
                    let (l_bound, u_bound) =
                        compute_bounds(&self.filters, other_bindings).unwrap_or_default();
                    if !l_bound.iter().all(|v| *v == DataValue::Null)
                        || !u_bound.iter().all(|v| *v == DataValue::Bot)
                    {
                        let mut lower_bound = prefix.clone();
                        lower_bound.extend(l_bound);
                        let mut upper_bound = prefix;
                        upper_bound.extend(u_bound);
                        let it = if scan_epoch {
                            Left(storage.delta_range_iter(&lower_bound, &upper_bound, true))
                        } else {
                            Right(storage.range_iter(&lower_bound, &upper_bound, true))
                        };
                        return Left(
                            it.map(move |res_found| -> Result<Option<Tuple>> {
                                if self.filters.is_empty() {
                                    let mut ret = tuple.clone();
                                    ret.extend(res_found.into_iter().cloned());
                                    Ok(Some(ret))
                                } else {
                                    let found = res_found.into_tuple();
                                    for (p, span) in self.filters_bytecodes.iter() {
                                        if !eval_bytecode_pred(p, &found, &mut stack, *span)? {
                                            return Ok(None);
                                        }
                                    }
                                    let mut ret = tuple.clone();
                                    ret.extend(found);
                                    Ok(Some(ret))
                                }
                            })
                            .filter_map(swap_option_result),
                        );
                    }
                }
                skip_range_check = true;

                let it = if scan_epoch {
                    Left(storage.delta_prefix_iter(&prefix))
                } else {
                    Right(storage.prefix_iter(&prefix))
                };

                Right(
                    it.map(move |res_found| -> Result<Option<Tuple>> {
                        if self.filters.is_empty() {
                            let mut ret = tuple.clone();
                            ret.extend(res_found.into_iter().cloned());
                            Ok(Some(ret))
                        } else {
                            let found = res_found.into_tuple();
                            for (p, span) in self.filters_bytecodes.iter() {
                                if !eval_bytecode_pred(p, &found, &mut stack, *span)? {
                                    return Ok(None);
                                }
                            }
                            let mut ret = tuple.clone();
                            ret.extend(found);
                            Ok(Some(ret))
                        }
                    })
                    .filter_map(swap_option_result),
                )
            })
            .flatten_ok()
            .map(flatten_err);
        Ok(if eliminate_indices.is_empty() {
            Box::new(it)
        } else {
            Box::new(it.map_ok(move |t| eliminate_from_tuple(t, &eliminate_indices)))
        })
    }
}

#[derive(Debug)]
pub(crate) struct StoredWithValidityRA {
    pub(crate) bindings: Vec<Symbol>,
    pub(crate) storage: RelationHandle,
    pub(crate) filters: Vec<Expr>,
    pub(crate) filters_bytecodes: Vec<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) valid_at: ValidityTs,
    pub(crate) span: SourceSpan,
}

impl StoredWithValidityRA {
    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        let bindings: BTreeMap<_, _> = self
            .bindings
            .iter()
            .cloned()
            .enumerate()
            .map(|(a, b)| (b, a))
            .collect();
        for e in self.filters.iter_mut() {
            e.fill_binding_indices(&bindings)?;
            self.filters_bytecodes.push((e.compile()?, e.span()));
        }
        Ok(())
    }
    pub(crate) fn iter<'a>(&'a self, tx: &'a SessionTx<'_>) -> Result<TupleIter<'a>> {
        let it = self.storage.skip_scan_all(tx, self.valid_at);
        Ok(if self.filters.is_empty() {
            Box::new(it)
        } else {
            Box::new(filter_iter(self.filters_bytecodes.clone(), it))
        })
    }
    pub(crate) fn prefix_join<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
    ) -> Result<TupleIter<'a>> {
        let mut right_invert_indices = right_join_indices.iter().enumerate().collect_vec();
        right_invert_indices.sort_by_key(|(_, b)| **b);
        let left_to_prefix_indices = right_invert_indices
            .into_iter()
            .map(|(a, _)| left_join_indices[a])
            .collect_vec();

        let mut skip_range_check = false;

        let it = left_iter
            .map_ok(move |tuple| {
                let prefix = left_to_prefix_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
                    .collect_vec();

                if !skip_range_check && !self.filters.is_empty() {
                    let other_bindings =
                        &self.bindings[right_join_indices.len()..self.storage.metadata.keys.len()];
                    let (l_bound, u_bound) =
                        compute_bounds(&self.filters, other_bindings).unwrap_or_default();
                    if !l_bound.iter().all(|v| *v == DataValue::Null)
                        || !u_bound.iter().all(|v| *v == DataValue::Bot)
                    {
                        let mut stack = vec![];
                        return Left(
                            self.storage
                                .skip_scan_bounded_prefix(
                                    tx,
                                    &prefix,
                                    &l_bound,
                                    &u_bound,
                                    self.valid_at,
                                )
                                .map(move |res_found| -> Result<Option<Tuple>> {
                                    let found = res_found?;
                                    for (p, span) in self.filters_bytecodes.iter() {
                                        if !eval_bytecode_pred(p, &found, &mut stack, *span)? {
                                            return Ok(None);
                                        }
                                    }
                                    let mut ret = tuple.clone();
                                    ret.extend(found);
                                    Ok(Some(ret))
                                })
                                .filter_map(swap_option_result),
                        );
                    }
                }
                skip_range_check = true;
                let mut stack = vec![];
                Right(
                    self.storage
                        .skip_scan_prefix(tx, &prefix, self.valid_at)
                        .map(move |res_found| -> Result<Option<Tuple>> {
                            let found = res_found?;
                            for (p, span) in self.filters_bytecodes.iter() {
                                if !eval_bytecode_pred(p, &found, &mut stack, *span)? {
                                    return Ok(None);
                                }
                            }
                            let mut ret = tuple.clone();
                            ret.extend(found);
                            Ok(Some(ret))
                        })
                        .filter_map(swap_option_result),
                )
            })
            .flatten_ok()
            .map(flatten_err);
        Ok(if eliminate_indices.is_empty() {
            Box::new(it)
        } else {
            Box::new(it.map_ok(move |t| eliminate_from_tuple(t, &eliminate_indices)))
        })
    }
}
