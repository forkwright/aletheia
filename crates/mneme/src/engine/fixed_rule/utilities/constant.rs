//! Constant-value fixed rule.
use std::collections::BTreeMap;

use crate::engine::error::DbResult as Result;
use crate::engine::fixed_rule::error::FixedRuleError;
use compact_str::CompactString;

use crate::engine::data::expr::Expr;
use crate::engine::data::program::WrongFixedRuleOptionError;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct Constant;

impl FixedRule for Constant {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        _poison: Poison,
    ) -> Result<()> {
        let data = payload.expr_option("data", None).unwrap();
        let data = data.get_const().unwrap().get_slice().unwrap();
        for row in data {
            let tuple = row.get_slice().unwrap().into();
            out.put(tuple)
        }
        Ok(())
    }

    fn arity(
        &self,
        options: &BTreeMap<CompactString, Expr>,
        rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        let data = options
            .get("data")
            .unwrap()
            .get_const()
            .unwrap()
            .get_slice()
            .unwrap();
        Ok(if data.is_empty() {
            match rule_head.len() {
                0 => {
                    return Err(Box::new(FixedRuleError::InvalidInput {
                        rule: "Constant".to_string(),
                        message: "Constant rule does not have data".to_string(),
                        location: snafu::location!(),
                    }));
                }
                i => i,
            }
        } else {
            data.first().unwrap().get_slice().unwrap().len()
        })
    }

    fn init_options(
        &self,
        options: &mut BTreeMap<CompactString, Expr>,
        _span: SourceSpan,
    ) -> Result<()> {
        let data = options
            .get("data")
            .ok_or_else(|| WrongFixedRuleOptionError {
                name: "data".to_string(),
                span: Default::default(),
                rule_name: "Constant".to_string(),
                help: "a list of lists is required".to_string(),
            })?;
        let data = match data.clone().eval_to_const()? {
            DataValue::List(l) => l,
            _ => return Err(Box::new(WrongFixedRuleOptionError {
                name: "data".to_string(),
                span: Default::default(),
                rule_name: "Constant".to_string(),
                help: "a list of lists is required".to_string(),
            })),
        };

        let mut tuples = vec![];
        let mut last_len = None;
        for row in data {
            match row {
                DataValue::List(tuple) => {
                    if let Some(l) = &last_len {
                        if *l != tuple.len() {
                            return Err(Box::new(FixedRuleError::InvalidInput {
                                rule: "Constant".to_string(),
                                message: "Constant head must have the same arity as the data given"
                                    .to_string(),
                                location: snafu::location!(),
                            }));
                        }
                    };
                    last_len = Some(tuple.len());
                    tuples.push(DataValue::List(tuple));
                }
                _row => {
                    return Err(Box::new(FixedRuleError::InvalidInput {
                        rule: "Constant".to_string(),
                        message: "Bad row for constant rule: {0:?}".to_string(),
                        location: snafu::location!(),
                    }));
                }
            }
        }

        options.insert(
            CompactString::from("data"),
            Expr::Const {
                val: DataValue::List(tuples),
                span: Default::default(),
            },
        );

        Ok(())
    }
}
