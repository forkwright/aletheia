//! Expression evaluation for krites v2.
//!
//! Evaluates AST expressions to values given variable bindings and parameters.

use std::collections::{BTreeMap, HashMap};

use crate::v2::error::{self, Result};
use crate::v2::parse::ast::{BinOp, Expr, UnaryOp};
use crate::v2::value::Value;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Evaluate an expression to a value.
///
/// # Arguments
///
/// * `expr` - The expression to evaluate.
/// * `bindings` - Variable bindings from the current tuple (var name → value).
/// * `params` - Runtime parameters ($name → value).
///
/// # Errors
///
/// Returns an error if evaluation fails (undefined variable, type mismatch).
pub fn eval_expr(
    expr: &Expr,
    bindings: &HashMap<String, Value>,
    params: &BTreeMap<String, Value>,
) -> Result<Value> {
    match expr {
        Expr::Var(name) => bindings
            .get(name)
            .cloned()
            .ok_or_else(|| {
                error::EvalSnafu {
                    message: format!("undefined variable: {name}"),
                }
                .build()
            }),
        Expr::Param(name) => params
            .get(name)
            .cloned()
            .ok_or_else(|| {
                error::EvalSnafu {
                    message: format!("undefined parameter: ${name}"),
                }
                .build()
            }),
        Expr::Literal(value) => Ok(value.clone()),
        Expr::FnCall { name, args } => {
            let arg_values: Vec<Value> = args
                .iter()
                .map(|arg| eval_expr(arg, bindings, params))
                .collect::<Result<Vec<_>>>()?;
            eval_function(name, &arg_values)
        }
        Expr::BinOp { op, left, right } => {
            let left_val = eval_expr(left, bindings, params)?;
            let right_val = eval_expr(right, bindings, params)?;
            eval_binary_op(*op, &left_val, &right_val)
        }
        Expr::UnaryOp { op, operand } => {
            let val = eval_expr(operand, bindings, params)?;
            eval_unary_op(*op, &val)
        }
    }
}

// ---------------------------------------------------------------------------
// Binary operations
// ---------------------------------------------------------------------------

fn eval_binary_op(op: BinOp, left: &Value, right: &Value) -> Result<Value> {
    match op {
        // Comparison
        BinOp::Eq => Ok(Value::Bool(left == right)),
        BinOp::Neq => Ok(Value::Bool(left != right)),
        BinOp::Lt => Ok(Value::Bool(left < right)),
        BinOp::Gt => Ok(Value::Bool(left > right)),
        BinOp::Lte => Ok(Value::Bool(left <= right)),
        BinOp::Gte => Ok(Value::Bool(left >= right)),

        // Logical
        BinOp::And => {
            let l = left.as_bool().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!("expected bool in 'and', got {}", left.type_name()),
                }
                .build()
            })?;
            let r = right.as_bool().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!("expected bool in 'and', got {}", right.type_name()),
                }
                .build()
            })?;
            Ok(Value::Bool(l && r))
        }
        BinOp::Or => {
            let l = left.as_bool().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!("expected bool in 'or', got {}", left.type_name()),
                }
                .build()
            })?;
            let r = right.as_bool().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!("expected bool in 'or', got {}", right.type_name()),
                }
                .build()
            })?;
            Ok(Value::Bool(l || r))
        }

        // Arithmetic
        BinOp::Add => eval_add(left, right),
        BinOp::Sub => eval_sub(left, right),
        BinOp::Mul => eval_mul(left, right),
        BinOp::Div => eval_div(left, right),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "i64 to f64: precision loss acceptable for expression evaluation mixed-type arithmetic"
)]
fn eval_add(left: &Value, right: &Value) -> Result<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
        _ => Err(error::EvalSnafu {
            message: format!(
                "cannot add {} and {}",
                left.type_name(),
                right.type_name()
            ),
        }
        .build()),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "i64 to f64: precision loss acceptable for expression evaluation mixed-type arithmetic"
)]
fn eval_sub(left: &Value, right: &Value) -> Result<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
        _ => Err(error::EvalSnafu {
            message: format!(
                "cannot subtract {} from {}",
                right.type_name(),
                left.type_name()
            ),
        }
        .build()),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "i64 to f64: precision loss acceptable for expression evaluation mixed-type arithmetic"
)]
fn eval_mul(left: &Value, right: &Value) -> Result<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
        _ => Err(error::EvalSnafu {
            message: format!(
                "cannot multiply {} and {}",
                left.type_name(),
                right.type_name()
            ),
        }
        .build()),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "i64 to f64: precision loss acceptable for expression evaluation mixed-type arithmetic"
)]
fn eval_div(left: &Value, right: &Value) -> Result<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                return Err(error::EvalSnafu {
                    message: "division by zero".to_string(),
                }
                .build());
            }
            Ok(Value::Int(a / b))
        }
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
        (Value::Float(a), Value::Int(b)) => {
            if *b == 0 {
                return Err(error::EvalSnafu {
                    message: "division by zero".to_string(),
                }
                .build());
            }
            Ok(Value::Float(a / *b as f64))
        }
        _ => Err(error::EvalSnafu {
            message: format!(
                "cannot divide {} by {}",
                left.type_name(),
                right.type_name()
            ),
        }
        .build()),
    }
}

// ---------------------------------------------------------------------------
// Unary operations
// ---------------------------------------------------------------------------

fn eval_unary_op(op: UnaryOp, operand: &Value) -> Result<Value> {
    match op {
        UnaryOp::Not => {
            let b = operand.as_bool().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!("expected bool for 'not', got {}", operand.type_name()),
                }
                .build()
            })?;
            Ok(Value::Bool(!b))
        }
        UnaryOp::Neg => match operand {
            Value::Int(n) => Ok(Value::Int(-n)),
            Value::Float(f) => Ok(Value::Float(-f)),
            _ => Err(error::EvalSnafu {
                message: format!(
                    "cannot negate {} (expected numeric)",
                    operand.type_name()
                ),
            }
            .build()),
        },
    }
}

// ---------------------------------------------------------------------------
// Function calls
// ---------------------------------------------------------------------------

fn eval_function(name: &str, args: &[Value]) -> Result<Value> {
    match name {
        "contains" => {
            if args.len() != 2 {
                return Err(error::EvalSnafu {
                    message: format!("contains() expects 2 arguments, got {}", args.len()),
                }
                .build());
            }
            let haystack = args[0].as_str().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!(
                        "contains() first argument must be string, got {}",
                        args[0].type_name()
                    ),
                }
                .build()
            })?;
            let needle = args[1].as_str().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!(
                        "contains() second argument must be string, got {}",
                        args[1].type_name()
                    ),
                }
                .build()
            })?;
            Ok(Value::Bool(haystack.contains(needle)))
        }
        "starts_with" => {
            if args.len() != 2 {
                return Err(error::EvalSnafu {
                    message: format!(
                        "starts_with() expects 2 arguments, got {}",
                        args.len()
                    ),
                }
                .build());
            }
            let s = args[0].as_str().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!(
                        "starts_with() first argument must be string, got {}",
                        args[0].type_name()
                    ),
                }
                .build()
            })?;
            let prefix = args[1].as_str().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!(
                        "starts_with() second argument must be string, got {}",
                        args[1].type_name()
                    ),
                }
                .build()
            })?;
            Ok(Value::Bool(s.starts_with(prefix)))
        }
        "str_includes" => {
            if args.len() != 2 {
                return Err(error::EvalSnafu {
                    message: format!(
                        "str_includes() expects 2 arguments, got {}",
                        args.len()
                    ),
                }
                .build());
            }
            let haystack = args[0].as_str().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!(
                        "str_includes() first argument must be string, got {}",
                        args[0].type_name()
                    ),
                }
                .build()
            })?;
            let needle = args[1].as_str().ok_or_else(|| {
                error::EvalSnafu {
                    message: format!(
                        "str_includes() second argument must be string, got {}",
                        args[1].type_name()
                    ),
                }
                .build()
            })?;
            Ok(Value::Bool(haystack.contains(needle)))
        }
        _ => Err(error::EvalSnafu {
            message: format!("unknown function: {name}"),
        }
        .build()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn eval_literal() {
        let bindings = HashMap::new();
        let params = BTreeMap::new();
        let expr = Expr::Literal(Value::from(42_i64));
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::from(42_i64));
    }

    #[test]
    fn eval_variable() {
        let mut bindings = HashMap::new();
        bindings.insert("x".to_string(), Value::from("hello"));
        let params = BTreeMap::new();
        let expr = Expr::Var("x".to_string());
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::from("hello"));
    }

    #[test]
    fn eval_parameter() {
        let bindings = HashMap::new();
        let mut params = BTreeMap::new();
        params.insert("id".to_string(), Value::from("abc"));
        let expr = Expr::Param("id".to_string());
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::from("abc"));
    }

    #[test]
    fn eval_comparison() {
        let bindings = HashMap::new();
        let params = BTreeMap::new();
        
        // 5 > 3
        let expr = Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Literal(Value::from(5_i64))),
            right: Box::new(Expr::Literal(Value::from(3_i64))),
        };
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::Bool(true));

        // 5 == 3
        let expr = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Literal(Value::from(5_i64))),
            right: Box::new(Expr::Literal(Value::from(3_i64))),
        };
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::Bool(false));
    }

    #[test]
    fn eval_arithmetic() {
        let bindings = HashMap::new();
        let params = BTreeMap::new();
        
        // 5 + 3 * 2 = 11 (if properly left-to-right, but we don't parse that here)
        let expr = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(Expr::Literal(Value::from(5_i64))),
            right: Box::new(Expr::Literal(Value::from(3_i64))),
        };
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::from(8_i64));
    }

    #[test]
    fn eval_logical() {
        let bindings = HashMap::new();
        let params = BTreeMap::new();
        
        // true && false
        let expr = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::Literal(Value::Bool(true))),
            right: Box::new(Expr::Literal(Value::Bool(false))),
        };
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::Bool(false));
    }

    #[test]
    fn eval_contains() {
        let bindings = HashMap::new();
        let params = BTreeMap::new();
        
        let expr = Expr::FnCall {
            name: "contains".to_string(),
            args: vec![
                Expr::Literal(Value::from("hello world")),
                Expr::Literal(Value::from("world")),
            ],
        };
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::Bool(true));

        let expr = Expr::FnCall {
            name: "contains".to_string(),
            args: vec![
                Expr::Literal(Value::from("hello world")),
                Expr::Literal(Value::from("foo")),
            ],
        };
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::Bool(false));
    }

    #[test]
    fn eval_starts_with() {
        let bindings = HashMap::new();
        let params = BTreeMap::new();
        
        let expr = Expr::FnCall {
            name: "starts_with".to_string(),
            args: vec![
                Expr::Literal(Value::from("hello world")),
                Expr::Literal(Value::from("hello")),
            ],
        };
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::Bool(true));
    }

    #[test]
    fn eval_unary_not() {
        let bindings = HashMap::new();
        let params = BTreeMap::new();
        
        let expr = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::Literal(Value::Bool(true))),
        };
        assert_eq!(eval_expr(&expr, &bindings, &params).unwrap(), Value::Bool(false));
    }
}
