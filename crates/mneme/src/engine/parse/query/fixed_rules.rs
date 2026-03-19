//! Fixed rule parsing.
//! Datalog query parsing.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::engine::error::InternalResult as Result;
use crate::engine::parse::error::InvalidQuerySnafu;
use compact_str::CompactString;

use crate::engine::FixedRule;
use crate::engine::data::expr::Expr;
use crate::engine::data::functions::{MAX_VALIDITY_TS, str2vld};
use crate::engine::data::program::{
    FixedRuleApply, FixedRuleArg, InputInlineRulesOrFixed, InputProgram,
};
use crate::engine::data::symb::{PROG_ENTRY, Symbol};
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::fixed_rule::FixedRuleHandle;
use crate::engine::fixed_rule::utilities::constant::Constant;
use crate::engine::parse::expr::build_expr;
use crate::engine::parse::{ExtractSpan, Pair, Rule};

use super::atoms::parse_rule_head;

pub(crate) fn parse_fixed_rule(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<(Symbol, FixedRuleApply)> {
    let mut src = src.into_inner();
    let (out_symbol, head, aggr) = parse_rule_head(
        src.next().expect("pest guarantees fixed rule head"),
        param_pool,
    )?;

    for (a, _v) in aggr.iter().zip(head.iter()) {
        if a.is_some() {
            return Err(InvalidQuerySnafu {
                message: "fixed rule cannot be combined with aggregation".to_string(),
            }
            .build()
            .into());
        }
    }

    let mut seen_bindings = BTreeSet::new();
    let mut binding_gen_id = 0;

    let name_pair = src.next().expect("pest guarantees fixed rule name");
    let fixed_name = &name_pair.as_str();
    let args_list = src.next().expect("pest guarantees fixed rule args list");
    let args_list_span = args_list.extract_span();

    let (rule_args, mut options) = collect_fixed_rule_args(
        args_list,
        &mut seen_bindings,
        &mut binding_gen_id,
        param_pool,
        cur_vld,
    )?;

    let fixed = FixedRuleHandle::new(fixed_name, name_pair.extract_span());

    let fixed_impl = fixed_rules.get(&fixed.name as &str).ok_or_else(|| {
        InvalidQuerySnafu {
            message: format!("Fixed rule '{}' not found", fixed.name),
        }
        .build()
    })?;
    fixed_impl.init_options(&mut options, args_list_span)?;
    let arity = fixed_impl.arity(&options, &head, name_pair.extract_span())?;

    if !head.is_empty() && arity != head.len() {
        return Err(InvalidQuerySnafu {
            message: "Fixed rule head arity mismatch".to_string(),
        }
        .build()
        .into());
    }

    Ok((
        out_symbol,
        FixedRuleApply {
            fixed_handle: fixed,
            rule_args,
            options: Arc::new(options),
            head,
            arity,
            span: args_list_span,
            fixed_impl: fixed_impl.clone(),
        },
    ))
}

fn collect_fixed_rule_args<'i>(
    args_list: Pair<'i>,
    seen_bindings: &mut BTreeSet<&'i str>,
    binding_gen_id: &mut u32,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
) -> Result<(Vec<FixedRuleArg>, BTreeMap<CompactString, Expr>)> {
    let mut rule_args: Vec<FixedRuleArg> = vec![];
    let mut options: BTreeMap<CompactString, Expr> = Default::default();
    for nxt in args_list.into_inner() {
        match nxt.as_rule() {
            Rule::fixed_rel => {
                let inner = nxt
                    .into_inner()
                    .next()
                    .expect("pest guarantees fixed_rel inner");
                let arg = match inner.as_rule() {
                    Rule::fixed_rule_rel => {
                        parse_fixed_rule_rel_arg(inner, seen_bindings, binding_gen_id)?
                    }
                    Rule::fixed_relation_rel => parse_fixed_relation_rel_arg(
                        inner,
                        seen_bindings,
                        binding_gen_id,
                        param_pool,
                        cur_vld,
                    )?,
                    Rule::fixed_named_relation_rel => parse_fixed_named_relation_rel_arg(
                        inner,
                        seen_bindings,
                        param_pool,
                        cur_vld,
                    )?,
                    _ => unreachable!(),
                };
                rule_args.push(arg);
            }
            Rule::fixed_opt_pair => {
                let mut inner = nxt.into_inner();
                let name = inner.next().expect("pest guarantees option name").as_str();
                let val = inner.next().expect("pest guarantees option value");
                let val = build_expr(val, param_pool)?;
                options.insert(CompactString::from(name), val);
            }
            _ => unreachable!(),
        }
    }
    Ok((rule_args, options))
}

fn parse_fixed_rule_rel_arg<'i>(
    inner: Pair<'i>,
    seen_bindings: &mut BTreeSet<&'i str>,
    binding_gen_id: &mut u32,
) -> Result<FixedRuleArg> {
    let span = inner.extract_span();
    let mut els = inner.into_inner();
    let name = els.next().expect("pest guarantees fixed_rule_rel name");
    let mut bindings = Vec::with_capacity(els.size_hint().1.unwrap_or(4));
    for v in els {
        let s = v.as_str();
        if s == "_" {
            let symb = Symbol::new(format!("*_*{binding_gen_id}"), v.extract_span());
            *binding_gen_id += 1;
            bindings.push(symb);
        } else {
            if !seen_bindings.insert(s) {
                return Err(InvalidQuerySnafu {
                    message: "fixed rule cannot have duplicate bindings".to_string(),
                }
                .build()
                .into());
            }
            bindings.push(Symbol::new(s, v.extract_span()));
        }
    }
    Ok(FixedRuleArg::InMem {
        name: Symbol::new(name.as_str(), name.extract_span()),
        bindings,
        span,
    })
}

fn parse_fixed_relation_rel_arg<'i>(
    inner: Pair<'i>,
    seen_bindings: &mut BTreeSet<&'i str>,
    binding_gen_id: &mut u32,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
) -> Result<FixedRuleArg> {
    let span = inner.extract_span();
    let mut els = inner.into_inner();
    let name = els.next().expect("pest guarantees fixed_relation_rel name");
    let mut bindings = vec![];
    let mut valid_at = None;
    for v in els {
        match v.as_rule() {
            Rule::var => {
                let s = v.as_str();
                if s == "_" {
                    let symb = Symbol::new(format!("*_*{binding_gen_id}"), v.extract_span());
                    *binding_gen_id += 1;
                    bindings.push(symb);
                } else {
                    if !seen_bindings.insert(s) {
                        return Err(InvalidQuerySnafu {
                            message: "fixed rule cannot have duplicate bindings".to_string(),
                        }
                        .build()
                        .into());
                    }
                    bindings.push(Symbol::new(v.as_str(), v.extract_span()))
                }
            }
            Rule::validity_clause => {
                let vld_inner = v
                    .into_inner()
                    .next()
                    .expect("pest guarantees validity expr");
                let vld_expr = build_expr(vld_inner, param_pool)?;
                valid_at = Some(expr2vld_spec(vld_expr, cur_vld)?)
            }
            _ => unreachable!(),
        }
    }
    Ok(FixedRuleArg::Stored {
        name: Symbol::new(
            name.as_str()
                .strip_prefix('*')
                .expect("pest guarantees * prefix on stored relation"),
            name.extract_span(),
        ),
        bindings,
        valid_at,
        span,
    })
}

fn parse_fixed_named_relation_rel_arg<'i>(
    inner: Pair<'i>,
    seen_bindings: &mut BTreeSet<&'i str>,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
) -> Result<FixedRuleArg> {
    let span = inner.extract_span();
    let mut els = inner.into_inner();
    let name = els
        .next()
        .expect("pest guarantees fixed_named_relation_rel name");
    let mut bindings = BTreeMap::new();
    let mut valid_at = None;
    for p in els {
        match p.as_rule() {
            Rule::fixed_named_relation_arg_pair => {
                let mut vs = p.into_inner();
                let kp = vs.next().expect("pest guarantees named arg key");
                let k = CompactString::from(kp.as_str());
                let v = match vs.next() {
                    Some(vp) => {
                        if !seen_bindings.insert(vp.as_str()) {
                            return Err(InvalidQuerySnafu {
                                message: "fixed rule cannot have duplicate bindings".to_string(),
                            }
                            .build()
                            .into());
                        }
                        Symbol::new(vp.as_str(), vp.extract_span())
                    }
                    None => {
                        if !seen_bindings.insert(kp.as_str()) {
                            return Err(InvalidQuerySnafu {
                                message: "fixed rule cannot have duplicate bindings".to_string(),
                            }
                            .build()
                            .into());
                        }
                        Symbol::new(k.clone(), kp.extract_span())
                    }
                };
                bindings.insert(k, v);
            }
            Rule::validity_clause => {
                let vld_inner = p
                    .into_inner()
                    .next()
                    .expect("pest guarantees validity expr");
                let vld_expr = build_expr(vld_inner, param_pool)?;
                valid_at = Some(expr2vld_spec(vld_expr, cur_vld)?)
            }
            _ => unreachable!(),
        }
    }
    Ok(FixedRuleArg::NamedStored {
        name: Symbol::new(
            name.as_str()
                .strip_prefix(':')
                .expect("pest guarantees : prefix on named stored relation"),
            name.extract_span(),
        ),
        bindings,
        valid_at,
        span,
    })
}

pub(crate) fn make_empty_const_rule(prog: &mut InputProgram, bindings: &[Symbol]) {
    let entry_symbol = Symbol::new(PROG_ENTRY, Default::default());
    let mut options = BTreeMap::new();
    options.insert(
        CompactString::from("data"),
        Expr::Const {
            val: DataValue::List(vec![]),
            span: Default::default(),
        },
    );
    prog.prog.insert(
        entry_symbol,
        InputInlineRulesOrFixed::Fixed {
            fixed: FixedRuleApply {
                fixed_handle: FixedRuleHandle {
                    name: Symbol::new("Constant", Default::default()),
                },
                rule_args: vec![],
                options: Arc::new(options),
                head: bindings.to_vec(),
                arity: bindings.len(),
                span: Default::default(),
                fixed_impl: Arc::new(Box::new(Constant)),
            },
        },
    );
}

pub(crate) fn expr2vld_spec(expr: Expr, cur_vld: ValidityTs) -> Result<ValidityTs> {
    let _vld_span = expr.span();
    match expr.eval_to_const()? {
        DataValue::Num(n) => {
            let microseconds = n.get_int().ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "bad specification of validity".to_string(),
                }
                .build()
            })?;
            Ok(ValidityTs(Reverse(microseconds)))
        }
        DataValue::Str(s) => match &s as &str {
            "NOW" => Ok(cur_vld),
            "END" => Ok(MAX_VALIDITY_TS),
            s => Ok(str2vld(s).map_err(|e| {
                InvalidQuerySnafu {
                    message: format!("bad specification of validity: {e}"),
                }
                .build()
            })?),
        },
        _ => {
            return Err(InvalidQuerySnafu {
                message: "bad specification of validity".to_string(),
            }
            .build()
            .into());
        }
    }
}
