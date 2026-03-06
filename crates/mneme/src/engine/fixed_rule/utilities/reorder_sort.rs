/*
 * Copyright 2022, The Cozo Project Authors.
 *
 * This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
 * If a copy of the MPL was not distributed with this file,
 * You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::collections::BTreeMap;

use crate::bail;
use crate::engine::error::DbResult as Result;
use itertools::Itertools;
use smartstring::{LazyCompact, SmartString};

use crate::engine::data::expr::{Expr, eval_bytecode};
use crate::engine::data::functions::OP_LIST;
use crate::engine::data::program::WrongFixedRuleOptionError;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct ReorderSort;

impl FixedRule for ReorderSort {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let in_rel = payload.get_input(0)?;

        let mut out_list = match payload.expr_option("out", None)? {
            Expr::Const {
                val: DataValue::List(l),
                span,
            } => l
                .iter()
                .map(|d| Expr::Const {
                    val: d.clone(),
                    span,
                })
                .collect_vec(),
            Expr::Apply { op, args, .. } if *op == OP_LIST => args.to_vec(),
            _ => {
                bail!(WrongFixedRuleOptionError {
                    name: "out".to_string(),
                    span: payload.span(),
                    rule_name: payload.name().to_string(),
                    help: "This option must evaluate to a list".to_string()
                })
            }
        };

        let mut sort_by = payload.expr_option(
            "sort_by",
            Some(Expr::Const {
                val: DataValue::Null,
                span: SourceSpan(0, 0),
            }),
        )?;
        let sort_descending = payload.bool_option("descending", Some(false))?;
        let break_ties = payload.bool_option("break_ties", Some(false))?;
        let skip = payload.non_neg_integer_option("skip", Some(0))?;
        let take = payload.non_neg_integer_option("take", Some(0))?;

        let binding_map = in_rel.get_binding_map(0);
        sort_by.fill_binding_indices(&binding_map)?;
        for out in out_list.iter_mut() {
            out.fill_binding_indices(&binding_map)?;
        }
        let out_bytecods: Vec<_> = out_list.iter().map(|e| e.compile()).try_collect()?;
        let sort_by_bytecodes = sort_by.compile()?;
        let mut stack = vec![];

        let mut buffer = vec![];
        for tuple in in_rel.iter()? {
            let tuple = tuple?;
            let sorter = eval_bytecode(&sort_by_bytecodes, &tuple, &mut stack)?;
            let mut s_tuple: Vec<_> = out_bytecods
                .iter()
                .map(|ex| eval_bytecode(ex, &tuple, &mut stack))
                .try_collect()?;
            s_tuple.push(sorter);
            buffer.push(s_tuple);
            poison.check()?;
        }
        if sort_descending {
            buffer.sort_by(|l, r| r.last().cmp(&l.last()));
        } else {
            buffer.sort_by(|l, r| l.last().cmp(&r.last()));
        }

        let mut count = 0usize;
        let mut rank = 0usize;
        let mut last = &DataValue::Bot;
        let take_plus_skip = take.saturating_add(skip);
        for val in &buffer {
            let sorter = val.last().unwrap();

            if sorter == last {
                count += 1;
            } else {
                count += 1;
                rank = count;
                last = sorter;
            }

            if take != 0 && count > take_plus_skip {
                break;
            }

            if count <= skip {
                continue;
            }
            let mut out_t = vec![DataValue::from(if break_ties { count } else { rank } as i64)];
            out_t.extend_from_slice(&val[0..val.len() - 1]);
            out.put(out_t);
            poison.check()?;
        }
        Ok(())
    }

    fn arity(
        &self,
        opts: &BTreeMap<SmartString<LazyCompact>, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        let out_opts = opts.get("out").ok_or_else(|| {
            crate::engine::error::AdhocError("ReorderSort: option 'out' not provided".to_string())
        })?;
        Ok(match out_opts {
            Expr::Const {
                val: DataValue::List(l),
                ..
            } => l.len() + 1,
            Expr::Apply { op, args, .. } if **op == OP_LIST => args.len() + 1,
            _ => bail!("ReorderSort: invalid option 'out' given, expect a list"),
        })
    }
}
