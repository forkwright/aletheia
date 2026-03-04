/*
 * Copyright 2022, The Cozo Project Authors.
 *
 * This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
 * If a copy of the MPL was not distributed with this file,
 * You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::cmp::Reverse;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use crate::engine::error::DbResult as Result;
use crate::{bail, ensure};
use either::{Left, Right};
use itertools::Itertools;
use pest::Parser;
use smartstring::{LazyCompact, SmartString};

use crate::engine::data::aggr::{parse_aggr, Aggregation};
use crate::engine::data::expr::Expr;
use crate::engine::data::functions::{str2vld, MAX_VALIDITY_TS};
use crate::engine::data::program::{
    FixedRuleApply, FixedRuleArg, InputAtom, InputInlineRule, InputInlineRulesOrFixed,
    InputNamedFieldRelationApplyAtom, InputProgram, InputRelationApplyAtom, InputRuleApplyAtom,
    QueryAssertion, QueryOutOptions, RelationOp, ReturnMutation, SearchInput, SortDir, Unification,
};
use crate::engine::data::relation::{ColType, ColumnDef, NullableColType, StoredRelationMetadata};
use crate::engine::data::symb::{Symbol, PROG_ENTRY};
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::fixed_rule::utilities::constant::Constant;
use crate::engine::fixed_rule::FixedRuleHandle;
use crate::engine::parse::expr::build_expr;
use crate::engine::parse::schema::parse_schema;
use crate::engine::parse::{CozoScriptParser, ExtractSpan, Pair, Pairs, Rule, SourceSpan};
use crate::engine::runtime::relation::InputRelationHandle;
use crate::engine::FixedRule;









#[derive(Debug)]
struct MultipleRuleDefinitionError(String, Vec<SourceSpan>);





impl Error for MultipleRuleDefinitionError {}

impl Display for MultipleRuleDefinitionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "The rule '{0}' cannot have multiple definitions since it contains non-Horn clauses",
            self.0
        )
    }
}


fn merge_spans(symbs: &[Symbol]) -> SourceSpan {
    let mut fst = symbs.first().unwrap().span;
    for nxt in symbs.iter().skip(1) {
        fst = fst.merge(nxt.span);
    }
    fst
}

pub(crate) fn parse_query(
    src: Pairs<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<InputProgram> {
    let mut progs: BTreeMap<Symbol, InputInlineRulesOrFixed> = Default::default();
    let mut out_opts: QueryOutOptions = Default::default();
    let mut disable_magic_rewrite = false;

    let mut stored_relation = None;
    let mut returning_mutation = ReturnMutation::NotReturning;

    for pair in src {
        match pair.as_rule() {
            Rule::rule => {
                let (name, rule) = parse_rule(pair, param_pool, cur_vld)?;

                match progs.entry(name) {
                    Entry::Vacant(e) => {
                        e.insert(InputInlineRulesOrFixed::Rules { rules: vec![rule] });
                    }
                    Entry::Occupied(mut e) => {
                        let key = e.key().to_string();
                        match e.get_mut() {
                            InputInlineRulesOrFixed::Rules { rules: rs } => {
                                
                                let prev = rs.first().unwrap();
                                ensure!(prev.aggr == rule.aggr,
                                    "Rule {} has multiple definitions with conflicting heads", key
                                );

                                rs.push(rule);
                            }
                            InputInlineRulesOrFixed::Fixed { fixed } => {
                                let _fixed_span = fixed.span;
                                bail!("The rule '{}' cannot have multiple definitions since it contains non-Horn clauses", e.key().name)
                            }
                        }
                    }
                }
            }
            Rule::fixed_rule => {
                let rule_span = pair.extract_span();
                let (name, apply) = parse_fixed_rule(pair, param_pool, fixed_rules, cur_vld)?;

                match progs.entry(name) {
                    Entry::Vacant(e) => {
                        e.insert(InputInlineRulesOrFixed::Fixed { fixed: apply });
                    }
                    Entry::Occupied(e) => {
                        let found_name = e.key().name.to_string();
                        let mut found_span = match e.get() {
                            InputInlineRulesOrFixed::Rules { rules } => {
                                rules.iter().map(|r| r.span).collect_vec()
                            }
                            InputInlineRulesOrFixed::Fixed { fixed } => vec![fixed.span],
                        };
                        found_span.push(rule_span);
                        bail!(MultipleRuleDefinitionError(found_name, found_span));
                    }
                }
            }
            Rule::const_rule => {
                let span = pair.extract_span();
                let mut src = pair.into_inner();
                let (name, mut head, aggr) = parse_rule_head(src.next().unwrap(), param_pool)?;

                if let Some(found) = progs.get(&name) {
                    let mut found_span = match found {
                        InputInlineRulesOrFixed::Rules { rules } => {
                            rules.iter().map(|r| r.span).collect_vec()
                        }
                        InputInlineRulesOrFixed::Fixed { fixed } => {
                            vec![fixed.span]
                        }
                    };
                    found_span.push(span);
                    bail!("The rule '{}' cannot have multiple definitions since it contains non-Horn clauses", name.name);
                }

                

                for (a, _v) in aggr.iter().zip(head.iter()) {
                    ensure!(a.is_none(), "Constant rules cannot have aggregation application");
                }
                let data_part = src.next().unwrap();
                let data_part_str = data_part.as_str();
                let data = build_expr(data_part.clone(), param_pool)?;
                let mut options = BTreeMap::new();
                options.insert(SmartString::from("data"), data);
                let handle = FixedRuleHandle {
                    name: Symbol::new("Constant", span),
                };
                let fixed_impl = Box::new(Constant);
                fixed_impl.init_options(&mut options, span)?;
                let arity = fixed_impl.arity(&options, &head, span)?;

                ensure!(arity != 0, "Encountered empty row for constant rule");
                ensure!(
                    head.is_empty() || arity == head.len(),
                    "Fixed rule head arity mismatch"
                );
                if head.is_empty() && name.is_prog_entry() {
                    if let Ok(mut datalist) =
                        CozoScriptParser::parse(Rule::param_list, data_part_str)
                    {
                        for s in datalist.next().unwrap().into_inner() {
                            if s.as_rule() == Rule::param {
                                head.push(Symbol::new(
                                    s.as_str().strip_prefix('$').unwrap(),
                                    Default::default(),
                                ));
                            }
                        }
                    }
                }
                progs.insert(
                    name,
                    InputInlineRulesOrFixed::Fixed {
                        fixed: FixedRuleApply {
                            fixed_handle: handle,
                            rule_args: vec![],
                            options: Arc::new(options),
                            head,
                            arity,
                            span,
                            fixed_impl: Arc::new(fixed_impl),
                        },
                    },
                );
            }
            Rule::timeout_option => {
                let pair = pair.into_inner().next().unwrap();
                let _span = pair.extract_span();
                let timeout = build_expr(pair, param_pool)?
                    .eval_to_const()
                    .map_err(|_err| crate::engine::error::AdhocError("Query option {} is not constant".to_string()))?
                    .get_float()
                    .ok_or(crate::engine::error::AdhocError("Query option {} requires a non-negative integer".to_string()))?;
                if timeout > 0. {
                    out_opts.timeout = Some(timeout);
                } else {
                    out_opts.timeout = None;
                }
            }
            Rule::sleep_option => {
                #[cfg(target_arch = "wasm32")]
                bail!(":sleep is not supported under WASM");

                #[cfg(not(target_arch = "wasm32"))]
                {
                    let pair = pair.into_inner().next().unwrap();
                    let _span = pair.extract_span();
                    let sleep = build_expr(pair, param_pool)?
                        .eval_to_const()
                        .map_err(|_err| crate::engine::error::AdhocError("Query option {} is not constant".to_string()))?
                        .get_float()
                        .ok_or(crate::engine::error::AdhocError("Query option {} requires a non-negative integer".to_string()))?;
                    ensure!(sleep > 0., crate::engine::error::AdhocError("Query option {} requires a positive integer".to_string()));
                    out_opts.sleep = Some(sleep);
                }
            }
            Rule::limit_option => {
                let pair = pair.into_inner().next().unwrap();
                let _span = pair.extract_span();
                let limit = build_expr(pair, param_pool)?
                    .eval_to_const()
                    .map_err(|_err| crate::engine::error::AdhocError("Query option {} is not constant".to_string()))?
                    .get_non_neg_int()
                    .ok_or(crate::engine::error::AdhocError("Query option {} requires a non-negative integer".to_string()))?;
                out_opts.limit = Some(limit as usize);
            }
            Rule::offset_option => {
                let pair = pair.into_inner().next().unwrap();
                let _span = pair.extract_span();
                let offset = build_expr(pair, param_pool)?
                    .eval_to_const()
                    .map_err(|_err| crate::engine::error::AdhocError("Query option {} is not constant".to_string()))?
                    .get_non_neg_int()
                    .ok_or(crate::engine::error::AdhocError("Query option {} requires a non-negative integer".to_string()))?;
                out_opts.offset = Some(offset as usize);
            }
            Rule::sort_option => {
                for part in pair.into_inner() {
                    let mut var = "";
                    let mut dir = SortDir::Asc;
                    let mut span = part.extract_span();
                    for a in part.into_inner() {
                        match a.as_rule() {
                            Rule::out_arg => {
                                var = a.as_str();
                                span = a.extract_span();
                            }
                            Rule::sort_asc => dir = SortDir::Asc,
                            Rule::sort_desc => dir = SortDir::Dsc,
                            _ => unreachable!(),
                        }
                    }
                    out_opts.sorters.push((Symbol::new(var, span), dir));
                }
            }
            Rule::returning_option => {
                returning_mutation = ReturnMutation::Returning;
            }
            Rule::relation_option => {
                let span = pair.extract_span();
                let mut args = pair.into_inner();
                let op = match args.next().unwrap().as_rule() {
                    Rule::relation_create => RelationOp::Create,
                    Rule::relation_replace => RelationOp::Replace,
                    Rule::relation_put => RelationOp::Put,
                    Rule::relation_insert => RelationOp::Insert,
                    Rule::relation_update => RelationOp::Update,
                    Rule::relation_rm => RelationOp::Rm,
                    Rule::relation_delete => RelationOp::Delete,
                    Rule::relation_ensure => RelationOp::Ensure,
                    Rule::relation_ensure_not => RelationOp::EnsureNot,
                    _ => unreachable!(),
                };

                let name_p = args.next().unwrap();
                let name = Symbol::new(name_p.as_str(), name_p.extract_span());
                match args.next() {
                    None => stored_relation = Some(Left((name, span, op))),
                    Some(schema_p) => {
                        let (mut metadata, mut key_bindings, mut dep_bindings) =
                            parse_schema(schema_p)?;
                        if !matches!(op, RelationOp::Create | RelationOp::Replace) {
                            key_bindings.extend(dep_bindings);
                            dep_bindings = vec![];
                            metadata.keys.extend(metadata.non_keys);
                            metadata.non_keys = vec![];
                        }
                        stored_relation = Some(Right((
                            InputRelationHandle {
                                name,
                                metadata,
                                key_bindings,
                                dep_bindings,
                                span,
                            },
                            op,
                        )))
                    }
                }
            }
            Rule::assert_none_option => {
                ensure!(
                    out_opts.assertion.is_none(),
                    "Multiple query output assertions defined"
                );
                out_opts.assertion = Some(QueryAssertion::AssertNone(pair.extract_span()))
            }
            Rule::assert_some_option => {
                ensure!(
                    out_opts.assertion.is_none(),
                    "Multiple query output assertions defined"
                );
                out_opts.assertion = Some(QueryAssertion::AssertSome(pair.extract_span()))
            }
            Rule::disable_magic_rewrite_option => {
                let pair = pair.into_inner().next().unwrap();
                let _span = pair.extract_span();
                let val = build_expr(pair, param_pool)?
                    .eval_to_const()
                    .map_err(|_err| crate::engine::error::AdhocError("Query option is not constant".to_string()))?
                    .get_bool()
                    .ok_or(crate::engine::error::AdhocError("Query option requires a boolean".to_string()))?;
                disable_magic_rewrite = val;
            }
            Rule::EOI => break,
            r => unreachable!("{:?}", r),
        }
    }

    let mut prog = InputProgram {
        prog: progs,
        out_opts,
        disable_magic_rewrite,
    };

    if prog.prog.is_empty() {
        if let Some((
            InputRelationHandle {
                key_bindings,
                dep_bindings,
                ..
            },
            RelationOp::Create,
            _,
        )) = &prog.out_opts.store_relation
        {
            let mut bindings = key_bindings.clone();
            bindings.extend_from_slice(dep_bindings);
            make_empty_const_rule(&mut prog, &bindings);
        }
    }

    // let head_arity = prog.get_entry_arity()?;

    match stored_relation {
        None => {}
        Some(Left((name, span, op))) => {
            let head = prog.get_entry_out_head()?;
            for symb in &head {
                symb.ensure_valid_field()?;
            }

            let metadata = StoredRelationMetadata {
                keys: head
                    .iter()
                    .map(|s| ColumnDef {
                        name: s.name.clone(),
                        typing: NullableColType {
                            coltype: ColType::Any,
                            nullable: true,
                        },
                        default_gen: None,
                    })
                    .collect(),
                non_keys: vec![],
            };

            let handle = InputRelationHandle {
                name,
                metadata,
                key_bindings: head,
                dep_bindings: vec![],
                span,
            };
            prog.out_opts.store_relation = Some((handle, op, returning_mutation))
        }
        Some(Right((h, o))) => prog.out_opts.store_relation = Some((h, o, returning_mutation)),
    }

    if prog.prog.is_empty() {
        if let Some((handle, RelationOp::Create, _)) = &prog.out_opts.store_relation {
            let mut bindings = handle.dep_bindings.clone();
            bindings.extend_from_slice(&handle.key_bindings);
            make_empty_const_rule(&mut prog, &bindings);
        }
    }

    if !prog.out_opts.sorters.is_empty() {
        

        let head_args = prog.get_entry_out_head()?;

        for (sorter, _) in &prog.out_opts.sorters {
            ensure!(
                head_args.contains(sorter),
                "Sort key not found"
            );
        }
    }

    

    let empty_mutation_head = match &prog.out_opts.store_relation {
        None => false,
        Some((handle, _, _)) => {
            if handle.key_bindings.is_empty() {
                if handle.dep_bindings.is_empty() {
                    true
                } else {
                    bail!("Input relation has no keys");
                }
            } else {
                false
            }
        }
    };

    if empty_mutation_head {
        let head_args = prog.get_entry_out_head()?;
        if let Some((handle, _, _)) = &mut prog.out_opts.store_relation {
            if head_args.is_empty() {
                bail!("Input relation has no keys");
            }
            handle.key_bindings = head_args.clone();
            handle.metadata.keys = head_args
                .iter()
                .map(|s| ColumnDef {
                    name: s.name.clone(),
                    typing: NullableColType {
                        coltype: ColType::Any,
                        nullable: true,
                    },
                    default_gen: None,
                })
                .collect();
        } else {
            unreachable!()
        }
    }

    Ok(prog)
}

fn parse_rule(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
) -> Result<(Symbol, InputInlineRule)> {
    let span = src.extract_span();
    let mut src = src.into_inner();
    let head = src.next().unwrap();
    let _head_span = head.extract_span();
    let (name, head, aggr) = parse_rule_head(head, param_pool)?;

    

    ensure!(!head.is_empty(), crate::engine::error::AdhocError("Horn-clause rule cannot have empty rule head".to_string()));
    let body = src.next().unwrap();
    let mut body_clauses = vec![];
    let mut ignored_counter = 0;
    for atom_src in body.into_inner() {
        body_clauses.push(parse_disjunction(
            atom_src,
            param_pool,
            cur_vld,
            &mut ignored_counter,
        )?)
    }

    Ok((
        name,
        InputInlineRule {
            head,
            aggr,
            body: body_clauses,
            span,
        },
    ))
}

fn parse_disjunction(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
    ignored_counter: &mut u32,
) -> Result<InputAtom> {
    let span = pair.extract_span();
    let res: Vec<_> = pair
        .into_inner()
        .filter_map(|v| match v.as_rule() {
            Rule::or_op => None,
            _ => Some(parse_atom(v, param_pool, cur_vld, ignored_counter)),
        })
        .try_collect()?;
    Ok(if res.len() == 1 {
        res.into_iter().next().unwrap()
    } else {
        InputAtom::Disjunction { inner: res, span }
    })
}

fn parse_atom(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
    ignored_counter: &mut u32,
) -> Result<InputAtom> {
    Ok(match src.as_rule() {
        Rule::rule_body => {
            let span = src.extract_span();
            let grouped: Vec<_> = src
                .into_inner()
                .map(|v| parse_disjunction(v, param_pool, cur_vld, ignored_counter))
                .try_collect()?;
            InputAtom::Conjunction {
                inner: grouped,
                span,
            }
        }
        Rule::disjunction => parse_disjunction(src, param_pool, cur_vld, ignored_counter)?,
        Rule::negation => {
            let span = src.extract_span();
            let mut src = src.into_inner();
            src.next().unwrap();
            let inner = parse_atom(src.next().unwrap(), param_pool, cur_vld, ignored_counter)?;
            InputAtom::Negation {
                inner: inner.into(),
                span,
            }
        }
        Rule::expr => {
            let expr = build_expr(src, param_pool)?;
            InputAtom::Predicate { inner: expr }
        }
        Rule::unify => {
            let span = src.extract_span();
            let mut src = src.into_inner();
            let var = src.next().unwrap();
            let mut symb = Symbol::new(var.as_str(), var.extract_span());
            if symb.is_ignored_symbol() {
                symb.name = format!("*^*{}", *ignored_counter).into();
                *ignored_counter += 1;
            }
            let expr = build_expr(src.next().unwrap(), param_pool)?;
            InputAtom::Unification {
                inner: Unification {
                    binding: symb,
                    expr,
                    one_many_unif: false,
                    span,
                },
            }
        }
        Rule::unify_multi => {
            let span = src.extract_span();
            let mut src = src.into_inner();
            let var = src.next().unwrap();
            let mut symb = Symbol::new(var.as_str(), var.extract_span());
            if symb.is_ignored_symbol() {
                symb.name = format!("*^*{}", *ignored_counter).into();
                *ignored_counter += 1;
            }
            src.next().unwrap();
            let expr = build_expr(src.next().unwrap(), param_pool)?;
            InputAtom::Unification {
                inner: Unification {
                    binding: symb,
                    expr,
                    one_many_unif: true,
                    span,
                },
            }
        }
        Rule::rule_apply => {
            let span = src.extract_span();
            let mut src = src.into_inner();
            let name = src.next().unwrap();
            let args: Vec<_> = src
                .next()
                .unwrap()
                .into_inner()
                .map(|v| build_expr(v, param_pool))
                .try_collect()?;
            InputAtom::Rule {
                inner: InputRuleApplyAtom {
                    name: Symbol::new(name.as_str(), name.extract_span()),
                    args,
                    span,
                },
            }
        }
        Rule::relation_apply => {
            let span = src.extract_span();
            let mut src = src.into_inner();
            let name = src.next().unwrap();
            let args: Vec<_> = src
                .next()
                .unwrap()
                .into_inner()
                .map(|v| build_expr(v, param_pool))
                .try_collect()?;
            let valid_at = match src.next() {
                None => None,
                Some(vld_clause) => {
                    let vld_expr = build_expr(vld_clause.into_inner().next().unwrap(), param_pool)?;
                    Some(expr2vld_spec(vld_expr, cur_vld)?)
                }
            };
            InputAtom::Relation {
                inner: InputRelationApplyAtom {
                    name: Symbol::new(&name.as_str()[1..], name.extract_span()),
                    args,
                    valid_at,
                    span,
                },
            }
        }
        Rule::search_apply => {
            let span = src.extract_span();
            let mut src = src.into_inner();
            let name_p = src.next().unwrap();
            let name_segs = name_p.as_str().split(':').collect_vec();

            

            ensure!(
                name_segs.len() == 2,
                "Search head must be of the form `relation_name:index_name`"
            );
            let relation = Symbol::new(name_segs[0], name_p.extract_span());
            let index = Symbol::new(name_segs[1], name_p.extract_span());
            let bindings: BTreeMap<SmartString<LazyCompact>, Expr> = src
                .next()
                .unwrap()
                .into_inner()
                .map(|arg| extract_named_apply_arg(arg, param_pool))
                .try_collect()?;
            let parameters: BTreeMap<SmartString<LazyCompact>, Expr> = src
                .map(|arg| extract_named_apply_arg(arg, param_pool))
                .try_collect()?;

            let opts = SearchInput {
                relation,
                index,
                bindings,
                span,
                parameters,
            };

            InputAtom::Search { inner: opts }
        }
        Rule::relation_named_apply => {
            let span = src.extract_span();
            let mut src = src.into_inner();
            let name_p = src.next().unwrap();
            let name = Symbol::new(&name_p.as_str()[1..], name_p.extract_span());
            let args = src
                .next()
                .unwrap()
                .into_inner()
                .map(|arg| extract_named_apply_arg(arg, param_pool))
                .try_collect()?;
            let valid_at = match src.next() {
                None => None,
                Some(vld_clause) => {
                    let vld_expr = build_expr(vld_clause.into_inner().next().unwrap(), param_pool)?;
                    Some(expr2vld_spec(vld_expr, cur_vld)?)
                }
            };
            InputAtom::NamedFieldRelation {
                inner: InputNamedFieldRelationApplyAtom {
                    name,
                    args,
                    span,
                    valid_at,
                },
            }
        }
        r => unreachable!("{:?}", r),
    })
}

fn extract_named_apply_arg(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<(SmartString<LazyCompact>, Expr)> {
    let mut inner = pair.into_inner();
    let name_p = inner.next().unwrap();
    let name = SmartString::from(name_p.as_str());
    let arg = match inner.next() {
        Some(a) => build_expr(a, param_pool)?,
        None => Expr::Binding {
            var: Symbol::new(name.clone(), name_p.extract_span()),
            tuple_pos: None,
        },
    };
    Ok((name, arg))
}

fn parse_rule_head(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<(
    Symbol,
    Vec<Symbol>,
    Vec<Option<(Aggregation, Vec<DataValue>)>>,
)> {
    let mut src = src.into_inner();
    let name = src.next().unwrap();
    let mut args = vec![];
    let mut aggrs = vec![];
    for p in src {
        let (arg, aggr) = parse_rule_head_arg(p, param_pool)?;
        args.push(arg);
        aggrs.push(aggr);
    }
    Ok((Symbol::new(name.as_str(), name.extract_span()), args, aggrs))
}



fn parse_rule_head_arg(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<(Symbol, Option<(Aggregation, Vec<DataValue>)>)> {
    let src = src.into_inner().next().unwrap();
    Ok(match src.as_rule() {
        Rule::var => (Symbol::new(src.as_str(), src.extract_span()), None),
        Rule::aggr_arg => {
            let mut inner = src.into_inner();
            let aggr_p = inner.next().unwrap();
            let aggr_name = aggr_p.as_str();
            let var = inner.next().unwrap();
            let args: Vec<_> = inner
                .map(|v| -> Result<DataValue> { build_expr(v, param_pool)?.eval_to_const() })
                .try_collect()?;
            (
                Symbol::new(var.as_str(), var.extract_span()),
                Some((
                    parse_aggr(aggr_name)
                        .ok_or_else(|| crate::engine::error::AdhocError(format!("Aggregation '{aggr_name}' not found")))?
                        .clone(),
                    args,
                )),
            )
        }
        _ => unreachable!(),
    })
}



fn parse_fixed_rule(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<(Symbol, FixedRuleApply)> {
    let mut src = src.into_inner();
    let (out_symbol, head, aggr) = parse_rule_head(src.next().unwrap(), param_pool)?;

    

    

    for (a, _v) in aggr.iter().zip(head.iter()) {
        ensure!(a.is_none(), crate::engine::error::AdhocError("fixed rule cannot be combined with aggregation".to_string()))
    }

    let mut seen_bindings = BTreeSet::new();
    let mut binding_gen_id = 0;

    let name_pair = src.next().unwrap();
    let fixed_name = &name_pair.as_str();
    let mut rule_args: Vec<FixedRuleArg> = vec![];
    let mut options: BTreeMap<SmartString<LazyCompact>, Expr> = Default::default();
    let args_list = src.next().unwrap();
    let args_list_span = args_list.extract_span();

    for nxt in args_list.into_inner() {
        match nxt.as_rule() {
            Rule::fixed_rel => {
                let inner = nxt.into_inner().next().unwrap();
                let span = inner.extract_span();
                match inner.as_rule() {
                    Rule::fixed_rule_rel => {
                        let mut els = inner.into_inner();
                        let name = els.next().unwrap();
                        let mut bindings = Vec::with_capacity(els.size_hint().1.unwrap_or(4));
                        for v in els {
                            let s = v.as_str();
                            if s == "_" {
                                let symb =
                                    Symbol::new(format!("*_*{binding_gen_id}"), v.extract_span());
                                binding_gen_id += 1;
                                bindings.push(symb);
                            } else {
                                if !seen_bindings.insert(s) {
                                    bail!("fixed rule cannot have duplicate bindings")
                                }
                                let symb = Symbol::new(s, v.extract_span());
                                bindings.push(symb);
                            }
                        }
                        rule_args.push(FixedRuleArg::InMem {
                            name: Symbol::new(name.as_str(), name.extract_span()),
                            bindings,
                            span,
                        })
                    }
                    Rule::fixed_relation_rel => {
                        let mut els = inner.into_inner();
                        let name = els.next().unwrap();
                        let mut bindings = vec![];
                        let mut valid_at = None;
                        for v in els {
                            match v.as_rule() {
                                Rule::var => {
                                    let s = v.as_str();
                                    if s == "_" {
                                        let symb = Symbol::new(
                                            format!("*_*{binding_gen_id}"),
                                            v.extract_span(),
                                        );
                                        binding_gen_id += 1;
                                        bindings.push(symb);
                                    } else {
                                        if !seen_bindings.insert(s) {
                                            bail!("fixed rule cannot have duplicate bindings")
                                        }
                                        bindings.push(Symbol::new(v.as_str(), v.extract_span()))
                                    }
                                }
                                Rule::validity_clause => {
                                    let vld_inner = v.into_inner().next().unwrap();
                                    let vld_expr = build_expr(vld_inner, param_pool)?;
                                    valid_at = Some(expr2vld_spec(vld_expr, cur_vld)?)
                                }
                                _ => unreachable!(),
                            }
                        }
                        rule_args.push(FixedRuleArg::Stored {
                            name: Symbol::new(
                                name.as_str().strip_prefix('*').unwrap(),
                                name.extract_span(),
                            ),
                            bindings,
                            valid_at,
                            span,
                        })
                    }
                    Rule::fixed_named_relation_rel => {
                        let mut els = inner.into_inner();
                        let name = els.next().unwrap();
                        let mut bindings = BTreeMap::new();
                        let mut valid_at = None;
                        for p in els {
                            match p.as_rule() {
                                Rule::fixed_named_relation_arg_pair => {
                                    let mut vs = p.into_inner();
                                    let kp = vs.next().unwrap();
                                    let k = SmartString::from(kp.as_str());
                                    let v = match vs.next() {
                                        Some(vp) => {
                                            if !seen_bindings.insert(vp.as_str()) {
                                                bail!("fixed rule cannot have duplicate bindings")
                                            }
                                            Symbol::new(vp.as_str(), vp.extract_span())
                                        }
                                        None => {
                                            if !seen_bindings.insert(kp.as_str()) {
                                                bail!("fixed rule cannot have duplicate bindings")
                                            }
                                            Symbol::new(k.clone(), kp.extract_span())
                                        }
                                    };
                                    bindings.insert(k, v);
                                }
                                Rule::validity_clause => {
                                    let vld_inner = p.into_inner().next().unwrap();
                                    let vld_expr = build_expr(vld_inner, param_pool)?;
                                    valid_at = Some(expr2vld_spec(vld_expr, cur_vld)?)
                                }
                                _ => unreachable!(),
                            }
                        }

                        rule_args.push(FixedRuleArg::NamedStored {
                            name: Symbol::new(
                                name.as_str().strip_prefix(':').unwrap(),
                                name.extract_span(),
                            ),
                            bindings,
                            valid_at,
                            span,
                        })
                    }
                    _ => unreachable!(),
                }
            }
            Rule::fixed_opt_pair => {
                let mut inner = nxt.into_inner();
                let name = inner.next().unwrap().as_str();
                let val = inner.next().unwrap();
                let val = build_expr(val, param_pool)?;
                options.insert(SmartString::from(name), val);
            }
            _ => unreachable!(),
        }
    }

    let fixed = FixedRuleHandle::new(fixed_name, name_pair.extract_span());

    let fixed_impl = fixed_rules
        .get(&fixed.name as &str)
        .ok_or_else(|| crate::engine::error::AdhocError(format!("Fixed rule '{}' not found", fixed.name)))?;
    fixed_impl.init_options(&mut options, args_list_span)?;
    let arity = fixed_impl.arity(&options, &head, name_pair.extract_span())?;

    ensure!(
        head.is_empty() || arity == head.len(),
        "Fixed rule head arity mismatch"
    );

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





fn make_empty_const_rule(prog: &mut InputProgram, bindings: &[Symbol]) {
    let entry_symbol = Symbol::new(PROG_ENTRY, Default::default());
    let mut options = BTreeMap::new();
    options.insert(
        SmartString::from("data"),
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

fn expr2vld_spec(expr: Expr, cur_vld: ValidityTs) -> Result<ValidityTs> {
    let _vld_span = expr.span();
    match expr.eval_to_const()? {
        DataValue::Num(n) => {
            let microseconds = n.get_int().ok_or(crate::engine::error::AdhocError("bad specification of validity".to_string()))?;
            Ok(ValidityTs(Reverse(microseconds)))
        }
        DataValue::Str(s) => match &s as &str {
            "NOW" => Ok(cur_vld),
            "END" => Ok(MAX_VALIDITY_TS),
            s => Ok(str2vld(s).map_err(|_| crate::engine::error::AdhocError("bad specification of validity".to_string()))?),
        },
        _ => {
            bail!("bad specification of validity")
        }
    }
}
