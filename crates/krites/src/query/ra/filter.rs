//! Predicate filter operator.
//!
//! Applies compiled bytecode predicates to each tuple from the parent,
//! passing through only those that satisfy all filters.
#![expect(
    clippy::explicit_iter_loop,
    clippy::iter_not_returning_iterator,
    clippy::result_large_err,
    reason = "engine-internal filter RA -- iter returns TupleIter (boxed trait), not Self::Iterator"
)]

use std::collections::{BTreeMap, BTreeSet};

use crate::data::expr::{Bytecode, Expr, eval_bytecode_pred};
use crate::data::program::MagicSymbol;
use crate::data::symb::Symbol;
use crate::data::tuple::TupleIter;
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::EpochStore;
use crate::runtime::transact::SessionTx;

use super::{RelAlgebra, eliminate_from_tuple, get_eliminate_indices};

/// Filter operator: applies predicate expressions to parent tuples.
///
/// Filters are compiled to bytecode at plan compilation time and evaluated
/// per-tuple at iteration time. Multiple filters are AND-combined.
///
/// # Complexity
///
/// O(P * F) where P is parent tuples and F is filter count.
pub(crate) struct FilteredRA {
    pub(crate) parent: Box<RelAlgebra>,
    pub(crate) filters: Vec<Expr>,
    pub(crate) filters_bytecodes: Vec<(Vec<Bytecode>, SourceSpan)>,
    pub(crate) to_eliminate: BTreeSet<Symbol>,
    pub(crate) span: SourceSpan,
}

impl FilteredRA {
    pub(crate) fn do_eliminate_temp_vars(&mut self, used: &BTreeSet<Symbol>) -> Result<()> {
        for binding in self.parent.bindings_before_eliminate() {
            if !used.contains(&binding) {
                self.to_eliminate.insert(binding.clone());
            }
        }
        let mut nxt = used.clone();
        for e in self.filters.iter() {
            nxt.extend(e.bindings()?);
        }
        self.parent.eliminate_temp_vars(&nxt)?;
        Ok(())
    }

    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        let parent_bindings: BTreeMap<_, _> = self
            .parent
            .bindings_after_eliminate()
            .into_iter()
            .enumerate()
            .map(|(a, b)| (b, a))
            .collect();
        for e in self.filters.iter_mut() {
            e.fill_binding_indices(&parent_bindings)?;
            self.filters_bytecodes.push((e.compile()?, e.span()));
        }
        Ok(())
    }
    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
        poison: Poison,
    ) -> Result<TupleIter<'a>> {
        let bindings = self.parent.bindings_after_eliminate();
        let eliminate_indices = get_eliminate_indices(&bindings, &self.to_eliminate);
        let mut stack = vec![];
        Ok(Box::new(
            self.parent
                .iter(tx, delta_rule, stores, poison)?
                .filter_map(move |tuple| match tuple {
                    Ok(t) => {
                        for (p, span) in self.filters_bytecodes.iter() {
                            match eval_bytecode_pred(p, &t, &mut stack, *span) {
                                Ok(false) => return None,
                                Err(e) => return Some(Err(e)),
                                // NOTE: filter passed, continue to next
                                Ok(true) => {}
                            }
                        }
                        let t = eliminate_from_tuple(t, &eliminate_indices);
                        Some(Ok(t))
                    }
                    Err(e) => Some(Err(e)),
                }),
        ))
    }
}
