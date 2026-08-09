//! Fixed rule that materialises a literal list of rows as its output.
//!
//! The `data` option must evaluate to a constant list of equal-arity rows;
//! `init_options` normalises it once (folding it back into a `Const`
//! expression) so `run` and `arity` can both read it cheaply.
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::program::WrongFixedRuleOptionError;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::FixedRuleError;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

pub(crate) struct Constant;

fn invalid(message: impl Into<String>) -> crate::error::InternalError {
    FixedRuleError::InvalidInput {
        rule: "Constant".to_string(),
        message: message.into(),
        location: snafu::location!(),
    }
    .into()
}

impl FixedRule for Constant {
    fn init_options(
        &self,
        options: &mut BTreeMap<CompactString, Expr>,
        _span: SourceSpan,
    ) -> Result<()> {
        let raw = options
            .get("data")
            .ok_or_else(|| WrongFixedRuleOptionError {
                name: "data".to_string(),
                span: SourceSpan::default(),
                rule_name: "Constant".to_string(),
                help: "a list of lists is required".to_string(),
            })?
            .clone();

        let DataValue::List(rows) = raw.eval_to_const()? else {
            return Err(WrongFixedRuleOptionError {
                name: "data".to_string(),
                span: SourceSpan::default(),
                rule_name: "Constant".to_string(),
                help: "a list of lists is required".to_string(),
            }
            .into());
        };

        let mut expected_arity: Option<usize> = None;
        let mut normalized = Vec::with_capacity(rows.len());
        for row in rows {
            let DataValue::List(row_values) = row else {
                return Err(invalid("every row in 'data' must itself be a list"));
            };
            if let Some(arity) = expected_arity {
                if arity != row_values.len() {
                    return Err(invalid(
                        "every row in 'data' must have the same arity as the rule head",
                    ));
                }
            } else {
                expected_arity = Some(row_values.len());
            }
            normalized.push(DataValue::List(row_values));
        }

        options.insert(
            CompactString::from("data"),
            Expr::Const {
                val: DataValue::List(normalized),
                span: SourceSpan::default(),
            },
        );
        Ok(())
    }

    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        _poison: Poison,
    ) -> Result<()> {
        let data = payload.expr_option("data", None)?;
        let Some(DataValue::List(rows)) = data.get_const().cloned() else {
            return Err(invalid("'data' option is not a constant list"));
        };
        for row in rows {
            let DataValue::List(values) = row else {
                return Err(invalid("row in 'data' is not a list"));
            };
            out.put(values);
        }
        Ok(())
    }

    fn arity(
        &self,
        options: &BTreeMap<CompactString, Expr>,
        rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        let data_expr = options
            .get("data")
            .ok_or_else(|| invalid("'data' option missing"))?;
        let Some(DataValue::List(rows)) = data_expr.get_const() else {
            return Err(invalid("'data' option is not a constant in arity check"));
        };
        match rows.first() {
            Some(DataValue::List(first_row)) => Ok(first_row.len()),
            Some(_) => Err(invalid("first row in 'data' is not a list")),
            None if rule_head.is_empty() => {
                Err(invalid("Constant rule has no data and no rule head"))
            }
            None => Ok(rule_head.len()),
        }
    }
}
