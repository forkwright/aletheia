//! System command dispatch: routes each `::` command to the appropriate parser.
//!
//! Index creation for FTS, HNSW, and LSH is delegated to [`super::index`] to
//! keep this file focused on top-level dispatch.
#![expect(
    clippy::pedantic,
    clippy::result_large_err,
    reason = "System command parser — InternalError is the crate-wide Result type, pedantic style deferred"
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use itertools::Itertools;

use crate::FixedRule;
use crate::data::symb::Symbol;
use crate::data::value::{DataValue, ValidityTs};
use crate::error::InternalResult as Result;
use crate::parse::error::InvalidQuerySnafu;
use crate::parse::expr::{build_expr, parse_string};
use crate::parse::query::parse_query;
use crate::parse::{ExtractSpan, Pairs, Rule};
use crate::runtime::relation::AccessLevel;

use super::SysOp;
use super::index::{
    parse_fts_index_create, parse_hnsw_index_create, parse_index_drop, parse_lsh_index_create,
};

/// Parse the inner pairs of a `sys_script` into a [`SysOp`].
///
/// Each system command is a `::keyword` form. This function dispatches on
/// the pest rule to the appropriate handler.
///
/// # Errors
///
/// Returns an error if the command syntax is invalid or an option value
/// cannot be evaluated.
pub(crate) fn parse_sys(
    mut src: Pairs<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    algorithms: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<SysOp> {
    let inner = src.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected system command".to_string(),
        }
        .build()
    })?;
    Ok(match inner.as_rule() {
        Rule::compact_op => SysOp::Compact,
        Rule::running_op => SysOp::ListRunning,
        Rule::kill_op => parse_kill_op(inner, param_pool)?,
        Rule::explain_op => parse_explain_op(inner, param_pool, algorithms, cur_vld)?,
        Rule::describe_relation_op => parse_describe_op(inner)?,
        Rule::list_relations_op => SysOp::ListRelations,
        Rule::remove_relations_op => {
            let rel = inner
                .into_inner()
                .map(|rels_p| Symbol::new(rels_p.as_str(), rels_p.extract_span()))
                .collect_vec();
            SysOp::RemoveRelation(rel)
        }
        Rule::list_columns_op => parse_single_relation_op(inner, SysOp::ListColumns)?,
        Rule::list_indices_op => parse_single_relation_op(inner, SysOp::ListIndices)?,
        Rule::rename_relations_op => parse_rename_op(inner)?,
        Rule::access_level_op => parse_access_level_op(inner)?,
        Rule::trigger_relation_show_op => parse_single_relation_op(inner, SysOp::ShowTrigger)?,
        Rule::trigger_relation_op => parse_trigger_op(inner, param_pool, algorithms, cur_vld)?,
        Rule::lsh_idx_op => parse_adv_index_op(inner, param_pool, parse_lsh_index_create)?,
        Rule::fts_idx_op => parse_adv_index_op(inner, param_pool, parse_fts_index_create)?,
        Rule::vec_idx_op => parse_adv_index_op(inner, param_pool, parse_hnsw_index_create)?,
        Rule::index_op => parse_standard_index_op(inner)?,
        Rule::list_fixed_rules => SysOp::ListFixedRules,
        r => {
            return Err(InvalidQuerySnafu {
                message: format!("unexpected rule {:?} in system parser", r),
            }
            .build()
            .into());
        }
    })
}

/// Parse `::kill <id>`.
fn parse_kill_op(
    inner: crate::parse::Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
) -> Result<SysOp> {
    let i_expr = inner.into_inner().next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected process ID".to_string(),
        }
        .build()
    })?;
    let i_val = build_expr(i_expr, param_pool)?.eval_to_const()?;
    let i_val = i_val.get_int().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "Process ID must be an integer".to_string(),
        }
        .build()
    })?;
    let pid = u64::try_from(i_val).map_err(|_e| {
        InvalidQuerySnafu {
            message: "Process ID must be a non-negative integer".to_string(),
        }
        .build()
    })?;
    Ok(SysOp::KillRunning(pid))
}

/// Parse `::explain { query }`.
fn parse_explain_op(
    inner: crate::parse::Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    algorithms: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<SysOp> {
    let prog = parse_query(
        inner
            .into_inner()
            .next()
            .ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "expected query in explain".to_string(),
                }
                .build()
            })?
            .into_inner(),
        param_pool,
        algorithms,
        cur_vld,
    )?;
    Ok(SysOp::Explain(Box::new(prog)))
}

/// Parse `::describe <relation> [description]`.
fn parse_describe_op(inner: crate::parse::Pair<'_>) -> Result<SysOp> {
    let mut inner = inner.into_inner();
    let rels_p = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected relation name".to_string(),
        }
        .build()
    })?;
    let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
    let description = match inner.next() {
        None => Default::default(),
        Some(desc_p) => parse_string(desc_p)?,
    };
    Ok(SysOp::DescribeRelation(rel, description))
}

/// Parse a system command that takes a single relation argument and wraps
/// it in a `SysOp` variant (e.g., `ListColumns`, `ListIndices`, `ShowTrigger`).
fn parse_single_relation_op(
    inner: crate::parse::Pair<'_>,
    constructor: fn(Symbol) -> SysOp,
) -> Result<SysOp> {
    let rels_p = inner.into_inner().next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected relation name".to_string(),
        }
        .build()
    })?;
    Ok(constructor(Symbol::new(
        rels_p.as_str(),
        rels_p.extract_span(),
    )))
}

/// Parse `::rename <old> -> <new> [, ...]`.
fn parse_rename_op(inner: crate::parse::Pair<'_>) -> Result<SysOp> {
    let mut rename_pairs = vec![];
    for pair in inner.into_inner() {
        let mut src = pair.into_inner();
        let rels_p = src.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected old relation name in rename".to_string(),
            }
            .build()
        })?;
        let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
        let rels_p = src.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected new relation name in rename".to_string(),
            }
            .build()
        })?;
        let new_rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
        rename_pairs.push((rel, new_rel));
    }
    Ok(SysOp::RenameRelation(rename_pairs))
}

/// Parse `::access_level <level> <relation> [, ...]`.
fn parse_access_level_op(inner: crate::parse::Pair<'_>) -> Result<SysOp> {
    let mut ps = inner.into_inner();
    let access_level = match ps
        .next()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected access level".to_string(),
            }
            .build()
        })?
        .as_str()
    {
        "normal" => AccessLevel::Normal,
        "protected" => AccessLevel::Protected,
        "read_only" => AccessLevel::ReadOnly,
        "hidden" => AccessLevel::Hidden,
        s => {
            return Err(InvalidQuerySnafu {
                message: format!("unexpected access level: {}", s),
            }
            .build()
            .into());
        }
    };
    let rels: Vec<_> = ps
        .map(|rel_p| Symbol::new(rel_p.as_str(), rel_p.extract_span()))
        .collect();
    Ok(SysOp::SetAccessLevel(rels, access_level))
}

/// Parse `::set_triggers <relation> { on put { ... } on rm { ... } ... }`.
fn parse_trigger_op(
    inner: crate::parse::Pair<'_>,
    _param_pool: &BTreeMap<String, DataValue>,
    algorithms: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<SysOp> {
    let mut src = inner.into_inner();
    let rels_p = src.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected relation name".to_string(),
        }
        .build()
    })?;
    let rel = Symbol::new(rels_p.as_str(), rels_p.extract_span());
    let mut puts = vec![];
    let mut rms = vec![];
    let mut replaces = vec![];
    for clause in src {
        let mut clause_inner = clause.into_inner();
        let op = clause_inner.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected trigger operation".to_string(),
            }
            .build()
        })?;
        let script = clause_inner.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected trigger script".to_string(),
            }
            .build()
        })?;
        let script_str = script.as_str();
        // Validate the trigger script by parsing it (result is discarded).
        parse_query(
            script.into_inner(),
            &Default::default(),
            algorithms,
            cur_vld,
        )?;
        match op.as_rule() {
            Rule::trigger_put => puts.push(script_str.to_string()),
            Rule::trigger_rm => rms.push(script_str.to_string()),
            Rule::trigger_replace => replaces.push(script_str.to_string()),
            r => {
                return Err(InvalidQuerySnafu {
                    message: format!("unexpected rule {:?} in trigger parser", r),
                }
                .build()
                .into());
            }
        }
    }
    Ok(SysOp::SetTriggers(rel, puts, rms, replaces))
}

/// Parse an advanced index operation (FTS, HNSW, or LSH) that uses
/// `index_create_adv` or `index_drop` syntax.
///
/// The `create_fn` parameter selects which index type to parse.
fn parse_adv_index_op(
    inner: crate::parse::Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    create_fn: fn(crate::parse::Pair<'_>, &BTreeMap<String, DataValue>) -> Result<SysOp>,
) -> Result<SysOp> {
    let inner = inner.into_inner().next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected index operation".to_string(),
        }
        .build()
    })?;
    match inner.as_rule() {
        Rule::index_create_adv => create_fn(inner, param_pool),
        Rule::index_drop => parse_index_drop(inner),
        r => Err(InvalidQuerySnafu {
            message: format!("unexpected rule {:?} in index parser", r),
        }
        .build()
        .into()),
    }
}

/// Parse a standard (B-tree) index operation: `::index create` or `::index drop`.
fn parse_standard_index_op(inner: crate::parse::Pair<'_>) -> Result<SysOp> {
    let inner = inner.into_inner().next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected index operation".to_string(),
        }
        .build()
    })?;
    match inner.as_rule() {
        Rule::index_create => {
            let _span = inner.extract_span();
            let mut inner = inner.into_inner();
            let rel = inner.next().ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "expected relation name".to_string(),
                }
                .build()
            })?;
            let name = inner.next().ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "expected index name".to_string(),
                }
                .build()
            })?;
            let cols: Vec<_> = inner
                .map(|p| Symbol::new(p.as_str(), p.extract_span()))
                .collect();
            if cols.is_empty() {
                return Err(InvalidQuerySnafu {
                    message: "index must have at least one column specified".to_string(),
                }
                .build()
                .into());
            }
            Ok(SysOp::CreateIndex(
                Symbol::new(rel.as_str(), rel.extract_span()),
                Symbol::new(name.as_str(), name.extract_span()),
                cols,
            ))
        }
        Rule::index_drop => parse_index_drop(inner),
        r => Err(InvalidQuerySnafu {
            message: format!("unexpected rule {:?} in index parser", r),
        }
        .build()
        .into()),
    }
}
