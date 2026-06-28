//! Persistent stored relation sources.
//!
//! These sources scan relations that live in the storage engine (fjall or mem).
//! Two variants:
//! - [`StoredRA`]: standard scan/prefix-join/neg-join on a stored relation.
//! - [`StoredWithValidityRA`]: time-travel scan using validity timestamps,
//!   enabling point-in-time queries on relations with a `Validity` key column.
#![expect(
    clippy::explicit_iter_loop,
    clippy::indexing_slicing,
    clippy::iter_not_returning_iterator,
    clippy::mutable_key_type,
    clippy::result_large_err,
    clippy::too_many_arguments,
    clippy::unnecessary_wraps,
    reason = "engine-internal stored RA -- indexing validated by join construction, mutable keys are Symbol"
)]

use std::collections::{BTreeMap, BTreeSet};

use either::{Left, Right};
use itertools::Itertools;

use super::{eliminate_from_tuple, filter_iter, flatten_err, invert_option_err, join_is_prefix};
use crate::data::expr::{Bytecode, Expr, compute_bounds, eval_bytecode_pred};
use crate::data::symb::Symbol;
use crate::data::tuple::{Tuple, TupleIter};
use crate::data::value::{DataValue, ValidityTs};
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::relation::RelationHandle;
use crate::runtime::transact::SessionTx;
use crate::utils::swap_option_result;

/// Persistent stored relation source (no time-travel).
///
/// Scans a [`RelationHandle`] from the storage engine. Supports:
/// - Full scan (`iter`)
/// - Prefix join (`prefix_join`) when join keys align with the storage key prefix
/// - Point lookup join when all keys are bound
/// - Anti-join (`neg_join`) for negated relation atoms
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
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let mut stack = vec![];

        let it = left_iter
            .map_ok(move |tuple| -> Result<Option<Tuple>> {
                let prefix = left_to_prefix_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
                    .collect_vec();
                let key = &prefix[0..key_len];
                poison.account_work(1)?;
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

    /// Prefix join on a stored relation.
    ///
    /// # Complexity
    ///
    /// - Point lookup (all keys bound): O(L * log N) where N is relation size.
    /// - Prefix scan: O(L * M) where M is matching rows per prefix.
    /// - With filter bounds: O(L * log N) via bounded range scan.
    pub(crate) fn prefix_join<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
        poison: Poison,
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
                poison,
            );
        }

        let range_bounds = if self.filters.is_empty() {
            None
        } else {
            let other_bindings =
                &self.bindings[right_join_indices.len()..self.storage.metadata.keys.len()];
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
                    return Left(
                        self.storage
                            .scan_bounded_prefix(tx, &prefix, l_bound, u_bound)
                            .map(move |res_found| -> Result<Option<Tuple>> {
                                poison_for_scan.account_work(1)?;
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
                skip_range_check = true;
                Right(
                    self.storage
                        .scan_prefix(tx, &prefix)
                        .map(move |res_found| -> Result<Option<Tuple>> {
                            poison_for_scan.account_work(1)?;
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

    /// Anti-join on a stored relation.
    ///
    /// # Complexity
    ///
    /// - Prefix: O(L * M) per left tuple scanning matching prefixes.
    /// - Materialized: O(N) to build hash set + O(L) to probe.
    pub(crate) fn neg_join<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
        poison: Poison,
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
                            poison.account_work(1)?;
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

                        Ok(Some(eliminate_from_tuple(tuple, &eliminate_indices)))
                    })
                    .map(flatten_err)
                    .filter_map(invert_option_err),
            ))
        } else {
            let mut right_join_vals = BTreeSet::new();

            for tuple in self.storage.scan_all(tx) {
                poison.account_work(1)?;
                let tuple = tuple?;
                // SAFETY: `right_join_indices` contains indices validated to be within `tuple` bounds.
                let to_join: Box<[DataValue]> = right_join_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
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

    /// Full scan over the stored relation.
    ///
    /// # Complexity
    ///
    /// O(N) where N is relation size. Filters add O(N * F).
    pub(crate) fn iter<'a>(&'a self, tx: &'a SessionTx<'_>) -> Result<TupleIter<'a>> {
        let it = self.storage.scan_all(tx);
        Ok(if self.filters.is_empty() {
            Box::new(it)
        } else {
            Box::new(filter_iter(self.filters_bytecodes.clone(), it))
        })
    }
}

/// Stored relation source with time-travel (validity-based scanning).
///
/// Like [`StoredRA`] but filters tuples by a validity timestamp,
/// enabling point-in-time queries. Requires the relation's last key
/// column to be of type `Validity`.
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

    /// Full scan with validity filtering.
    ///
    /// # Complexity
    ///
    /// O(N) where N is relation size (skip-scan over validity).
    pub(crate) fn iter<'a>(&'a self, tx: &'a SessionTx<'_>) -> Result<TupleIter<'a>> {
        let it = self.storage.skip_scan_all(tx, self.valid_at);
        Ok(if self.filters.is_empty() {
            Box::new(it)
        } else {
            Box::new(filter_iter(self.filters_bytecodes.clone(), it))
        })
    }

    /// Prefix join with validity filtering.
    ///
    /// # Complexity
    ///
    /// O(L * M) where L is left tuples, M is matching entries per prefix.
    pub(crate) fn prefix_join<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        left_iter: TupleIter<'a>,
        (left_join_indices, right_join_indices): (Vec<usize>, Vec<usize>),
        eliminate_indices: BTreeSet<usize>,
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let mut right_invert_indices = right_join_indices.iter().enumerate().collect_vec();
        right_invert_indices.sort_by_key(|(_, b)| **b);
        let left_to_prefix_indices = right_invert_indices
            .into_iter()
            .map(|(a, _)| left_join_indices[a])
            .collect_vec();

        let mut skip_range_check = false;
        let range_bounds = if self.filters.is_empty() {
            None
        } else {
            let other_bindings =
                &self.bindings[right_join_indices.len()..self.storage.metadata.keys.len()];
            Some(compute_bounds(&self.filters, other_bindings)?)
        };

        let it = left_iter
            .map_ok(move |tuple| {
                let poison_for_scan = poison.clone();
                let prefix = left_to_prefix_indices
                    .iter()
                    .map(|i| tuple[*i].clone())
                    .collect_vec();

                if let Some((l_bound, u_bound)) = &range_bounds
                    && !skip_range_check
                    && (!l_bound.iter().all(|v| *v == DataValue::Null)
                        || !u_bound.iter().all(|v| *v == DataValue::Bot))
                {
                    let mut stack = vec![];
                    return Left(
                        self.storage
                            .skip_scan_bounded_prefix(tx, &prefix, l_bound, u_bound, self.valid_at)
                            .map(move |res_found| -> Result<Option<Tuple>> {
                                poison_for_scan.account_work(1)?;
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
                skip_range_check = true;
                let mut stack = vec![];
                Right(
                    self.storage
                        .skip_scan_prefix(tx, &prefix, self.valid_at)
                        .map(move |res_found| -> Result<Option<Tuple>> {
                            poison_for_scan.account_work(1)?;
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
