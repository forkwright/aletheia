//! Query option parsing and post-parse validation.
//!
//! Handles `:timeout`, `:sleep`, `:limit`, `:offset`, `:sort`, `:assert_none`,
//! `:assert_some`, `:disable_magic_rewrite`, and stored relation operations
//! (`:create`, `:put`, `:rm`, `:insert`, `:update`, `:delete`, `:ensure`,
//! `:ensure_not`, `:replace`).
#![expect(
    clippy::pedantic,
    clippy::result_large_err,
    reason = "Query option parser — InternalError is the crate-wide Result type, pedantic style deferred"
)]

use std::collections::BTreeMap;

use either::{Left, Right};

use crate::data::program::{
    InputProgram, QueryAssertion, QueryOutOptions, RelationOp, ReturnMutation, SortDir,
};
use crate::data::relation::{ColType, ColumnDef, NullableColType, StoredRelationMetadata};
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::parse::error::InvalidQuerySnafu;
use crate::parse::expr::build_expr;
use crate::parse::schema::parse_schema;
use crate::parse::{ExtractSpan, Pair, Rule, SourceSpan};
use crate::runtime::relation::InputRelationHandle;

use super::fixed_rules::make_empty_const_rule;

/// Parse a query option that expects a floating-point value.
pub(super) fn parse_query_option_float(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<f64> {
    let inner = pair.into_inner().next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "query option missing value".to_string(),
        }
        .build()
    })?;
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

/// Parse the `:sleep` option, rejecting non-positive values and WASM targets.
pub(super) fn parse_sleep_option(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<f64> {
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

/// Parse a query option that expects a non-negative integer.
pub(super) fn parse_query_option_usize(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<usize> {
    let inner = pair.into_inner().next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "query option missing value".to_string(),
        }
        .build()
    })?;
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
    // INVARIANT: get_non_neg_int already filtered negatives.
    Ok(usize::try_from(n).unwrap_or(usize::MAX))
}

/// Parse a query option that expects a boolean value.
pub(super) fn parse_query_option_bool(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<bool> {
    let inner = pair.into_inner().next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "query option missing value".to_string(),
        }
        .build()
    })?;
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

/// Collect `:sort` option entries into `out_opts.sorters`.
///
/// Each sort entry is a variable name with an ascending or descending direction.
pub(super) fn collect_sort_option(pair: Pair<'_>, out_opts: &mut QueryOutOptions) {
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
                // INVARIANT: grammar guarantees sort_option children are only
                // out_arg, sort_asc, or sort_desc. Any other rule indicates a
                // grammar-parser mismatch.
                _ => {
                    debug_assert!(false, "unexpected rule in sort_option");
                }
            }
        }
        out_opts.sorters.push((Symbol::new(var, span), dir));
    }
}

/// Set the query assertion, rejecting if one is already defined.
pub(super) fn set_unique_assertion(
    out_opts: &mut QueryOutOptions,
    assertion: QueryAssertion,
) -> Result<()> {
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

pub(super) type StoredRelationSpec =
    either::Either<(Symbol, SourceSpan, RelationOp), (InputRelationHandle, RelationOp)>;

/// Parse a stored relation option (`:create`, `:put`, `:rm`, etc.) with optional schema.
pub(super) fn parse_relation_stored_option(
    pair: Pair<'_>,
    _param_pool: &BTreeMap<String, DataValue>,
) -> Result<StoredRelationSpec> {
    let span = pair.extract_span();
    let mut args = pair.into_inner();
    let op_pair = args.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "relation option missing operator".to_string(),
        }
        .build()
    })?;
    let op = match op_pair.as_rule() {
        Rule::relation_create => RelationOp::Create,
        Rule::relation_replace => RelationOp::Replace,
        Rule::relation_put => RelationOp::Put,
        Rule::relation_insert => RelationOp::Insert,
        Rule::relation_update => RelationOp::Update,
        Rule::relation_rm => RelationOp::Rm,
        Rule::relation_delete => RelationOp::Delete,
        Rule::relation_ensure => RelationOp::Ensure,
        Rule::relation_ensure_not => RelationOp::EnsureNot,
        _ => {
            return Err(InvalidQuerySnafu {
                message: "unexpected rule in parser".to_string(),
            }
            .build()
            .into());
        }
    };
    let name_p = args.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "relation option missing name".to_string(),
        }
        .build()
    })?;
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

/// Ensure that an empty `:create` has a placeholder constant rule.
pub(super) fn ensure_empty_create_has_const_rule(prog: &mut InputProgram) {
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

/// Apply the stored relation specification to the program's output options.
pub(super) fn apply_stored_relation(
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

/// Validate that all sort keys reference variables present in the rule head.
pub(super) fn validate_sort_keys(prog: &InputProgram) -> Result<()> {
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

/// If the stored relation has no explicit key bindings, infer them from the entry head.
pub(super) fn resolve_empty_mutation_head(prog: &mut InputProgram) -> Result<()> {
    let empty_mutation_head = match &prog.out_opts.store_relation {
        None => false,
        Some((handle, _, _)) if handle.key_bindings.is_empty() => {
            if handle.dep_bindings.is_empty() {
                true
            } else {
                return Err(InvalidQuerySnafu {
                    message: "Input relation has no keys".to_string(),
                }
                .build()
                .into());
            }
        }
        Some(_) => false,
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
            return Err(InvalidQuerySnafu {
                message: "unexpected empty stored relation".to_string(),
            }
            .build()
            .into());
        }
    }
    Ok(())
}
