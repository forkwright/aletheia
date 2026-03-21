#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeMap;

use itertools::Itertools;

use super::RelAlgebra;
use crate::data::program::MagicSymbol;
use crate::data::symb::Symbol;
use crate::data::tuple::TupleIter;
use crate::error::InternalResult as Result;
use crate::runtime::temp_store::EpochStore;
use crate::runtime::transact::SessionTx;

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
                *old_order_indices
                    .get(k)
                    .expect("program logic error: reorder indices mismatch")
            })
            .collect_vec();
        Ok(Box::new(
            self.relation
                .iter(tx, delta_rule, stores)?
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
