//! Data extraction helpers for stored relation operations.
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use compact_str::CompactString;
use itertools::Itertools;

use crate::engine::data::expr::Expr;
use crate::engine::data::program::{FixedRuleApply, InputInlineRulesOrFixed, InputProgram};
use crate::engine::data::relation::{ColumnDef, NullableColType};
use crate::engine::data::symb::Symbol;
use crate::engine::data::tuple::Tuple;
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::error::InternalResult as Result;
use crate::engine::fixed_rule::FixedRuleHandle;
use crate::engine::fixed_rule::utilities::constant::Constant;
use crate::engine::query::error::*;

pub(crate) enum DataExtractor {
    DefaultExtractor(Expr, NullableColType),
    IndexExtractor(usize, NullableColType),
}

impl DataExtractor {
    pub(crate) fn extract_data(&self, tuple: &Tuple, cur_vld: ValidityTs) -> Result<DataValue> {
        Ok(match self {
            DataExtractor::DefaultExtractor(expr, typ) => typ
                .coerce(expr.clone().eval_to_const()?, cur_vld)
                .map_err(|e| {
                    EvalFailedSnafu {
                        message: format!("{e}: when processing tuple {tuple:?}"),
                    }
                    .build()
                })?,
            DataExtractor::IndexExtractor(i, typ) => {
                typ.coerce(tuple[*i].clone(), cur_vld).map_err(|e| {
                    EvalFailedSnafu {
                        message: format!("{e}: when processing tuple {tuple:?}"),
                    }
                    .build()
                })?
            }
        })
    }
}

pub(crate) fn make_extractors(
    stored: &[ColumnDef],
    input: &[ColumnDef],
    bindings: &[Symbol],
    tuple_headers: &[Symbol],
) -> Result<Vec<DataExtractor>> {
    stored
        .iter()
        .map(|s| make_extractor(s, input, bindings, tuple_headers))
        .try_collect()
}

pub(crate) fn make_update_extractors(
    stored: &[ColumnDef],
    input: &[ColumnDef],
    bindings: &[Symbol],
    tuple_headers: &[Symbol],
) -> Result<Vec<Option<DataExtractor>>> {
    let input_keys: BTreeSet<_> = input.iter().map(|b| &b.name).collect();
    let mut extractors = Vec::with_capacity(stored.len());
    for col in stored.iter() {
        if input_keys.contains(&col.name) {
            extractors.push(Some(make_extractor(col, input, bindings, tuple_headers)?));
        } else {
            extractors.push(None);
        }
    }
    Ok(extractors)
}

pub(crate) fn make_extractor(
    stored: &ColumnDef,
    input: &[ColumnDef],
    bindings: &[Symbol],
    tuple_headers: &[Symbol],
) -> Result<DataExtractor> {
    for (inp_col, inp_binding) in input.iter().zip(bindings.iter()) {
        if inp_col.name == stored.name {
            for (idx, tuple_head) in tuple_headers.iter().enumerate() {
                if tuple_head == inp_binding {
                    return Ok(DataExtractor::IndexExtractor(idx, stored.typing.clone()));
                }
            }
        }
    }
    if let Some(expr) = &stored.default_gen {
        Ok(DataExtractor::DefaultExtractor(
            expr.clone(),
            stored.typing.clone(),
        ))
    } else {
        Err(StoredRelationSnafu {
            message: "cannot make extractor for column",
        }
        .build()
        .into())
    }
}

pub(crate) fn make_const_rule(
    program: &mut InputProgram,
    rule_name: &str,
    bindings: Vec<Symbol>,
    data: Vec<DataValue>,
) {
    let rule_symbol = Symbol::new(CompactString::from(rule_name), Default::default());
    let mut options = BTreeMap::new();
    options.insert(
        CompactString::from("data"),
        Expr::Const {
            val: DataValue::List(data),
            span: Default::default(),
        },
    );
    let bindings_arity = bindings.len();
    program.prog.insert(
        rule_symbol,
        InputInlineRulesOrFixed::Fixed {
            fixed: FixedRuleApply {
                fixed_handle: FixedRuleHandle {
                    name: Symbol::new("Constant", Default::default()),
                },
                rule_args: vec![],
                options: Arc::new(options),
                head: bindings,
                arity: bindings_arity,
                span: Default::default(),
                fixed_impl: Arc::new(Box::new(Constant)),
            },
        },
    );
}
