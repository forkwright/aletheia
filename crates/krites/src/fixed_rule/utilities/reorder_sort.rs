//! Fixed rule that projects, sorts, ranks, and paginates an input relation.
//!
//! `out` is a list of expressions evaluated per input row to build the
//! projected columns; `sort_by` is an expression evaluated per row to
//! produce the sort key. Output rows are prefixed with a rank (dense rank
//! by default, or a running count when `break_ties` is set), then
//! `skip`/`take` apply pagination over the ranked, sorted rows.
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;

use crate::data::expr::{Expr, eval_bytecode};
use crate::data::functions::OP_LIST;
use crate::data::program::WrongFixedRuleOptionError;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::{ConfigSnafu, FixedRuleError};
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

pub(crate) struct ReorderSort;

/// Extract the projection list from the `out` option: either a literal
/// constant list, or an `OP_LIST` application over row-referencing exprs.
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn projection_exprs(payload: &FixedRulePayload<'_, '_>) -> Result<Vec<Expr>> {
    match payload.expr_option("out", None)? {
        Expr::Const {
            val: DataValue::List(items),
            span,
        } => Ok(items
            .iter()
            .map(|item| Expr::Const {
                val: item.clone(),
                span,
            })
            .collect()),
        Expr::Apply { op, args, .. } if *op == OP_LIST => Ok(args.to_vec()),
        _ => Err(WrongFixedRuleOptionError {
            name: "out".to_string(),
            span: payload.span(),
            rule_name: payload.name().to_string(),
            help: "This option must evaluate to a list".to_string(),
        }
        .into()),
    }
}

impl FixedRule for ReorderSort {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let source = payload.get_input(0)?;
        let mut projections = projection_exprs(&payload)?;
        let mut sort_key = payload.expr_option(
            "sort_by",
            Some(Expr::Const {
                val: DataValue::Null,
                span: SourceSpan(0, 0),
            }),
        )?;
        let descending = payload.bool_option("descending", Some(false))?;
        let break_ties = payload.bool_option("break_ties", Some(false))?;
        let skip = payload.non_neg_integer_option("skip", Some(0))?;
        let take = payload.non_neg_integer_option("take", Some(0))?;

        let binding = source.get_binding_map(0);
        sort_key.fill_binding_indices(&binding)?;
        for projection in &mut projections {
            projection.fill_binding_indices(&binding)?;
        }
        let projection_programs: Vec<_> = projections.iter().map(Expr::compile).try_collect()?;
        let sort_program = sort_key.compile()?;

        let mut eval_stack = vec![];
        let mut scored_rows: Vec<(DataValue, Vec<DataValue>)> = vec![];
        for row in source.iter()? {
            let row = row?;
            let key = eval_bytecode(&sort_program, &row, &mut eval_stack)?;
            let mut projected: Vec<DataValue> = Vec::with_capacity(projection_programs.len());
            for program in &projection_programs {
                projected.push(eval_bytecode(program, &row, &mut eval_stack)?);
            }
            scored_rows.push((key, projected));
            poison.check()?;
        }

        if descending {
            scored_rows.sort_by(|(a, _), (b, _)| b.cmp(a));
        } else {
            scored_rows.sort_by(|(a, _), (b, _)| a.cmp(b));
        }

        let mut rank = 0usize;
        let mut position = 0usize;
        let mut last_key: Option<&DataValue> = None;
        let take_upper_bound = take.saturating_add(skip);

        for (key, projected) in &scored_rows {
            position += 1;
            if last_key != Some(key) {
                rank = position;
                last_key = Some(key);
            }

            if take != 0 && position > take_upper_bound {
                break;
            }
            if position <= skip {
                continue;
            }

            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_wrap,
                reason = "row rank/position values fit i64 for the row counts this crate handles"
            )]
            let rank_value = DataValue::from(if break_ties { position } else { rank } as i64);
            let mut output_row = Vec::with_capacity(projected.len() + 1);
            output_row.push(rank_value);
            output_row.extend(projected.iter().cloned());
            out.put(output_row);
            poison.check()?;
        }

        Ok(())
    }

    fn arity(
        &self,
        options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        let out_option = options.get("out").ok_or_else(|| {
            ConfigSnafu {
                rule: "ReorderSort",
                param: "out",
                message: "option 'out' not provided",
            }
            .build()
        })?;
        match out_option {
            Expr::Const {
                val: DataValue::List(items),
                ..
            } => Ok(items.len() + 1),
            Expr::Apply { op, args, .. } if **op == OP_LIST => Ok(args.len() + 1),
            _ => Err(FixedRuleError::Config {
                rule: "ReorderSort".to_string(),
                param: "out".to_string(),
                message: "invalid option 'out' given, expect a list".to_string(),
                location: snafu::location!(),
            }
            .into()),
        }
    }
}
