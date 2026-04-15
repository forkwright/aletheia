//! Expression parsing from Datalog source.
//!
//! Converts pest expression pairs into typed [`Expr`] AST nodes using Pratt
//! precedence climbing. Also provides [`expr2bytecode`] for compiling
//! expressions to the bytecode VM representation.
//!
//! String literal parsing is delegated to the [`strings`] submodule.
#![expect(
    clippy::indexing_slicing,
    clippy::needless_return,
    clippy::pedantic,
    clippy::result_large_err,
    reason = "Datalog expression parser — indexing into pest pairs by structural position, InternalError is the crate-wide Result type"
)]

mod bytecode;
mod strings;

pub(crate) use bytecode::expr2bytecode;
pub(crate) use strings::parse_string;

use std::collections::BTreeMap;
use std::sync::LazyLock;

use itertools::Itertools;
use pest::pratt_parser::{Op, PrattParser};

use crate::data::expr::{Expr, get_op};
use crate::data::functions::{
    OP_ADD, OP_AND, OP_COALESCE, OP_CONCAT, OP_DIV, OP_EQ, OP_GE, OP_GT, OP_JSON_OBJECT, OP_LE,
    OP_LIST, OP_LT, OP_MAYBE_GET, OP_MINUS, OP_MOD, OP_MUL, OP_NEGATE, OP_NEQ, OP_OR, OP_POW,
    OP_SUB,
};
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::parse::error::InvalidQuerySnafu;
use crate::parse::{ExtractSpan, Pair, Rule, SourceSpan};

/// Pratt parser defining operator precedence and associativity for Datalog
/// expressions. Lowest precedence at the top, highest at the bottom.
static PRATT_PARSER: LazyLock<PrattParser<Rule>> = LazyLock::new(|| {
    use pest::pratt_parser::Assoc::*;

    PrattParser::new()
        .op(Op::infix(Rule::op_or, Left))
        .op(Op::infix(Rule::op_and, Left))
        .op(Op::infix(Rule::op_gt, Left)
            | Op::infix(Rule::op_lt, Left)
            | Op::infix(Rule::op_ge, Left)
            | Op::infix(Rule::op_le, Left))
        .op(Op::infix(Rule::op_eq, Left) | Op::infix(Rule::op_ne, Left))
        .op(Op::infix(Rule::op_mod, Left))
        .op(Op::infix(Rule::op_add, Left)
            | Op::infix(Rule::op_sub, Left)
            | Op::infix(Rule::op_concat, Left))
        .op(Op::infix(Rule::op_mul, Left) | Op::infix(Rule::op_div, Left))
        .op(Op::infix(Rule::op_pow, Right))
        .op(Op::infix(Rule::op_coalesce, Left))
        .op(Op::prefix(Rule::minus))
        .op(Op::prefix(Rule::negate))
        .op(Op::infix(Rule::op_field_access, Left))
});

/// Build a typed [`Expr`] from a pest expression pair using Pratt parsing.
///
/// The pair must have rule `Rule::expr`. Delegates to the Pratt parser which
/// calls [`build_term`] for primaries and [`build_expr_infix`] for operators.
///
/// # Errors
///
/// Returns an error if the expression contains invalid syntax, missing
/// parameters, or unknown function references.
pub(crate) fn build_expr(pair: Pair<'_>, param_pool: &BTreeMap<String, DataValue>) -> Result<Expr> {
    if pair.as_rule() != Rule::expr {
        return Err(InvalidQuerySnafu {
            message: "Invalid expression encountered".to_string(),
        }
        .build()
        .into());
    }

    PRATT_PARSER
        .map_primary(|v| build_term(v, param_pool))
        .map_infix(build_expr_infix)
        .map_prefix(|op, rhs| build_expr_prefix(op, rhs))
        .parse(pair.into_inner())
}

/// Build a prefix (unary) expression from the operator pair and parsed operand.
fn build_expr_prefix(op: Pair<'_>, rhs: Result<Expr>) -> Result<Expr> {
    let rhs = rhs?;
    let rhs_span = rhs.span();
    Ok(match op.as_rule() {
        Rule::minus => Expr::Apply {
            op: &OP_MINUS,
            args: [rhs].into(),
            span: op.extract_span().merge(rhs_span),
        },
        Rule::negate => Expr::Apply {
            op: &OP_NEGATE,
            args: [rhs].into(),
            span: op.extract_span().merge(rhs_span),
        },
        r => {
            return Err(InvalidQuerySnafu {
                message: format!(
                    "unexpected rule {:?} in parser - grammar mismatch, please file a bug",
                    r
                ),
            }
            .build()
            .into());
        }
    })
}

/// Resolve a binary infix operator to its function implementation and
/// construct the resulting [`Expr::Apply`] node.
fn build_expr_infix(lhs: Result<Expr>, op: Pair<'_>, rhs: Result<Expr>) -> Result<Expr> {
    let args = vec![lhs?, rhs?];
    let op = match op.as_rule() {
        Rule::op_add => &OP_ADD,
        Rule::op_sub => &OP_SUB,
        Rule::op_mul => &OP_MUL,
        Rule::op_div => &OP_DIV,
        Rule::op_mod => &OP_MOD,
        Rule::op_pow => &OP_POW,
        Rule::op_eq => &OP_EQ,
        Rule::op_ne => &OP_NEQ,
        Rule::op_gt => &OP_GT,
        Rule::op_ge => &OP_GE,
        Rule::op_lt => &OP_LT,
        Rule::op_le => &OP_LE,
        Rule::op_concat => &OP_CONCAT,
        Rule::op_or => &OP_OR,
        Rule::op_and => &OP_AND,
        Rule::op_coalesce => &OP_COALESCE,
        Rule::op_field_access => &OP_MAYBE_GET,
        r => {
            return Err(InvalidQuerySnafu {
                message: format!(
                    "unexpected rule {:?} in parser - grammar mismatch, please file a bug",
                    r
                ),
            }
            .build()
            .into());
        }
    };
    #[expect(clippy::indexing_slicing, reason = "index bounds validated — vec has exactly 2 elements")]
    let start = args[0].span().0;
    #[expect(clippy::indexing_slicing, reason = "index bounds validated — vec has exactly 2 elements")]
    let end = args[1].span().0 + args[1].span().1;
    let length = end - start;
    Ok(Expr::Apply {
        op,
        args: args.into(),
        span: SourceSpan(start, length),
    })
}

/// Build a primary (leaf) expression term from a single pest pair.
///
/// Handles variables, parameters, numeric/string/boolean/null literals,
/// list/object constructors, function applications, `cond`/`if` special
/// forms, and grouping parentheses.
fn build_term(pair: Pair<'_>, param_pool: &BTreeMap<String, DataValue>) -> Result<Expr> {
    let span = pair.extract_span();
    let grammar_rule = pair.as_rule();
    Ok(match grammar_rule {
        Rule::var => Expr::Binding {
            var: Symbol::new(pair.as_str(), pair.extract_span()),
            tuple_pos: None,
        },
        Rule::param => build_param_term(pair, param_pool, span)?,
        Rule::pos_int => build_decimal_int_term(pair, span)?,
        Rule::hex_pos_int => build_radix_int_term(pair.as_str(), 16, span)?,
        Rule::octo_pos_int => build_radix_int_term(pair.as_str(), 8, span)?,
        Rule::bin_pos_int => build_radix_int_term(pair.as_str(), 2, span)?,
        Rule::dot_float | Rule::sci_float => build_float_term(pair, span)?,
        Rule::null => Expr::Const {
            val: DataValue::Null,
            span,
        },
        Rule::boolean => Expr::Const {
            val: DataValue::from(pair.as_str() == "true"),
            span,
        },
        Rule::quoted_string | Rule::s_quoted_string | Rule::raw_string => {
            let s = parse_string(pair)?;
            Expr::Const {
                val: DataValue::Str(s),
                span,
            }
        }
        Rule::list => build_list_term(pair, param_pool, span)?,
        Rule::object => build_object_term(pair, param_pool, span)?,
        Rule::apply => build_apply_term(pair, param_pool, span)?,
        Rule::grouping => build_expr(
            pair.into_inner().next().ok_or_else(|| {
                InvalidQuerySnafu {
                    message: "empty grouping expression".to_string(),
                }
                .build()
            })?,
            param_pool,
        )?,
        r => {
            return Err(InvalidQuerySnafu {
                message: format!(
                    "unexpected rule {:?} in parser - grammar mismatch, please file a bug",
                    r
                ),
            }
            .build()
            .into());
        }
    })
}

/// Build a parameter reference (`$name`) term, looking up the value in the pool.
fn build_param_term(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    span: SourceSpan,
) -> Result<Expr> {
    let param_str = pair.as_str().strip_prefix('$').ok_or_else(|| {
        InvalidQuerySnafu {
            message: "parameter missing '$' prefix".to_string(),
        }
        .build()
    })?;
    Ok(Expr::Const {
        val: param_pool
            .get(param_str)
            .ok_or_else(|| {
                InvalidQuerySnafu {
                    message: format!("Required parameter {param_str} not found"),
                }
                .build()
            })?
            .clone(),
        span,
    })
}

/// Build a decimal integer literal term.
fn build_decimal_int_term(pair: Pair<'_>, span: SourceSpan) -> Result<Expr> {
    let i = pair.as_str().replace('_', "").parse::<i64>().map_err(|e| {
        InvalidQuerySnafu {
            message: format!("Cannot parse integer: {e}"),
        }
        .build()
    })?;
    Ok(Expr::Const {
        val: DataValue::from(i),
        span,
    })
}

/// Build a non-decimal integer literal term (hex, octal, binary).
fn build_radix_int_term(src: &str, radix: u32, span: SourceSpan) -> Result<Expr> {
    let i = parse_int(src, radix)?;
    Ok(Expr::Const {
        val: DataValue::from(i),
        span,
    })
}

/// Build a floating-point literal term.
fn build_float_term(pair: Pair<'_>, span: SourceSpan) -> Result<Expr> {
    let f = pair.as_str().replace('_', "").parse::<f64>().map_err(|e| {
        InvalidQuerySnafu {
            message: format!("Cannot parse float: {e}"),
        }
        .build()
    })?;
    Ok(Expr::Const {
        val: DataValue::from(f),
        span,
    })
}

/// Build a list constructor `[expr, ...]` term.
fn build_list_term(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    span: SourceSpan,
) -> Result<Expr> {
    let collected: Vec<_> = pair
        .into_inner()
        .map(|p| build_expr(p, param_pool))
        .try_collect()?;
    Ok(Expr::Apply {
        op: &OP_LIST,
        args: collected.into(),
        span,
    })
}

/// Build an object constructor `{key: value, ...}` term.
fn build_object_term(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    span: SourceSpan,
) -> Result<Expr> {
    let mut args = vec![];
    for p in pair.into_inner() {
        let mut p = p.into_inner();
        let k = p.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "missing key in object pair".to_string(),
            }
            .build()
        })?;
        let v = p.next().ok_or_else(|| {
            InvalidQuerySnafu {
                message: "missing value in object pair".to_string(),
            }
            .build()
        })?;
        args.push(build_expr(k, param_pool)?);
        args.push(build_expr(v, param_pool)?);
    }
    Ok(Expr::Apply {
        op: &OP_JSON_OBJECT,
        args: args.into(),
        span,
    })
}

/// Build a function application `name(args...)` term, including special
/// forms `cond(...)` and `if(...)`.
fn build_apply_term(
    pair: Pair<'_>,
    param_pool: &BTreeMap<String, DataValue>,
    span: SourceSpan,
) -> Result<Expr> {
    let mut p = pair.into_inner();
    let ident_p = p.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "missing identifier in function application".to_string(),
        }
        .build()
    })?;
    let ident = ident_p.as_str();
    let args: Vec<_> = p
        .next()
        .ok_or_else(|| {
            InvalidQuerySnafu {
                message: "missing arguments in function application".to_string(),
            }
            .build()
        })?
        .into_inner()
        .map(|v| build_expr(v, param_pool))
        .try_collect()?;

    match ident {
        "cond" => build_cond_expr(args, span),
        "if" => build_if_expr(args, span),
        _ => build_function_call(ident, args, span),
    }
}

/// Build a `cond(cond1, val1, cond2, val2, ..., default)` expression.
///
/// If the argument count is odd, a `null` condition is inserted before the
/// last value. A final `true -> null` fallback is appended unless the last
/// condition is already a boolean `true`.
fn build_cond_expr(mut args: Vec<Expr>, span: SourceSpan) -> Result<Expr> {
    if args.is_empty() {
        return Err(InvalidQuerySnafu {
            message: "'cond' cannot have empty body".to_string(),
        }
        .build()
        .into());
    }
    if args.len() & 1 == 1 {
        args.insert(
            args.len() - 1,
            Expr::Const {
                val: DataValue::Null,
                span: args
                    .last()
                    .ok_or_else(|| {
                        InvalidQuerySnafu {
                            message: "missing last argument for cond".to_string(),
                        }
                        .build()
                    })?
                    .span(),
            },
        )
    }
    let mut clauses = args
        .chunks(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect_vec();
    if let Some((cond, _)) = clauses.last() {
        match cond {
            // NOTE: last clause already has true default, no null fallback needed
            Expr::Const {
                val: DataValue::Bool(true),
                ..
            } => {}
            _ => {
                clauses.push((
                    Expr::Const {
                        val: DataValue::from(true),
                        span,
                    },
                    Expr::Const {
                        val: DataValue::Null,
                        span,
                    },
                ));
            }
        }
    }
    Ok(Expr::Cond { clauses, span })
}

/// Build an `if(condition, then_value[, else_value])` expression.
///
/// Desugars into a two-clause `Cond`: `[(condition, then), (true, else)]`.
/// The else branch defaults to `null` if omitted.
fn build_if_expr(args: Vec<Expr>, span: SourceSpan) -> Result<Expr> {
    if args.len() != 2 && args.len() != 3 {
        return Err(InvalidQuerySnafu {
            message: "wrong number of arguments to if: 2 or 3 required".to_string(),
        }
        .build()
        .into());
    }

    let mut clauses = vec![];
    let mut args = args.into_iter();
    let cond = args.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "missing condition in if expression".to_string(),
        }
        .build()
    })?;
    let then = args.next().ok_or_else(|| {
        InvalidQuerySnafu {
            message: "missing then branch in if expression".to_string(),
        }
        .build()
    })?;
    clauses.push((cond, then));
    let else_branch = args.next().unwrap_or(Expr::Const {
        val: DataValue::Null,
        span,
    });
    clauses.push((
        Expr::Const {
            val: DataValue::from(true),
            span,
        },
        else_branch,
    ));
    Ok(Expr::Cond { clauses, span })
}

/// Build a regular function call expression, resolving the function name
/// to a built-in operator and validating arity.
fn build_function_call(ident: &str, mut args: Vec<Expr>, span: SourceSpan) -> Result<Expr> {
    match get_op(ident) {
        None => Ok(Expr::UnboundApply {
            op: ident.into(),
            args: args.into(),
            span,
        }),
        Some(op) => {
            op.post_process_args(&mut args);

            if op.vararg {
                if op.min_arity > args.len() {
                    return Err(InvalidQuerySnafu {
                        message: format!(
                            "Wrong number of arguments for function: need at least {} argument(s)",
                            op.min_arity
                        ),
                    }
                    .build()
                    .into());
                }
            } else if op.min_arity != args.len() {
                return Err(InvalidQuerySnafu {
                    message: format!(
                        "Wrong number of arguments for function: need exactly {} argument(s)",
                        op.min_arity
                    ),
                }
                .build()
                .into());
            }
            Ok(Expr::Apply {
                op,
                args: args.into(),
                span,
            })
        }
    }
}

/// Parse a prefixed integer literal (`0x`, `0o`, `0b`) into an `i64`.
///
/// Skips the 2-character prefix, strips underscores, and parses with the
/// given radix. Returns an error instead of panicking on malformed input.
///
/// # Errors
///
/// Returns `InvalidQuery` if the prefix is missing or the digits are invalid
/// for the given radix.
pub(crate) fn parse_int(s: &str, radix: u32) -> Result<i64> {
    // WHY: get(2..) skips "0x"/"0o"/"0b" prefix; grammar guarantees valid integer format
    // but we propagate the error rather than panicking for robustness.
    let digits = s.get(2..).ok_or_else(|| {
        InvalidQuerySnafu {
            message: format!("integer literal too short: '{s}'"),
        }
        .build()
    })?;
    i64::from_str_radix(&digits.replace('_', ""), radix).map_err(|e| {
        InvalidQuerySnafu {
            message: format!("cannot parse integer '{s}': {e}"),
        }
        .build()
        .into()
    })
}
