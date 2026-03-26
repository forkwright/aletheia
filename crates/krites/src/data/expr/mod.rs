//! Expression evaluation and representation.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]

use std::fmt::Debug;

use crate::data::error::*;
use crate::error::{InternalError, InternalResult as Result};

use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::parse::SourceSpan;

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Debug)]
#[non_exhaustive]
pub enum Bytecode {
    /// push 1
    Binding {
        var: Symbol,
        tuple_pos: Option<usize>,
    },
    /// push 1
    Const {
        val: DataValue,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// pop n, push 1
    Apply {
        op: &'static Op,
        arity: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// pop 1
    JumpIfFalse {
        jump_to: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// unchanged
    Goto {
        jump_to: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
}

pub(crate) fn unbound_variable_err(name: &str) -> DataError {
    UnboundVariableSnafu {
        message: format!("The variable '{name}' is unbound"),
    }
    .build()
}

pub(crate) fn tuple_too_short_err(name: &str, index: usize, length: usize) -> DataError {
    InvalidValueSnafu {
        message: format!(
            "The tuple bound by variable '{name}' is too short: index is {index}, length is {length}"
        ),
    }
    .build()
}

pub fn eval_bytecode_pred(
    bytecodes: &[Bytecode],
    bindings: impl AsRef<[DataValue]>,
    stack: &mut Vec<DataValue>,
    _span: SourceSpan,
) -> Result<bool> {
    match eval_bytecode(bytecodes, bindings, stack)? {
        DataValue::Bool(b) => Ok(b),
        v => Err(TypeMismatchSnafu {
            op: "predicate evaluation".to_string(),
            expected: format!("a boolean value, got {:?}", v),
        }
        .build()
        .into()),
    }
}

pub fn eval_bytecode(
    bytecodes: &[Bytecode],
    bindings: impl AsRef<[DataValue]>,
    stack: &mut Vec<DataValue>,
) -> Result<DataValue> {
    stack.clear();
    let mut pointer = 0;
    loop {
        if pointer == bytecodes.len() {
            break;
        }
        #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
        let current_instruction = &bytecodes[pointer];
        match current_instruction {
            Bytecode::Binding { var, tuple_pos, .. } => match tuple_pos {
                None => {
                    return Err(unbound_variable_err(&var.name).into());
                }
                Some(i) => {
                    let val = bindings
                        .as_ref()
                        .get(*i)
                        .ok_or_else(|| {
                            InternalError::from(tuple_too_short_err(
                                &var.name,
                                *i,
                                bindings.as_ref().len(),
                            ))
                        })?
                        .clone();
                    stack.push(val);
                    pointer += 1;
                }
            },
            Bytecode::Const { val, .. } => {
                stack.push(val.clone());
                pointer += 1;
            }
            Bytecode::Apply { op, arity, span: _ } => {
                let frame_start = stack.len() - *arity;
                let args_frame = &stack[frame_start..];
                let result = (op.inner)(args_frame)?;
                stack.truncate(frame_start);
                stack.push(result);
                pointer += 1;
            }
            Bytecode::JumpIfFalse { jump_to, span: _ } => {
                let val = stack.pop().unwrap_or(DataValue::Null);
                let cond = val.get_bool().ok_or_else(|| {
                    InternalError::from(
                        TypeMismatchSnafu {
                            op: "predicate evaluation".to_string(),
                            expected: format!("a boolean value, got {:?}", val),
                        }
                        .build(),
                    )
                })?;
                if cond {
                    pointer += 1;
                } else {
                    pointer = *jump_to;
                }
            }
            Bytecode::Goto { jump_to, .. } => {
                pointer = *jump_to;
            }
        }
    }
    Ok(stack.pop().unwrap_or(DataValue::Null))
}

mod expr_impl;
mod op;

pub(crate) use expr_impl::{Expr, compute_bounds, no_implementation_err};
pub(crate) use op::{Op, ValueRange, get_op};
