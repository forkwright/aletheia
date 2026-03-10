//! Datalog query parsing.
use std::cmp::Reverse;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::bail;
use crate::engine::error::DbResult as Result;
use crate::engine::parse::error::InvalidQuerySnafu;
use compact_str::CompactString;
use either::{Left, Right};
use itertools::Itertools;
use pest::Parser;

use crate::engine::FixedRule;
use crate::engine::data::aggr::{Aggregation, parse_aggr};
use crate::engine::data::expr::Expr;
use crate::engine::data::functions::{MAX_VALIDITY_TS, str2vld};
use crate::engine::data::program::{
    FixedRuleApply, FixedRuleArg, InputAtom, InputInlineRule, InputInlineRulesOrFixed,
    InputNamedFieldRelationApplyAtom, InputProgram, InputRelationApplyAtom, InputRuleApplyAtom,
    QueryAssertion, QueryOutOptions, RelationOp, ReturnMutation, SearchInput, SortDir, Unification,
};
use crate::engine::data::relation::{ColType, ColumnDef, NullableColType, StoredRelationMetadata};
use crate::engine::data::symb::{PROG_ENTRY, Symbol};
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::fixed_rule::FixedRuleHandle;
use crate::engine::fixed_rule::utilities::constant::Constant;
use crate::engine::parse::expr::build_expr;
use crate::engine::parse::schema::parse_schema;
use crate::engine::parse::{DatalogParser, ExtractSpan, Pair, Pairs, Rule};
use crate::engine::runtime::relation::InputRelationHandle;


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
                                let prev = rs.first().expect("rules vec always has at least one element");
                                if prev.aggr != rule.aggr {
                                    bail!(InvalidQuerySnafu {
                                        message: format!("Rule {key} has multiple definitions with conflicting heads")
                                    }
                                    .build());
                                }

                                rs.push(rule);
                            }
                            InputInlineRulesOrFixed::Fixed { .. } => {
                                bail!(InvalidQuerySnafu {
                                    message: format!(
                                        "The rule '{}' cannot have multiple definitions since it contains non-Horn clauses",
                                        e.key().name
                                    )
                                }
                                .build())
                            }
                        }
                    }
                }
            }
            Rule::fixed_rule => {
                let (name, apply) = parse_fixed_rule(pair, param_pool, fixed_rules, cur_vld)?;

                match progs.entry(name) {
                    Entry::Vacant(e) => {
                        e.insert(InputInlineRulesOrFixed::Fixed { fixed: apply });
                    }
                    Entry::Occupied(e) => {
                        let found_name = e.key().name.to_string();
                        bail!(InvalidQuerySnafu {
                            message: format!(
                                "The rule '{found_name}' cannot have multiple definitions since it contains non-Horn clauses"
                            )
                        }
                        .build());
                    }
                }
            }
            Rule::const_rule => {
                let span = pair.extract_span();
                let mut src = pair.into_inner();
                let (name, mut head, aggr) = parse_rule_head(src.next().expect("pest guarantees rule head"), param_pool)?;

                if progs.contains_key(&name) {
                    bail!(InvalidQuerySnafu {
                        message: format!(
                            "The rule '{}' cannot have multiple definitions since it contains non-Horn clauses",
                            name.name
                        )
                    }
                    .build());
                }

                for (a, _v) in aggr.iter().zip(head.iter()) {
                    if a.is_some() {
                        bail!(InvalidQuerySnafu {
                            message: "Constant rules cannot have aggregation application".to_string()
                        }
                        .build());
                    }
                }
                let data_part = src.next().expect("pest guarantees data part after rule head");
                let data_part_str = data_part.as_str();
                let data = build_expr(data_part.clone(), param_pool)?;
                let mut options = BTreeMap::new();
                options.insert(CompactString::from("data"), data);
                let handle = FixedRuleHandle {
                    name: Symbol::new("Constant", span),
                };
                let fixed_impl = Box::new(Constant);
                fixed_impl.init_options(&mut options, span)?;
                let arity = fixed_impl.arity(&options, &head, span)?;

                if arity == 0 {
                    bail!(InvalidQuerySnafu {
                        message: "Encountered empty row for constant rule".to_string()
                    }
                    .build());
                }
                if !head.is_empty() && arity != head.len() {
                    bail!(InvalidQuerySnafu {
                        message: "Fixed rule head arity mismatch".to_string()
                    }
                    .build());
                }
                if head.is_empty() && name.is_prog_entry() {
                    if let Ok(mut datalist) = DatalogParser::parse(Rule::param_list, data_part_str)
                    {
                        for s in datalist.next().expect("pest guarantees param_list token").into_inner() {
                            if s.as_rule() == Rule::param {
                                head.push(Symbol::new(
                                    s.as_str().strip_prefix('$').expect("pest guarantees $ prefix on param"),
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
                let pair = pair.into_inner().next().expect("pest guarantees timeout value");
                let _span = pair.extract_span();
                let timeout = build_expr(pair, param_pool)?
                    .eval_to_const()
                    .map_err(|_err| {
                        crate::engine::error::AdhocError(
                            "Query option {} is not constant".to_string(),
                        )
                    })?
                    .get_float()
                    .ok_or(crate::engine::error::AdhocError(
                        "Query option {} requires a non-negative integer".to_string(),
                    ))?;
                if timeout > 0. {
                    out_opts.timeout = Some(timeout);
                } else {
                    out_opts.timeout = None;
                }
            }
            Rule::sleep_option => {
                #[cfg(target_arch = "wasm32")]
                bail!(InvalidQuerySnafu {
                    message: ":sleep is not supported under WASM".to_string()
                }
                .build());

                #[cfg(not(target_arch = "wasm32"))]
                {
                    let pair = pair.into_inner().next().expect("pest guarantees sleep value");
                    let _span = pair.extract_span();
                    let sleep = build_expr(pair, param_pool)?
                        .eval_to_const()
                        .map_err(|_err| {
                            crate::engine::error::AdhocError(
                                "Query option {} is not constant".to_string(),
                            )
                        })?
                        .get_float()
                        .ok_or(crate::engine::error::AdhocError(
                            "Query option {} requires a non-negative integer".to_string(),
                        ))?;
                    if sleep <= 0. {
                        bail!(InvalidQuerySnafu {
                            message: "Query option :sleep requires a positive integer".to_string()
                        }
                        .build());
                    }
                    out_opts.sleep = Some(sleep);
                }
            }
            Rule::limit_option => {
                let pair = pair.into_inner().next().expect("pest guarantees limit value");
                let _span = pair.extract_span();
                let limit = build_expr(pair, param_pool)?
                    .eval_to_const()
                    .map_err(|_err| {
                        crate::engine::error::AdhocError(
                            "Query option {} is not constant".to_string(),
                        )
                    })?
                    .get_non_neg_int()
                    .ok_or(crate::engine::error::AdhocError(
                        "Query option {} requires a non-negative integer".to_string(),
                    ))?;
                out_opts.limit = Some(limit as usize);
            }
            Rule::offset_option => {
                let pair = pair.into_inner().next().expect("pest guarantees offset value");
                let _span = pair.extract_span();
                let offset = build_expr(pair, param_pool)?
                    .eval_to_const()
                    .map_err(|_err| {
                        crate::engine::error::AdhocError(
                            "Query option {} is not constant".to_string(),
                        )
                    })?
                    .get_non_neg_int()
                    .ok_or(crate::engine::error::AdhocError(
                        "Query option {} requires a non-negative integer".to_string(),
                    ))?;
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
                let op = match args.next().expect("pest guarantees relation op").as_rule() {
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

                let name_p = args.next().expect("pest guarantees relation name");
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
                if out_opts.assertion.is_some() {
                    bail!(InvalidQuerySnafu {
                        message: "Multiple query output assertions defined".to_string()
                    }
                    .build());
                }
                out_opts.assertion = Some(QueryAssertion::AssertNone(pair.extract_span()))
            }
            Rule::assert_some_option => {
                if out_opts.assertion.is_some() {
                    bail!(InvalidQuerySnafu {
                        message: "Multiple query output assertions defined".to_string()
                    }
                    .build());
                }
                out_opts.assertion = Some(QueryAssertion::AssertSome(pair.extract_span()))
            }
            Rule::disable_magic_rewrite_option => {
                let pair = pair.into_inner().next().expect("pest guarantees magic rewrite value");
                let _span = pair.extract_span();
                let val = build_expr(pair, param_pool)?
                    .eval_to_const()
                    .map_err(|_err| {
                        crate::engine::error::AdhocError("Query option is not constant".to_string())
                    })?
                    .get_bool()
                    .ok_or(crate::engine::error::AdhocError(
                        "Query option requires a boolean".to_string(),
                    ))?;
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
            if !head_args.contains(sorter) {
                bail!(InvalidQuerySnafu {
                    message: "Sort key not found".to_string()
                }
                .build());
            }
        }
    }

    let empty_mutation_head = match &prog.out_opts.store_relation {
        None => false,
        Some((handle, _, _)) => {
            if handle.key_bindings.is_empty() {
                if handle.dep_bindings.is_empty() {
                    true
                } else {
                    bail!(InvalidQuerySnafu {
                        message: "Input relation has no keys".to_string()
                    }
                    .build());
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
                bail!(InvalidQuerySnafu {
                    message: "Input relation has no keys".to_string()
                }
                .build());
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
    let head = src.next().expect("pest guarantees rule head");
    let _head_span = head.extract_span();
    let (name, head, aggr) = parse_rule_head(head, param_pool)?;

    if head.is_empty() {
        bail!(InvalidQuerySnafu {
            message: "Horn-clause rule cannot have empty rule head".to_string()
        }
        .build());
    }
    let body = src.next().expect("pest guarantees rule body after head");
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
        res.into_iter().next().expect("just checked len == 1")
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
            src.next().expect("pest guarantees negation marker");
            let inner = parse_atom(src.next().expect("pest guarantees negation body"), param_pool, cur_vld, ignored_counter)?;
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
            let var = src.next().expect("pest guarantees unify variable");
            let mut symb = Symbol::new(var.as_str(), var.extract_span());
            if symb.is_ignored_symbol() {
                symb.name = format!("*^*{}", *ignored_counter).into();
                *ignored_counter += 1;
            }
            let expr = build_expr(src.next().expect("pest guarantees unify expression"), param_pool)?;
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
            let var = src.next().expect("pest guarantees unify_multi variable");
            let mut symb = Symbol::new(var.as_str(), var.extract_span());
            if symb.is_ignored_symbol() {
                symb.name = format!("*^*{}", *ignored_counter).into();
                *ignored_counter += 1;
            }
            src.next().expect("pest guarantees unify_multi separator");
            let expr = build_expr(src.next().expect("pest guarantees unify_multi expression"), param_pool)?;
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
            let name = src.next().expect("pest guarantees rule_apply name");
            let args: Vec<_> = src
                .next()
                .expect("pest guarantees rule_apply args")
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
            let name = src.next().expect("pest guarantees relation_apply name");
            let args: Vec<_> = src
                .next()
                .expect("pest guarantees relation_apply args")
                .into_inner()
                .map(|v| build_expr(v, param_pool))
                .try_collect()?;
            let valid_at = match src.next() {
                None => None,
                Some(vld_clause) => {
                    let vld_expr = build_expr(vld_clause.into_inner().next().expect("pest guarantees validity expr"), param_pool)?;
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
            let name_p = src.next().expect("pest guarantees search_apply name");
            let name_segs = name_p.as_str().split(':').collect_vec();

            if name_segs.len() != 2 {
                bail!(InvalidQuerySnafu {
                    message: "Search head must be of the form `relation_name:index_name`".to_string()
                }
                .build());
            }
            let relation = Symbol::new(name_segs[0], name_p.extract_span());
            let index = Symbol::new(name_segs[1], name_p.extract_span());
            let bindings: BTreeMap<CompactString, Expr> = src
                .next()
                .expect("pest guarantees search_apply bindings")
                .into_inner()
                .map(|arg| extract_named_apply_arg(arg, param_pool))
                .try_collect()?;
            let parameters: BTreeMap<CompactString, Expr> = src
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
            let name_p = src.next().expect("pest guarantees relation_named_apply name");
            let name = Symbol::new(&name_p.as_str()[1..], name_p.extract_span());
            let args = src
                .next()
                .expect("pest guarantees relation_named_apply args")
                .into_inner()
                .map(|arg| extract_named_apply_arg(arg, param_pool))
                .try_collect()?;
            let valid_at = match src.next() {
                None => None,
                Some(vld_clause) => {
                    let vld_expr = build_expr(vld_clause.into_inner().next().expect("pest guarantees validity expr"), param_pool)?;
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
) -> Result<(CompactString, Expr)> {
    let mut inner = pair.into_inner();
    let name_p = inner.next().expect("pest guarantees named arg key");
    let name = CompactString::from(name_p.as_str());
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
    let name = src.next().expect("pest guarantees rule head name");
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
    let src = src.into_inner().next().expect("pest guarantees rule head arg inner");
    Ok(match src.as_rule() {
        Rule::var => (Symbol::new(src.as_str(), src.extract_span()), None),
        Rule::aggr_arg => {
            let mut inner = src.into_inner();
            let aggr_p = inner.next().expect("pest guarantees aggregation name");
            let aggr_name = aggr_p.as_str();
            let var = inner.next().expect("pest guarantees aggregation variable");
            let args: Vec<_> = inner
                .map(|v| -> Result<DataValue> { build_expr(v, param_pool)?.eval_to_const() })
                .try_collect()?;
            (
                Symbol::new(var.as_str(), var.extract_span()),
                Some((
                    parse_aggr(aggr_name)
                        .ok_or_else(|| {
                            crate::engine::error::AdhocError(format!(
                                "Aggregation '{aggr_name}' not found"
                            ))
                        })?
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
    let (out_symbol, head, aggr) = parse_rule_head(src.next().expect("pest guarantees fixed rule head"), param_pool)?;

    for (a, _v) in aggr.iter().zip(head.iter()) {
        if a.is_some() {
            bail!(InvalidQuerySnafu {
                message: "fixed rule cannot be combined with aggregation".to_string()
            }
            .build());
        }
    }

    let mut seen_bindings = BTreeSet::new();
    let mut binding_gen_id = 0;

    let name_pair = src.next().expect("pest guarantees fixed rule name");
    let fixed_name = &name_pair.as_str();
    let mut rule_args: Vec<FixedRuleArg> = vec![];
    let mut options: BTreeMap<CompactString, Expr> = Default::default();
    let args_list = src.next().expect("pest guarantees fixed rule args list");
    let args_list_span = args_list.extract_span();

    for nxt in args_list.into_inner() {
        match nxt.as_rule() {
            Rule::fixed_rel => {
                let inner = nxt.into_inner().next().expect("pest guarantees fixed_rel inner");
                let span = inner.extract_span();
                match inner.as_rule() {
                    Rule::fixed_rule_rel => {
                        let mut els = inner.into_inner();
                        let name = els.next().expect("pest guarantees fixed_rule_rel name");
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
                                    bail!(InvalidQuerySnafu {
                                        message: "fixed rule cannot have duplicate bindings".to_string()
                                    }
                                    .build());
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
                        let name = els.next().expect("pest guarantees fixed_relation_rel name");
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
                                            bail!(InvalidQuerySnafu {
                                                message: "fixed rule cannot have duplicate bindings".to_string()
                                            }
                                            .build());
                                        }
                                        bindings.push(Symbol::new(v.as_str(), v.extract_span()))
                                    }
                                }
                                Rule::validity_clause => {
                                    let vld_inner = v.into_inner().next().expect("pest guarantees validity expr");
                                    let vld_expr = build_expr(vld_inner, param_pool)?;
                                    valid_at = Some(expr2vld_spec(vld_expr, cur_vld)?)
                                }
                                _ => unreachable!(),
                            }
                        }
                        rule_args.push(FixedRuleArg::Stored {
                            name: Symbol::new(
                                name.as_str().strip_prefix('*').expect("pest guarantees * prefix on stored relation"),
                                name.extract_span(),
                            ),
                            bindings,
                            valid_at,
                            span,
                        })
                    }
                    Rule::fixed_named_relation_rel => {
                        let mut els = inner.into_inner();
                        let name = els.next().expect("pest guarantees fixed_named_relation_rel name");
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
                                                bail!(InvalidQuerySnafu {
                                                    message: "fixed rule cannot have duplicate bindings".to_string()
                                                }
                                                .build());
                                            }
                                            Symbol::new(vp.as_str(), vp.extract_span())
                                        }
                                        None => {
                                            if !seen_bindings.insert(kp.as_str()) {
                                                bail!(InvalidQuerySnafu {
                                                    message: "fixed rule cannot have duplicate bindings".to_string()
                                                }
                                                .build());
                                            }
                                            Symbol::new(k.clone(), kp.extract_span())
                                        }
                                    };
                                    bindings.insert(k, v);
                                }
                                Rule::validity_clause => {
                                    let vld_inner = p.into_inner().next().expect("pest guarantees validity expr");
                                    let vld_expr = build_expr(vld_inner, param_pool)?;
                                    valid_at = Some(expr2vld_spec(vld_expr, cur_vld)?)
                                }
                                _ => unreachable!(),
                            }
                        }

                        rule_args.push(FixedRuleArg::NamedStored {
                            name: Symbol::new(
                                name.as_str().strip_prefix(':').expect("pest guarantees : prefix on named stored relation"),
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
                let name = inner.next().expect("pest guarantees option name").as_str();
                let val = inner.next().expect("pest guarantees option value");
                let val = build_expr(val, param_pool)?;
                options.insert(CompactString::from(name), val);
            }
            _ => unreachable!(),
        }
    }

    let fixed = FixedRuleHandle::new(fixed_name, name_pair.extract_span());

    let fixed_impl = fixed_rules.get(&fixed.name as &str).ok_or_else(|| {
        crate::engine::error::AdhocError(format!("Fixed rule '{}' not found", fixed.name))
    })?;
    fixed_impl.init_options(&mut options, args_list_span)?;
    let arity = fixed_impl.arity(&options, &head, name_pair.extract_span())?;

    if !head.is_empty() && arity != head.len() {
        bail!(InvalidQuerySnafu {
            message: "Fixed rule head arity mismatch".to_string()
        }
        .build());
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

fn make_empty_const_rule(prog: &mut InputProgram, bindings: &[Symbol]) {
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

fn expr2vld_spec(expr: Expr, cur_vld: ValidityTs) -> Result<ValidityTs> {
    let _vld_span = expr.span();
    match expr.eval_to_const()? {
        DataValue::Num(n) => {
            let microseconds = n.get_int().ok_or(crate::engine::error::AdhocError(
                "bad specification of validity".to_string(),
            ))?;
            Ok(ValidityTs(Reverse(microseconds)))
        }
        DataValue::Str(s) => match &s as &str {
            "NOW" => Ok(cur_vld),
            "END" => Ok(MAX_VALIDITY_TS),
            s => Ok(str2vld(s).map_err(|e| {
                crate::engine::error::AdhocError(format!("bad specification of validity: {e}"))
            })?),
        },
        _ => {
            bail!(InvalidQuerySnafu {
                message: "bad specification of validity".to_string()
            }
            .build())
        }
    }
}
