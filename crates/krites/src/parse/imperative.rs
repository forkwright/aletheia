//! Imperative script parsing.
//!
//! Parses `%if`/`%if_not`/`%loop`/`%return`/`%break`/`%continue`/`%swap`/`%debug`
//! control flow blocks that wrap Datalog query programs. These blocks enable
//! procedural logic within otherwise declarative Datalog scripts.
#![expect(
    clippy::pedantic,
    clippy::result_large_err,
    reason = "Imperative block parser — InternalError is the crate-wide Result type, pedantic style deferred"
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use compact_str::CompactString;
use either::{Left, Right};
use itertools::Itertools;

use crate::error::InternalResult as Result;
use crate::parse::query::parse_query;
use crate::parse::sys::parse_sys;
use crate::parse::{
    error::InvalidQuerySnafu, ExtractSpan, ImperativeProgram, ImperativeStmt, ImperativeStmtClause,
    ImperativeSysop, Pair, Rule,
};
use crate::{DataValue, FixedRule, ValidityTs};

/// Parse an imperative script block (the top-level `{ ... }` wrapper).
///
/// Iterates over child statements, delegating each to [`parse_imperative_stmt`],
/// and collects them into an [`ImperativeProgram`].
///
/// # Errors
///
/// Returns an error if any child statement contains invalid syntax.
pub(crate) fn parse_imperative_block(
    src: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<ImperativeProgram> {
    let mut collected = vec![];

    for pair in src.into_inner() {
        if pair.as_rule() == Rule::EOI {
            break;
        }
        collected.push(parse_imperative_stmt(
            pair,
            param_pool,
            fixed_rules,
            cur_vld,
        )?);
    }

    Ok(collected)
}

/// Parse a query program embedded in an imperative context, with an optional
/// `as <name>` storage binding.
fn parse_query_with_store_as(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<ImperativeStmtClause> {
    let mut src = pair.into_inner();
    let prog = parse_query(
        src.next()
            .ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "expected query program".to_string(),
                }
                .build()
            })?
            .into_inner(),
        param_pool,
        fixed_rules,
        cur_vld,
    )?;
    let store_as = src.next().map(|p| CompactString::from(p.as_str().trim()));
    Ok(ImperativeStmtClause { prog, store_as })
}

/// Parse a single imperative statement into its [`ImperativeStmt`] variant.
fn parse_imperative_stmt(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<ImperativeStmt> {
    Ok(match pair.as_rule() {
        Rule::break_stmt => {
            let span = pair.extract_span();
            let target = pair
                .into_inner()
                .next()
                .map(|p| CompactString::from(p.as_str()));
            ImperativeStmt::Break { target, span }
        }
        Rule::continue_stmt => {
            let span = pair.extract_span();
            let target = pair
                .into_inner()
                .next()
                .map(|p| CompactString::from(p.as_str()));
            ImperativeStmt::Continue { target, span }
        }
        Rule::return_stmt => parse_return_stmt(pair, param_pool, fixed_rules, cur_vld)?,
        Rule::if_chain | Rule::if_not_chain => {
            parse_if_stmt(pair, param_pool, fixed_rules, cur_vld)?
        }
        Rule::loop_block => parse_loop_stmt(pair, param_pool, fixed_rules, cur_vld)?,
        Rule::temp_swap => parse_temp_swap(pair)?,
        Rule::debug_stmt => {
            let name_p = pair.into_inner().next().ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "expected temp relation name in debug statement".to_string(),
                }
                .build()
            })?;
            ImperativeStmt::TempDebug {
                temp: CompactString::from(name_p.as_str()),
            }
        }
        Rule::imperative_sysop => {
            let mut src = pair.into_inner();
            let sysop = parse_sys(
                src.next()
                    .ok_or_else(|| {
                        InvalidQuerySnafu {
                            message: "expected system operation".to_string(),
                        }
                        .build()
                    })?
                    .into_inner(),
                param_pool,
                fixed_rules,
                cur_vld,
            )?;
            let store_as = src.next().map(|p| CompactString::from(p.as_str().trim()));
            ImperativeStmt::SysOp {
                sysop: ImperativeSysop { sysop, store_as },
            }
        }
        Rule::imperative_clause => {
            let prog =
                parse_query_with_store_as(pair, param_pool, fixed_rules, cur_vld)?;
            ImperativeStmt::Program { prog }
        }
        Rule::ignore_error_script => {
            let inner_pair = pair.into_inner().next().ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "expected query program in ignore_error block".to_string(),
                }
                .build()
            })?;
            let prog =
                parse_query_with_store_as(inner_pair, param_pool, fixed_rules, cur_vld)?;
            ImperativeStmt::IgnoreErrorProgram { prog }
        }
        r => {
            return Err(InvalidQuerySnafu {
                message: format!("unexpected rule {:?} in imperative parser", r),
            }
            .build()
            .into());
        }
    })
}

/// Parse a `%return` statement with its list of return values.
fn parse_return_stmt(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<ImperativeStmt> {
    let mut rets = vec![];
    for p in pair.into_inner() {
        match p.as_rule() {
            Rule::ident | Rule::underscore_ident => {
                rets.push(Right(CompactString::from(p.as_str())));
            }
            Rule::query_script_inner => {
                let prog =
                    parse_query_with_store_as(p, param_pool, fixed_rules, cur_vld)?;
                rets.push(Left(prog));
            }
            r => {
                return Err(InvalidQuerySnafu {
                    message: format!("unexpected rule {:?} in return statement", r),
                }
                .build()
                .into());
            }
        }
    }
    Ok(ImperativeStmt::Return { returns: rets })
}

/// Parse a `%if` or `%if_not` conditional statement.
fn parse_if_stmt(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<ImperativeStmt> {
    let negated = pair.as_rule() == Rule::if_not_chain;
    let mut inner = pair.into_inner();
    let condition = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected condition in if statement".to_string(),
        }
        .build()
    })?;
    let cond = match condition.as_rule() {
        Rule::underscore_ident => Left(CompactString::from(condition.as_str())),
        Rule::imperative_clause => {
            let prog = parse_query_with_store_as(
                condition, param_pool, fixed_rules, cur_vld,
            )?;
            Right(prog)
        }
        r => {
            return Err(InvalidQuerySnafu {
                message: format!("unexpected rule {:?} in if condition", r),
            }
            .build()
            .into());
        }
    };
    let body = inner
        .next()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected then-branch body".to_string(),
            }
            .build()
        })?
        .into_inner()
        .map(|p| parse_imperative_stmt(p, param_pool, fixed_rules, cur_vld))
        .try_collect()?;
    let else_body = match inner.next() {
        None => vec![],
        Some(rest) => rest
            .into_inner()
            .map(|p| parse_imperative_stmt(p, param_pool, fixed_rules, cur_vld))
            .try_collect()?,
    };
    Ok(ImperativeStmt::If {
        condition: cond,
        then_branch: body,
        else_branch: else_body,
        negated,
    })
}

/// Parse a `%loop [label] { body }` statement.
fn parse_loop_stmt(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<ImperativeStmt> {
    let mut inner = pair.into_inner();
    let mut label = None;
    let mut nxt = inner.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected loop body".to_string(),
        }
        .build()
    })?;
    if nxt.as_rule() == Rule::ident {
        label = Some(CompactString::from(nxt.as_str()));
        nxt = inner.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "expected loop body after label".to_string(),
            }
            .build()
        })?;
    }
    let body = parse_imperative_block(nxt, param_pool, fixed_rules, cur_vld)?;
    Ok(ImperativeStmt::Loop { label, body })
}

/// Parse a `%swap <left> <right>` statement.
fn parse_temp_swap(pair: Pair<'_>) -> Result<ImperativeStmt> {
    let mut pairs = pair.into_inner();
    let left = pairs.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected left relation name in swap".to_string(),
        }
        .build()
    })?;
    let right = pairs.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "expected right relation name in swap".to_string(),
        }
        .build()
    })?;
    Ok(ImperativeStmt::TempSwap {
        left: CompactString::from(left.as_str()),
        right: CompactString::from(right.as_str()),
    })
}
