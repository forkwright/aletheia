//! Temporary (derived) relation source.
//!
//! Represents intermediate relations computed during semi-naive evaluation.
//! These are the "delta" relations that drive the fixpoint iteration: each
//! epoch, new facts derived by rules are stored here and used as input for
//! the next epoch.
#![expect(
    clippy::explicit_iter_loop,
    clippy::indexing_slicing,
    clippy::iter_not_returning_iterator,
    clippy::mutable_key_type,
    clippy::result_large_err,
    clippy::semicolon_if_nothing_returned,
    reason = "engine-internal temp store RA -- indexing validated by join construction, mutable keys are Symbol"
)]

use std::collections::{BTreeMap, BTreeSet};

use either::{Left, Right};
use itertools::Itertools;

use super::{eliminate_from_tuple, filter_iter, flatten_err, invert_option_err, join_is_prefix};
use crate::data::expr::{Bytecode, Expr, compute_bounds, eval_bytecode_pred};
use crate::data::program::MagicSymbol;
use crate::data::symb::Symbol;
use crate::data::tuple::{Tuple, TupleIter};
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::EpochStore;
use crate::utils::swap_option_result;

/// Derived (temporary) relation source.
///
/// Points to an [`EpochStore`] by its [`MagicSymbol`] key.
/// During semi-naive evaluation, can scan either the full store
/// or just the delta (new facts from the previous epoch).
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

    /// Resolve the backing epoch store, returning an error if not found.
    fn resolve_store<'a>(
        &self,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<&'a EpochStore> {
        stores.get(&self.storage_key).ok_or_else(|| {
            crate::error::InternalError::from(
                crate::query::error::EvalFailedSnafu {
                    message: format!(
                        "temp store '{}' not found in epoch stores",
                        self.storage_key
                    ),
                }
                .build(),
            )
        })
    }

    /// Iterate over all (or delta) tuples in this derived relation.
    ///
    /// # Complexity
    ///
    /// O(T) where T is tuples in the store (or delta).
    /// Filter predicates add O(T * F) where F is filter count.
    pub(crate) fn iter<'a>(
        &'a self,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
        _poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let storage = self.resolve_store(stores)?;

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

    /// Anti-join: filter left tuples that have NO match in this temp store.
    ///
    /// # Complexity
    ///
    /// - Prefix join: O(L * log T) where L is left tuples, T is store size.
    /// - Materialized: O(T) to build hash set + O(L) to probe.
    pub(crate) fn neg_join<'a>(
        &'a self,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let storage = self.resolve_store(stores)?;
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
                            poison.account_work(1)?;
                            for (left_idx, right_idx) in
                                left_join_indices.iter().zip(right_join_indices.iter())
                            {
                                if tuple[*left_idx] != *found.get(*right_idx) {
                                    continue 'outer;
                                }
                            }
                            return Ok(None);
                        }

                        Ok(Some(eliminate_from_tuple(tuple, &eliminate_indices)))
                    })
                    .map(flatten_err)
                    .filter_map(invert_option_err),
            ))
        } else {
            let mut right_join_vals = BTreeSet::new();
            for tuple in storage.all_iter() {
                poison.account_work(1)?;
                let to_join: Box<[DataValue]> = right_join_indices
                    .iter()
                    .map(|i| tuple.get(*i).clone())
                    .collect();
                right_join_vals.insert(to_join);
            }

            Ok(Box::new(
                left_iter
                    .map_ok(move |tuple| -> Result<Option<Tuple>> {
                        // SAFETY: `left_join_indices` contains indices validated to be within `tuple` bounds.
                        let left_join_vals: Box<[DataValue]> = left_join_indices
                            .iter()
                            .map(|i| tuple[*i].clone())
                            .collect();
                        if right_join_vals.contains(&left_join_vals) {
                            return Ok(None);
                        }
                        Ok(Some(eliminate_from_tuple(tuple, &eliminate_indices)))
                    })
                    .map(flatten_err)
                    .filter_map(invert_option_err),
            ))
        }
    }

    /// Prefix join: scan temp store by prefix derived from left tuple join keys.
    ///
    /// # Complexity
    ///
    /// O(L * M) where L is left tuples, M is matching prefix entries per left tuple.
    /// With range bounds: O(L * log T) per left tuple via bounded range scan.
    pub(crate) fn prefix_join<'a>(
        &'a self,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let storage = self.resolve_store(stores)?;

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
        let range_bounds = if self.filters.is_empty() {
            None
        } else {
            let other_bindings = &self.bindings[right_join_indices.len()..];
            Some(compute_bounds(&self.filters, other_bindings)?)
        };
        let mut skip_range_check = false;
        let it = left_iter
            .map_ok(move |tuple| {
                let poison_for_scan = poison.clone();
                let prefix = left_to_prefix_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
                    .collect_vec();
                let mut stack = vec![];

                if let Some((l_bound, u_bound)) = &range_bounds
                    && !skip_range_check
                    && (!l_bound.iter().all(|v| *v == DataValue::Null)
                        || !u_bound.iter().all(|v| *v == DataValue::Bot))
                {
                    let mut lower_bound = prefix.clone();
                    lower_bound.extend(l_bound.iter().cloned());
                    let mut upper_bound = prefix;
                    upper_bound.extend(u_bound.iter().cloned());
                    let it = if scan_epoch {
                        Left(storage.delta_range_iter(&lower_bound, &upper_bound, true))
                    } else {
                        Right(storage.range_iter(&lower_bound, &upper_bound, true))
                    };
                    return Left(
                        it.map(move |res_found| -> Result<Option<Tuple>> {
                            poison_for_scan.account_work(1)?;
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
                skip_range_check = true;

                let it = if scan_epoch {
                    Left(storage.delta_prefix_iter(&prefix))
                } else {
                    Right(storage.prefix_iter(&prefix))
                };

                Right(
                    it.map(move |res_found| -> Result<Option<Tuple>> {
                        poison_for_scan.account_work(1)?;
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
