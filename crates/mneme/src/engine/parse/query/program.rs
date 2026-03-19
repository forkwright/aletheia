//! Query program parsing and construction.
//! Datalog query parsing.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Arc;

use crate::engine::error::InternalResult as Result;
use crate::engine::parse::error::InvalidQuerySnafu;
use compact_str::CompactString;
use either::{Left, Right};
use pest::Parser;

use crate::engine::FixedRule;
use crate::engine::data::program::{
    FixedRuleApply, InputInlineRulesOrFixed, InputProgram, QueryAssertion, QueryOutOptions,
    RelationOp, ReturnMutation, SortDir,
};
use crate::engine::data::relation::{ColType, ColumnDef, NullableColType, StoredRelationMetadata};
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::{DataValue, ValidityTs};
use crate::engine::fixed_rule::FixedRuleHandle;
use crate::engine::fixed_rule::utilities::constant::Constant;
use crate::engine::parse::expr::build_expr;
use crate::engine::parse::schema::parse_schema;
use crate::engine::parse::{DatalogParser, ExtractSpan, Pair, Pairs, Rule, SourceSpan};
use crate::engine::runtime::relation::InputRelationHandle;

use super::atoms::{parse_rule, parse_rule_head};
use super::fixed_rules::{make_empty_const_rule, parse_fixed_rule};

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
