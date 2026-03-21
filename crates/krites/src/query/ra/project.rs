use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools;

use super::{RelAlgebra, eliminate_from_tuple, flatten_err, get_eliminate_indices};
use crate::data::expr::{Bytecode, Expr, eval_bytecode};
use crate::data::program::MagicSymbol;
use crate::data::symb::Symbol;
use crate::data::tuple::{Tuple, TupleIter};
use crate::error::InternalResult as Result;
use crate::parse::SourceSpan;
use crate::query::error::*;
use crate::runtime::temp_store::EpochStore;
use crate::runtime::transact::SessionTx;
pub(crate) struct UnificationRA {
    pub(crate) parent: Box<RelAlgebra>,
    pub(crate) binding: Symbol,
    pub(crate) expr: Expr,
    pub(crate) expr_bytecode: Vec<Bytecode>,
    pub(crate) is_multi: bool,
    pub(crate) to_eliminate: BTreeSet<Symbol>,
    pub(crate) span: SourceSpan,
}

impl UnificationRA {
    pub(crate) fn fill_binding_indices_and_compile(&mut self) -> Result<()> {
        let parent_bindings: BTreeMap<_, _> = self
            .parent
            .bindings_after_eliminate()
            .into_iter()
            .enumerate()
            .map(|(a, b)| (b, a))
            .collect();
        self.expr.fill_binding_indices(&parent_bindings)?;
        self.expr_bytecode = self.expr.compile()?;
        Ok(())
    }
    pub(crate) fn do_eliminate_temp_vars(&mut self, used: &BTreeSet<Symbol>) -> Result<()> {
        for binding in self.parent.bindings_before_eliminate() {
            if !used.contains(&binding) {
                self.to_eliminate.insert(binding.clone());
            }
        }
        let mut nxt = used.clone();
        nxt.extend(self.expr.bindings()?);
        self.parent.eliminate_temp_vars(&nxt)?;
        Ok(())
    }

    pub(crate) fn iter<'a>(
        &'a self,
        tx: &'a SessionTx<'_>,
        delta_rule: Option<&MagicSymbol>,
        stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    ) -> Result<TupleIter<'a>> {
        let mut bindings = self.parent.bindings_after_eliminate();
        bindings.push(self.binding.clone());
        let eliminate_indices = get_eliminate_indices(&bindings, &self.to_eliminate);
        let mut stack = vec![];
        Ok(if self.is_multi {
            let it = self
                .parent
                .iter(tx, delta_rule, stores)?
                .map_ok(move |tuple| -> Result<Vec<Tuple>> {
                    let result_list = eval_bytecode(&self.expr_bytecode, &tuple, &mut stack)?;
                    let result_list = result_list.get_slice().ok_or_else(|| {
                        EvalFailedSnafu {
                            message: "Invalid spread unification",
                        }
                        .build()
                    })?;
                    let mut coll = vec![];
                    for result in result_list {
                        let mut ret = tuple.clone();
                        ret.push(result.clone());
                        let ret = ret;
                        let ret = eliminate_from_tuple(ret, &eliminate_indices);
                        coll.push(ret);
                    }
                    Ok(coll)
                })
                .map(flatten_err)
                .flatten_ok();
            Box::new(it)
        } else {
            Box::new(
                self.parent
                    .iter(tx, delta_rule, stores)?
                    .map_ok(move |tuple| -> Result<Tuple> {
                        let result = eval_bytecode(&self.expr_bytecode, &tuple, &mut stack)?;
                        let mut ret = tuple;
                        ret.push(result);
                        let ret = ret;
                        let ret = eliminate_from_tuple(ret, &eliminate_indices);
                        Ok(ret)
                    })
                    .map(flatten_err),
            )
        })
    }
}
