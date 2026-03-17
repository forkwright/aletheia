//! Datalog query parsing.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::cmp::Reverse;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::engine::error::InternalResult as Result;
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
use crate::engine::parse::{DatalogParser, ExtractSpan, Pair, Pairs, Rule, SourceSpan};
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
            Rule::rule => add_rule_to_program(pair, param_pool, cur_vld, &mut progs)?,
            Rule::fixed_rule => {
                add_fixed_rule_to_program(pair, param_pool, fixed_rules, cur_vld, &mut progs)?;
            }
            Rule::const_rule => add_const_rule_to_program(pair, param_pool, &mut progs)?,
            Rule::timeout_option => {
                let timeout = parse_query_option_float(pair, param_pool)?;
                out_opts.timeout = if timeout > 0. { Some(timeout) } else { None };
            }
            Rule::sleep_option => {
                out_opts.sleep = Some(parse_sleep_option(pair, param_pool)?);
            }
            Rule::limit_option => {
                out_opts.limit = Some(parse_query_option_usize(pair, param_pool)?);
            }
            Rule::offset_option => {
                out_opts.offset = Some(parse_query_option_usize(pair, param_pool)?);
            }
            Rule::sort_option => {
                collect_sort_option(pair, &mut out_opts);
            }
            Rule::returning_option => {
                returning_mutation = ReturnMutation::Returning;
            }
            Rule::relation_option => {
                stored_relation = Some(parse_relation_stored_option(pair, param_pool)?);
            }
            Rule::assert_none_option => {
                set_unique_assertion(
                    &mut out_opts,
                    QueryAssertion::AssertNone(pair.extract_span()),
                )?;
            }
            Rule::assert_some_option => {
                set_unique_assertion(
                    &mut out_opts,
                    QueryAssertion::AssertSome(pair.extract_span()),
                )?;
            }
            Rule::disable_magic_rewrite_option => {
                disable_magic_rewrite = parse_query_option_bool(pair, param_pool)?;
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

    ensure_empty_create_has_const_rule(&mut prog);
    apply_stored_relation(&mut prog, stored_relation, returning_mutation)?;
    ensure_empty_create_has_const_rule(&mut prog);
    validate_sort_keys(&mut prog)?;
    resolve_empty_mutation_head(&mut prog)?;

    Ok(prog)
}

fn add_rule_to_program(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
    progs: &mut BTreeMap<Symbol, InputInlineRulesOrFixed>,
) -> Result<()> {
    let (name, rule) = parse_rule(pair, param_pool, cur_vld)?;
    match progs.entry(name) {
        Entry::Vacant(e) => {
            e.insert(InputInlineRulesOrFixed::Rules { rules: vec![rule] });
        }
        Entry::Occupied(mut e) => {
            let key = e.key().to_string();
            match e.get_mut() {
                InputInlineRulesOrFixed::Rules { rules: rs } => {
                    let prev = rs
                        .first()
                        .expect("rules vec always has at least one element");
                    if prev.aggr != rule.aggr {
                        return Err(InvalidQuerySnafu {
                            message: format!(
                                "Rule {key} has multiple definitions with conflicting heads"
                            ),
                        }
                        .build()
                        .into());
                    }
                    rs.push(rule);
                }
                InputInlineRulesOrFixed::Fixed { .. } => {
                    return Err(InvalidQuerySnafu {
                        message: format!(
                            "The rule '{}' cannot have multiple definitions since it contains non-Horn clauses",
                            e.key().name
                        ),
                    }
                    .build()
                    .into());
                }
            }
        }
    }
    Ok(())
}

fn add_fixed_rule_to_program(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
    progs: &mut BTreeMap<Symbol, InputInlineRulesOrFixed>,
) -> Result<()> {
    let (name, apply) = parse_fixed_rule(pair, param_pool, fixed_rules, cur_vld)?;
    match progs.entry(name) {
        Entry::Vacant(e) => {
            e.insert(InputInlineRulesOrFixed::Fixed { fixed: apply });
        }
        Entry::Occupied(e) => {
            return Err(InvalidQuerySnafu {
                message: format!(
                    "The rule '{}' cannot have multiple definitions since it contains non-Horn clauses",
                    e.key().name
                ),
            }
            .build()
            .into());
        }
    }
    Ok(())
}

fn add_const_rule_to_program(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    progs: &mut BTreeMap<Symbol, InputInlineRulesOrFixed>,
) -> Result<()> {
    let span = pair.extract_span();
    let mut src = pair.into_inner();
    let (name, head, aggr) =
        parse_rule_head(src.next().expect("pest guarantees rule head"), param_pool)?;

    if progs.contains_key(&name) {
        return Err(InvalidQuerySnafu {
            message: format!(
                "The rule '{}' cannot have multiple definitions since it contains non-Horn clauses",
                name.name
            ),
        }
        .build()
        .into());
    }

    for (a, _v) in aggr.iter().zip(head.iter()) {
        if a.is_some() {
            return Err(InvalidQuerySnafu {
                message: "Constant rules cannot have aggregation application".to_string(),
            }
            .build()
            .into());
        }
    }

    let data_part = src
        .next()
        .expect("pest guarantees data part after rule head");
    build_and_insert_const_rule(name, head, data_part, param_pool, progs, span)
}

fn build_and_insert_const_rule(
    name: Symbol,
    mut head: Vec<Symbol>,
    data_part: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    progs: &mut BTreeMap<Symbol, InputInlineRulesOrFixed>,
    span: SourceSpan,
) -> Result<()> {
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
        return Err(InvalidQuerySnafu {
            message: "Encountered empty row for constant rule".to_string(),
        }
        .build()
        .into());
    }
    if !head.is_empty() && arity != head.len() {
        return Err(InvalidQuerySnafu {
            message: "Fixed rule head arity mismatch".to_string(),
        }
        .build()
        .into());
    }

    if head.is_empty()
        && name.is_prog_entry()
        && let Ok(mut datalist) = DatalogParser::parse(Rule::param_list, data_part_str)
    {
        extend_head_from_params(
            &mut head,
            datalist.next().expect("pest guarantees param_list token"),
        );
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
    Ok(())
}

fn extend_head_from_params(head: &mut Vec<Symbol>, param_list: Pair<'_>) {
    for s in param_list.into_inner() {
        if s.as_rule() == Rule::param {
            head.push(Symbol::new(
                s.as_str()
                    .strip_prefix('$')
                    .expect("pest guarantees $ prefix on param"),
                Default::default(),
            ));
        }
    }
}

fn parse_query_option_float(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<f64> {
    let inner = pair
        .into_inner()
        .next()
        .expect("pest guarantees option value");
    build_expr(inner, param_pool)?
        .eval_to_const()
        .map_err(|_err| {
            InvalidQuerySnafu {
                message: "Query option is not constant".to_string(),
            }
            .build()
        })?
        .get_float()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "Query option requires a non-negative number".to_string(),
            }
            .build()
        })
        .map_err(Into::into)
}

fn parse_sleep_option(pair: Pair<'_>, param_pool: &BTreeMap<String, DataValue>) -> Result<f64> {
    #[cfg(target_arch = "wasm32")]
    return Err(InvalidQuerySnafu {
        message: ":sleep is not supported under WASM".to_string(),
    }
    .build()
    .into());

    #[cfg(not(target_arch = "wasm32"))]
    {
        let sleep = parse_query_option_float(pair, param_pool)?;
        if sleep <= 0. {
            return Err(InvalidQuerySnafu {
                message: "Query option :sleep requires a positive integer".to_string(),
            }
            .build()
            .into());
        }
        Ok(sleep)
    }
}

fn parse_query_option_usize(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<usize> {
    let inner = pair
        .into_inner()
        .next()
        .expect("pest guarantees option value");
    let n = build_expr(inner, param_pool)?
        .eval_to_const()
        .map_err(|_err| {
            InvalidQuerySnafu {
                message: "Query option is not constant".to_string(),
            }
            .build()
        })?
        .get_non_neg_int()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "Query option requires a non-negative integer".to_string(),
            }
            .build()
        })?;
    Ok(n as usize)
}

fn parse_query_option_bool(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<bool> {
    let inner = pair
        .into_inner()
        .next()
        .expect("pest guarantees option value");
    build_expr(inner, param_pool)?
        .eval_to_const()
        .map_err(|_err| {
            InvalidQuerySnafu {
                message: "Query option is not constant".to_string(),
            }
            .build()
        })?
        .get_bool()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "Query option requires a boolean".to_string(),
            }
            .build()
        })
        .map_err(Into::into)
}

fn collect_sort_option(pair: Pair<'_>, out_opts: &mut QueryOutOptions) {
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

fn set_unique_assertion(out_opts: &mut QueryOutOptions, assertion: QueryAssertion) -> Result<()> {
    if out_opts.assertion.is_some() {
        return Err(InvalidQuerySnafu {
            message: "Multiple query output assertions defined".to_string(),
        }
        .build()
        .into());
    }
    out_opts.assertion = Some(assertion);
    Ok(())
}

type StoredRelationSpec =
    either::Either<(Symbol, SourceSpan, RelationOp), (InputRelationHandle, RelationOp)>;

fn parse_relation_stored_option(
    pair: Pair<'_>,
    _param_pool: &BTreeMap<String, DataValue>,
) -> Result<StoredRelationSpec> {
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
        None => Ok(Left((name, span, op))),
        Some(schema_p) => {
            let (mut metadata, mut key_bindings, mut dep_bindings) = parse_schema(schema_p)?;
            if !matches!(op, RelationOp::Create | RelationOp::Replace) {
                key_bindings.extend(dep_bindings);
                dep_bindings = vec![];
                metadata.keys.extend(metadata.non_keys);
                metadata.non_keys = vec![];
            }
            Ok(Right((
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

fn ensure_empty_create_has_const_rule(prog: &mut InputProgram) {
    if prog.prog.is_empty()
        && let Some((
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
        make_empty_const_rule(prog, &bindings);
    }
}

fn apply_stored_relation(
    prog: &mut InputProgram,
    stored_relation: Option<StoredRelationSpec>,
    returning_mutation: ReturnMutation,
) -> Result<()> {
    match stored_relation {
        // NOTE: no stored relation specified, query returns results directly
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
            prog.out_opts.store_relation = Some((handle, op, returning_mutation));
        }
        Some(Right((h, o))) => {
            prog.out_opts.store_relation = Some((h, o, returning_mutation));
        }
    }
    Ok(())
}

fn validate_sort_keys(prog: &mut InputProgram) -> Result<()> {
    if prog.out_opts.sorters.is_empty() {
        return Ok(());
    }
    let head_args = prog.get_entry_out_head()?;
    for (sorter, _) in &prog.out_opts.sorters {
        if !head_args.contains(sorter) {
            return Err(InvalidQuerySnafu {
                message: "Sort key not found".to_string(),
            }
            .build()
            .into());
        }
    }
    Ok(())
}

fn resolve_empty_mutation_head(prog: &mut InputProgram) -> Result<()> {
    let empty_mutation_head = match &prog.out_opts.store_relation {
        None => false,
        Some((handle, _, _)) => {
            if handle.key_bindings.is_empty() {
                if handle.dep_bindings.is_empty() {
                    true
                } else {
                    return Err(InvalidQuerySnafu {
                        message: "Input relation has no keys".to_string(),
                    }
                    .build()
                    .into());
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
                return Err(InvalidQuerySnafu {
                    message: "Input relation has no keys".to_string(),
                }
                .build()
                .into());
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
    Ok(())
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
        return Err(InvalidQuerySnafu {
            message: "Horn-clause rule cannot have empty rule head".to_string(),
        }
        .build()
        .into());
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
            let inner = parse_atom(
                src.next().expect("pest guarantees negation body"),
                param_pool,
                cur_vld,
                ignored_counter,
            )?;
            InputAtom::Negation {
                inner: inner.into(),
                span,
            }
        }
        Rule::expr => {
            let expr = build_expr(src, param_pool)?;
            InputAtom::Predicate { inner: expr }
        }
        Rule::unify => parse_unify_atom(src, param_pool, ignored_counter)?,
        Rule::unify_multi => parse_unify_multi_atom(src, param_pool, ignored_counter)?,
        Rule::rule_apply => parse_rule_apply_atom(src, param_pool)?,
        Rule::relation_apply => parse_relation_apply_atom(src, param_pool, cur_vld)?,
        Rule::search_apply => parse_search_apply_atom(src, param_pool)?,
        Rule::relation_named_apply => parse_relation_named_apply_atom(src, param_pool, cur_vld)?,
        r => unreachable!("{:?}", r),
    })
}

fn parse_unify_atom(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    ignored_counter: &mut u32,
) -> Result<InputAtom> {
    let span = src.extract_span();
    let mut src = src.into_inner();
    let var = src.next().expect("pest guarantees unify variable");
    let mut symb = Symbol::new(var.as_str(), var.extract_span());
    if symb.is_ignored_symbol() {
        symb.name = format!("*^*{}", *ignored_counter).into();
        *ignored_counter += 1;
    }
    let expr = build_expr(
        src.next().expect("pest guarantees unify expression"),
        param_pool,
    )?;
    Ok(InputAtom::Unification {
        inner: Unification {
            binding: symb,
            expr,
            one_many_unif: false,
            span,
        },
    })
}

fn parse_unify_multi_atom(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    ignored_counter: &mut u32,
) -> Result<InputAtom> {
    let span = src.extract_span();
    let mut src = src.into_inner();
    let var = src.next().expect("pest guarantees unify_multi variable");
    let mut symb = Symbol::new(var.as_str(), var.extract_span());
    if symb.is_ignored_symbol() {
        symb.name = format!("*^*{}", *ignored_counter).into();
        *ignored_counter += 1;
    }
    src.next().expect("pest guarantees unify_multi separator");
    let expr = build_expr(
        src.next().expect("pest guarantees unify_multi expression"),
        param_pool,
    )?;
    Ok(InputAtom::Unification {
        inner: Unification {
            binding: symb,
            expr,
            one_many_unif: true,
            span,
        },
    })
}

fn parse_rule_apply_atom(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<InputAtom> {
    let span = src.extract_span();
    let mut src = src.into_inner();
    let name = src.next().expect("pest guarantees rule_apply name");
    let args: Vec<_> = src
        .next()
        .expect("pest guarantees rule_apply args")
        .into_inner()
        .map(|v| build_expr(v, param_pool))
        .try_collect()?;
    Ok(InputAtom::Rule {
        inner: InputRuleApplyAtom {
            name: Symbol::new(name.as_str(), name.extract_span()),
            args,
            span,
        },
    })
}

fn parse_relation_apply_atom(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
) -> Result<InputAtom> {
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
            let vld_expr = build_expr(
                vld_clause
                    .into_inner()
                    .next()
                    .expect("pest guarantees validity expr"),
                param_pool,
            )?;
            Some(expr2vld_spec(vld_expr, cur_vld)?)
        }
    };
    Ok(InputAtom::Relation {
        inner: InputRelationApplyAtom {
            name: Symbol::new(name.as_str().get(1..).unwrap_or(""), name.extract_span()),
            args,
            valid_at,
            span,
        },
    })
}

fn parse_search_apply_atom(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<InputAtom> {
    let span = src.extract_span();
    let mut src = src.into_inner();
    let name_p = src.next().expect("pest guarantees search_apply name");
    let name_segs = name_p.as_str().split(':').collect_vec();

    if name_segs.len() != 2 {
        return Err(InvalidQuerySnafu {
            message: "Search head must be of the form `relation_name:index_name`".to_string(),
        }
        .build()
        .into());
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

    Ok(InputAtom::Search {
        inner: SearchInput {
            relation,
            index,
            bindings,
            span,
            parameters,
        },
    })
}

fn parse_relation_named_apply_atom(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
) -> Result<InputAtom> {
    let span = src.extract_span();
    let mut src = src.into_inner();
    let name_p = src
        .next()
        .expect("pest guarantees relation_named_apply name");
    let name = Symbol::new(
        name_p.as_str().get(1..).unwrap_or(""),
        name_p.extract_span(),
    );
    let args = src
        .next()
        .expect("pest guarantees relation_named_apply args")
        .into_inner()
        .map(|arg| extract_named_apply_arg(arg, param_pool))
        .try_collect()?;
    let valid_at = match src.next() {
        None => None,
        Some(vld_clause) => {
            let vld_expr = build_expr(
                vld_clause
                    .into_inner()
                    .next()
                    .expect("pest guarantees validity expr"),
                param_pool,
            )?;
            Some(expr2vld_spec(vld_expr, cur_vld)?)
        }
    };
    Ok(InputAtom::NamedFieldRelation {
        inner: InputNamedFieldRelationApplyAtom {
            name,
            args,
            span,
            valid_at,
        },
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
    let src = src
        .into_inner()
        .next()
        .expect("pest guarantees rule head arg inner");
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
                            InvalidQuerySnafu {
                                message: format!("Aggregation '{aggr_name}' not found"),
                            }
                            .build()
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
