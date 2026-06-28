//! Column reordering operator.
//!
//! Permutes columns of the parent relation to match the expected output
//! order. Inserted by the compiler when a rule head's variable order
//! differs from the relation's binding order.
#![expect(
    clippy::indexing_slicing,
    clippy::iter_not_returning_iterator,
    clippy::result_large_err,
    reason = "engine-internal reorder RA -- indexing validated by old_order_indices lookup"
)]

use std::collections::BTreeMap;

use itertools::Itertools;

use super::RelAlgebra;
use crate::data::program::MagicSymbol;
use crate::data::symb::Symbol;
use crate::data::tuple::TupleIter;
use crate::error::InternalResult as Result;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::EpochStore;
use crate::runtime::transact::SessionTx;

/// Column reordering (permutation) operator.
///
/// # Complexity
///
/// O(P * C) where P is parent tuples and C is column count.
#[derive(Debug)]
pub(crate) struct ReorderRA {
    pub(crate) relation: Box<RelAlgebra>,
    pub(crate) new_order: Vec<Symbol>,
}

impl ReorderRA {
    pub(crate) fn bindings(&self) -> Vec<Symbol> {
        self.new_order.clone()
    }
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let old_order = self.relation.bindings_after_eliminate();
        let old_order_indices: BTreeMap<_, _> = old_order
            .into_iter()
            .enumerate()
            .map(|(k, v)| (v, k))
            .collect();
        let reorder_indices = self
            .new_order
            .iter()
            .map(|k| {
                old_order_indices.get(k).copied().ok_or_else(|| {
                    crate::error::InternalError::from(
                        crate::query::error::CompilationFailedSnafu {
                            message: format!("reorder binding '{k}' not found in original order"),
                        }
                        .build(),
                    )
                })
            })
            .collect::<crate::error::InternalResult<Vec<_>>>()?
            .into_iter()
            .collect_vec();
        Ok(Box::new(
            self.relation
                .iter(tx, delta_rule, stores, poison)?
                .map_ok(move |tuple| {
                    let old = tuple;

                    reorder_indices
                        .iter()
                        .map(|i| old[*i].clone())
                        .collect_vec()
                }),
        ))
    }
}
