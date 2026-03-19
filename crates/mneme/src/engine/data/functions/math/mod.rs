//! Comparison, arithmetic, and basic math functions.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]

use super::arg;
use crate::engine::data::error::*;
type Result<T> = DataResult<T>;
use crate::engine::data::value::{DataValue, Num, Vector};

pub(crate) fn ensure_same_value_type(a: &DataValue, b: &DataValue) -> Result<()> {
    use DataValue::*;
    if !matches!(
        (a, b),
        (Null, Null)
            | (Bool(_), Bool(_))
            | (Num(_), Num(_))
            | (Str(_), Str(_))
            | (Bytes(_), Bytes(_))
            | (Regex(_), Regex(_))
            | (List(_), List(_))
            | (Set(_), Set(_))
            | (Bot, Bot)
    ) {
        return ComparisonTypeMismatchSnafu {
            left: format!("{a:?}"),
            right: format!("{b:?}"),
        }
        .fail();
    }
    Ok(())
}

pub(crate) fn add_vecs(args: &[DataValue]) -> Result<DataValue> {
    if args.len() == 1 {
        return Ok(arg(args, 0)?.clone());
    }
    let (last, first) = args
        .split_last()
        .expect("args is non-empty, len==1 case returned early");
    let first = add_vecs(first)?;
    match (first, last) {
        (DataValue::Vec(a), DataValue::Vec(b)) => {
            if a.len() != b.len() {
                return VectorLengthMismatchSnafu { op: "add" }.fail();
            }
            match (a, b) {
                (Vector::F32(a), Vector::F32(b)) => Ok(DataValue::Vec(Vector::F32(a + b))),
                (Vector::F64(a), Vector::F64(b)) => Ok(DataValue::Vec(Vector::F64(a + b))),
                (Vector::F32(a), Vector::F64(b)) => {
                    let a = a.mapv(f64::from);
                    Ok(DataValue::Vec(Vector::F64(a + b)))
                }
                (Vector::F64(a), Vector::F32(b)) => {
                    let b = b.mapv(f64::from);
                    Ok(DataValue::Vec(Vector::F64(a + b)))
                }
            }
        }
        (DataValue::Vec(a), b) => {
            let f = b.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "add",
                    expected: "numbers to add to vectors",
                }
                .build()
            })?;
            match a {
                Vector::F32(mut v) => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
                    )]
                    let f = f as f32;
                    v += f;
                    Ok(DataValue::Vec(Vector::F32(v)))
                }
                Vector::F64(mut v) => {
                    v += f;
                    Ok(DataValue::Vec(Vector::F64(v)))
                }
            }
        }
        (a, DataValue::Vec(b)) => {
            let f = a.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "add",
                    expected: "numbers to add to vectors",
                }
                .build()
            })?;
            match b {
                Vector::F32(v) => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
                    )]
                    let f = f as f32;
                    Ok(DataValue::Vec(Vector::F32(v + f)))
                }
                Vector::F64(v) => Ok(DataValue::Vec(Vector::F64(v + f))),
            }
        }
        _ => TypeMismatchSnafu {
            op: "add",
            expected: "numbers",
        }
        .fail(),
    }
}

pub(crate) fn mul_vecs(args: &[DataValue]) -> Result<DataValue> {
    if args.len() == 1 {
        return Ok(arg(args, 0)?.clone());
    }
    let (last, first) = args
        .split_last()
        .expect("args is non-empty, len==1 case returned early");
    let first = add_vecs(first)?;
    match (first, last) {
        (DataValue::Vec(a), DataValue::Vec(b)) => {
            if a.len() != b.len() {
                return VectorLengthMismatchSnafu { op: "add" }.fail();
            }
            match (a, b) {
                (Vector::F32(a), Vector::F32(b)) => Ok(DataValue::Vec(Vector::F32(a * b))),
                (Vector::F64(a), Vector::F64(b)) => Ok(DataValue::Vec(Vector::F64(a * b))),
                (Vector::F32(a), Vector::F64(b)) => {
                    let a = a.mapv(f64::from);
                    Ok(DataValue::Vec(Vector::F64(a * b)))
                }
                (Vector::F64(a), Vector::F32(b)) => {
                    let b = b.mapv(f64::from);
                    Ok(DataValue::Vec(Vector::F64(a * b)))
                }
            }
        }
        (DataValue::Vec(a), b) => {
            let f = b.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "add",
                    expected: "numbers to add to vectors",
                }
                .build()
            })?;
            match a {
                Vector::F32(mut v) => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
                    )]
                    let f = f as f32;
                    v *= f;
                    Ok(DataValue::Vec(Vector::F32(v)))
                }
                Vector::F64(mut v) => {
                    v *= f;
                    Ok(DataValue::Vec(Vector::F64(v)))
                }
            }
        }
        (a, DataValue::Vec(b)) => {
            let f = a.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "add",
                    expected: "numbers to add to vectors",
                }
                .build()
            })?;
            match b {
                Vector::F32(v) => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
                    )]
                    let f = f as f32;
                    Ok(DataValue::Vec(Vector::F32(v * f)))
                }
                Vector::F64(v) => Ok(DataValue::Vec(Vector::F64(v * f))),
            }
        }
        _ => TypeMismatchSnafu {
            op: "add",
            expected: "numbers",
        }
        .fail(),
    }
}

pub(crate) fn op_eq(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Num(Num::Float(f)), DataValue::Num(Num::Int(i)))
        | (DataValue::Num(Num::Int(i)), DataValue::Num(Num::Float(f))) => *i as f64 == *f,
        (a, b) => a == b,
    }))
}

pub(crate) fn op_neq(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Num(Num::Float(f)), DataValue::Num(Num::Int(i)))
        | (DataValue::Num(Num::Int(i)), DataValue::Num(Num::Float(f))) => *i as f64 != *f,
        (a, b) => a != b,
    }))
}

pub(crate) fn op_gt(args: &[DataValue]) -> Result<DataValue> {
    ensure_same_value_type(arg(args, 0)?, arg(args, 1)?)?;
    Ok(DataValue::from(match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Num(Num::Float(l)), DataValue::Num(Num::Int(r))) => *l > *r as f64,
        (DataValue::Num(Num::Int(l)), DataValue::Num(Num::Float(r))) => *l as f64 > *r,
        (a, b) => a > b,
    }))
}

pub(crate) fn op_ge(args: &[DataValue]) -> Result<DataValue> {
    ensure_same_value_type(arg(args, 0)?, arg(args, 1)?)?;
    Ok(DataValue::from(match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Num(Num::Float(l)), DataValue::Num(Num::Int(r))) => *l >= *r as f64,
        (DataValue::Num(Num::Int(l)), DataValue::Num(Num::Float(r))) => *l as f64 >= *r,
        (a, b) => a >= b,
    }))
}

pub(crate) fn op_lt(args: &[DataValue]) -> Result<DataValue> {
    ensure_same_value_type(arg(args, 0)?, arg(args, 1)?)?;
    Ok(DataValue::from(match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Num(Num::Float(l)), DataValue::Num(Num::Int(r))) => *l < (*r as f64),
        (DataValue::Num(Num::Int(l)), DataValue::Num(Num::Float(r))) => (*l as f64) < *r,
        (a, b) => a < b,
    }))
}

pub(crate) fn op_le(args: &[DataValue]) -> Result<DataValue> {
    ensure_same_value_type(arg(args, 0)?, arg(args, 1)?)?;
    Ok(DataValue::from(match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Num(Num::Float(l)), DataValue::Num(Num::Int(r))) => *l <= (*r as f64),
        (DataValue::Num(Num::Int(l)), DataValue::Num(Num::Float(r))) => (*l as f64) <= *r,
        (a, b) => a <= b,
    }))
}

pub(crate) fn op_add(args: &[DataValue]) -> Result<DataValue> {
    let mut i_accum = 0i64;
    let mut f_accum = 0.0f64;
    for arg in args {
        match arg {
            DataValue::Num(Num::Int(i)) => i_accum += i,
            DataValue::Num(Num::Float(f)) => f_accum += f,
            DataValue::Vec(_) => return add_vecs(args),
            _ => {
                return TypeMismatchSnafu {
                    op: "add",
                    expected: "numbers",
                }
                .fail();
            }
        }
    }
    if f_accum == 0.0f64 {
        Ok(DataValue::Num(Num::Int(i_accum)))
    } else {
        Ok(DataValue::Num(Num::Float(i_accum as f64 + f_accum)))
    }
}

pub(crate) fn op_max(args: &[DataValue]) -> Result<DataValue> {
    let res = args
        .iter()
        .try_fold(None, |accum, nxt| match (accum, nxt) {
            (None, d @ DataValue::Num(_)) => Ok(Some(d.clone())),
            (Some(DataValue::Num(a)), DataValue::Num(b)) => Ok(Some(DataValue::Num(a.max(*b)))),
            _ => TypeMismatchSnafu {
                op: "max",
                expected: "numbers",
            }
            .fail(),
        })?;
    match res {
        None => Ok(DataValue::Num(Num::Float(f64::NEG_INFINITY))),
        Some(v) => Ok(v),
    }
}

pub(crate) fn op_min(args: &[DataValue]) -> Result<DataValue> {
    let res = args
        .iter()
        .try_fold(None, |accum, nxt| match (accum, nxt) {
            (None, d @ DataValue::Num(_)) => Ok(Some(d.clone())),
            (Some(DataValue::Num(a)), DataValue::Num(b)) => Ok(Some(DataValue::Num(a.min(*b)))),
            _ => TypeMismatchSnafu {
                op: "min",
                expected: "numbers",
            }
            .fail(),
        })?;
    match res {
        None => Ok(DataValue::Num(Num::Float(f64::INFINITY))),
        Some(v) => Ok(v),
    }
}

mod arithmetic;
mod transcendental;

pub(crate) use arithmetic::*;
pub(crate) use transcendental::*;
