//! Rule and atom parsing: rules, disjunctions, atoms, applications.
//! Datalog query parsing.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]

use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;

use crate::data::aggr::{Aggregation, parse_aggr};
use crate::data::expr::Expr;
use crate::data::program::{
    InputAtom, InputInlineRule, InputNamedFieldRelationApplyAtom, InputRelationApplyAtom,
    InputRuleApplyAtom, SearchInput, Unification,
};
use crate::data::symb::Symbol;
use crate::data::value::{DataValue, ValidityTs};
use crate::error::InternalResult as Result;
use crate::parse::error::InvalidQuerySnafu;
use crate::parse::expr::build_expr;
use crate::parse::{ExtractSpan, Pair, Rule};

use super::fixed_rules::expr2vld_spec;

pub(crate) fn parse_rule(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    cur_vld: ValidityTs,
) -> Result<(Symbol, InputInlineRule)> {
    let span = src.extract_span();
    let mut src = src.into_inner();
    let head = src.next().unwrap_or_else(|| unreachable!());
    let _head_span = head.extract_span();
    let (name, head, aggr) = parse_rule_head(head, param_pool)?;

    if head.is_empty() {
        return Err(InvalidQuerySnafu {
            message: "Horn-clause rule cannot have empty rule head".to_string(),
        }
        .build()
        .into());
    }
    let body = src.next().unwrap_or_else(|| unreachable!());
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
        res.into_iter().next().unwrap_or_else(|| unreachable!())
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
            src.next().unwrap_or_else(|| unreachable!());
            let inner = parse_atom(
                src.next().unwrap_or_else(|| unreachable!()),
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
    let var = src.next().unwrap_or_else(|| unreachable!());
    let mut symb = Symbol::new(var.as_str(), var.extract_span());
    if symb.is_ignored_symbol() {
        symb.name = format!("*^*{}", *ignored_counter).into();
        *ignored_counter += 1;
    }
    let expr = build_expr(src.next().unwrap_or_else(|| unreachable!()), param_pool)?;
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
    let var = src.next().unwrap_or_else(|| unreachable!());
    let mut symb = Symbol::new(var.as_str(), var.extract_span());
    if symb.is_ignored_symbol() {
        symb.name = format!("*^*{}", *ignored_counter).into();
        *ignored_counter += 1;
    }
    src.next().unwrap_or_else(|| unreachable!());
    let expr = build_expr(src.next().unwrap_or_else(|| unreachable!()), param_pool)?;
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
    let name = src.next().unwrap_or_else(|| unreachable!());
    let args: Vec<_> = src
        .next()
        .unwrap_or_else(|| unreachable!())
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
    let name = src.next().unwrap_or_else(|| unreachable!());
    let args: Vec<_> = src
        .next()
        .unwrap_or_else(|| unreachable!())
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
                    .unwrap_or_else(|| unreachable!()),
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
    let name_p = src.next().unwrap_or_else(|| unreachable!());
    let name_segs = name_p.as_str().split(':').collect_vec();

    if name_segs.len() != 2 {
        return Err(InvalidQuerySnafu {
            message: "Search head must be of the form `relation_name:index_name`".to_string(),
        }
        .build()
        .into());
    }
    #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
    let relation = Symbol::new(name_segs[0], name_p.extract_span());
    #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
    let index = Symbol::new(name_segs[1], name_p.extract_span());
    let bindings: BTreeMap<CompactString, Expr> = src
        .next()
        .unwrap_or_else(|| unreachable!())
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
    let name_p = src.next().unwrap_or_else(|| unreachable!());
    let name = Symbol::new(
        name_p.as_str().get(1..).unwrap_or(""),
        name_p.extract_span(),
    );
    let args = src
        .next()
        .unwrap_or_else(|| unreachable!())
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
                    .unwrap_or_else(|| unreachable!()),
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
    let name_p = inner.next().unwrap_or_else(|| unreachable!());
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

pub(crate) fn parse_rule_head(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<(
    Symbol,
    Vec<Symbol>,
    Vec<Option<(Aggregation, Vec<DataValue>)>>,
)> {
    let mut src = src.into_inner();
    let name = src.next().unwrap_or_else(|| unreachable!());
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
    let src = src.into_inner().next().unwrap_or_else(|| unreachable!());
    Ok(match src.as_rule() {
        Rule::var => (Symbol::new(src.as_str(), src.extract_span()), None),
        Rule::aggr_arg => {
            let mut inner = src.into_inner();
            let aggr_p = inner.next().unwrap_or_else(|| unreachable!());
            let aggr_name = aggr_p.as_str();
            let var = inner.next().unwrap_or_else(|| unreachable!());
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
