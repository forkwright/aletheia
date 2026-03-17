use std::collections::{BTreeMap, BTreeSet};

use crate::engine::data::expr::{Bytecode, Expr, eval_bytecode_pred};
use crate::engine::data::program::MagicSymbol;
use crate::engine::data::symb::Symbol;
use crate::engine::data::tuple::TupleIter;
use crate::engine::error::InternalResult as Result;
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::temp_store::EpochStore;
use crate::engine::runtime::transact::SessionTx;

use super::{RelAlgebra, eliminate_from_tuple, get_eliminate_indices};

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
    ) -> Result<TupleIter<'a>> {
        let bindings = self.parent.bindings_after_eliminate();
        let eliminate_indices = get_eliminate_indices(&bindings, &self.to_eliminate);
        let mut stack = vec![];
        Ok(Box::new(
            self.parent
                .iter(tx, delta_rule, stores)?
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
