//! Comparison, arithmetic, and basic math functions.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::ops::Rem;

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
        return Ok(args[0].clone());
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
                    let a = a.mapv(|x| x as f64);
                    Ok(DataValue::Vec(Vector::F64(a + b)))
                }
                (Vector::F64(a), Vector::F32(b)) => {
                    let b = b.mapv(|x| x as f64);
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
                    v += f as f32;
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
                Vector::F32(v) => Ok(DataValue::Vec(Vector::F32(v + f as f32))),
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
        return Ok(args[0].clone());
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
                    let a = a.mapv(|x| x as f64);
                    Ok(DataValue::Vec(Vector::F64(a * b)))
                }
                (Vector::F64(a), Vector::F32(b)) => {
                    let b = b.mapv(|x| x as f64);
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
                    v *= f as f32;
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
                Vector::F32(v) => Ok(DataValue::Vec(Vector::F32(v * f as f32))),
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
    Ok(DataValue::from(match (&args[0], &args[1]) {
        (DataValue::Num(Num::Float(f)), DataValue::Num(Num::Int(i)))
        | (DataValue::Num(Num::Int(i)), DataValue::Num(Num::Float(f))) => *i as f64 == *f,
        (a, b) => a == b,
    }))
}

pub(crate) fn op_neq(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match (&args[0], &args[1]) {
        (DataValue::Num(Num::Float(f)), DataValue::Num(Num::Int(i)))
        | (DataValue::Num(Num::Int(i)), DataValue::Num(Num::Float(f))) => *i as f64 != *f,
        (a, b) => a != b,
    }))
}

pub(crate) fn op_gt(args: &[DataValue]) -> Result<DataValue> {
    ensure_same_value_type(&args[0], &args[1])?;
    Ok(DataValue::from(match (&args[0], &args[1]) {
        (DataValue::Num(Num::Float(l)), DataValue::Num(Num::Int(r))) => *l > *r as f64,
        (DataValue::Num(Num::Int(l)), DataValue::Num(Num::Float(r))) => *l as f64 > *r,
        (a, b) => a > b,
    }))
}

pub(crate) fn op_ge(args: &[DataValue]) -> Result<DataValue> {
    ensure_same_value_type(&args[0], &args[1])?;
    Ok(DataValue::from(match (&args[0], &args[1]) {
        (DataValue::Num(Num::Float(l)), DataValue::Num(Num::Int(r))) => *l >= *r as f64,
        (DataValue::Num(Num::Int(l)), DataValue::Num(Num::Float(r))) => *l as f64 >= *r,
        (a, b) => a >= b,
    }))
}

pub(crate) fn op_lt(args: &[DataValue]) -> Result<DataValue> {
    ensure_same_value_type(&args[0], &args[1])?;
    Ok(DataValue::from(match (&args[0], &args[1]) {
        (DataValue::Num(Num::Float(l)), DataValue::Num(Num::Int(r))) => *l < (*r as f64),
        (DataValue::Num(Num::Int(l)), DataValue::Num(Num::Float(r))) => (*l as f64) < *r,
        (a, b) => a < b,
    }))
}

pub(crate) fn op_le(args: &[DataValue]) -> Result<DataValue> {
    ensure_same_value_type(&args[0], &args[1])?;
    Ok(DataValue::from(match (&args[0], &args[1]) {
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

pub(crate) fn op_sub(args: &[DataValue]) -> Result<DataValue> {
    Ok(match (&args[0], &args[1]) {
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Int(b))) => {
            DataValue::Num(Num::Int(*a - *b))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Float(b))) => {
            DataValue::Num(Num::Float(*a - *b))
        }
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Float(b))) => {
            DataValue::Num(Num::Float((*a as f64) - b))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Int(b))) => {
            DataValue::Num(Num::Float(a - (*b as f64)))
        }
        (DataValue::Vec(a), DataValue::Vec(b)) => match (a, b) {
            (Vector::F32(a), Vector::F32(b)) => DataValue::Vec(Vector::F32(a - b)),
            (Vector::F64(a), Vector::F64(b)) => DataValue::Vec(Vector::F64(a - b)),
            (Vector::F32(a), Vector::F64(b)) => {
                let a = a.mapv(|x| x as f64);
                DataValue::Vec(Vector::F64(a - b))
            }
            (Vector::F64(a), Vector::F32(b)) => {
                let b = b.mapv(|x| x as f64);
                DataValue::Vec(Vector::F64(a - b))
            }
        },
        (DataValue::Vec(a), b) => {
            let b = b.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "sub",
                    expected: "numbers to subtract from vectors",
                }
                .build()
            })?;
            match a.clone() {
                Vector::F32(mut v) => {
                    v -= b as f32;
                    DataValue::Vec(Vector::F32(v))
                }
                Vector::F64(mut v) => {
                    v -= b;
                    DataValue::Vec(Vector::F64(v))
                }
            }
        }
        (a, DataValue::Vec(b)) => {
            let a = a.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "sub",
                    expected: "vectors to subtract from numbers",
                }
                .build()
            })?;
            match b.clone() {
                Vector::F32(mut v) => {
                    v -= a as f32;
                    DataValue::Vec(Vector::F32(-v))
                }
                Vector::F64(mut v) => {
                    v -= a;
                    DataValue::Vec(Vector::F64(-v))
                }
            }
        }
        _ => {
            return TypeMismatchSnafu {
                op: "sub",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_mul(args: &[DataValue]) -> Result<DataValue> {
    let mut i_accum = 1i64;
    let mut f_accum = 1.0f64;
    for arg in args {
        match arg {
            DataValue::Num(Num::Int(i)) => i_accum *= i,
            DataValue::Num(Num::Float(f)) => f_accum *= f,
            DataValue::Vec(_) => return mul_vecs(args),
            _ => {
                return TypeMismatchSnafu {
                    op: "mul",
                    expected: "numbers",
                }
                .fail();
            }
        }
    }
    if f_accum == 1.0f64 {
        Ok(DataValue::Num(Num::Int(i_accum)))
    } else {
        Ok(DataValue::Num(Num::Float(i_accum as f64 * f_accum)))
    }
}

pub(crate) fn op_div(args: &[DataValue]) -> Result<DataValue> {
    Ok(match (&args[0], &args[1]) {
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Int(b))) => {
            DataValue::Num(Num::Float((*a as f64) / (*b as f64)))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Float(b))) => {
            DataValue::Num(Num::Float(*a / *b))
        }
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Float(b))) => {
            DataValue::Num(Num::Float((*a as f64) / b))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Int(b))) => {
            DataValue::Num(Num::Float(a / (*b as f64)))
        }
        (DataValue::Vec(a), DataValue::Vec(b)) => match (a, b) {
            (Vector::F32(a), Vector::F32(b)) => DataValue::Vec(Vector::F32(a / b)),
            (Vector::F64(a), Vector::F64(b)) => DataValue::Vec(Vector::F64(a / b)),
            (Vector::F32(a), Vector::F64(b)) => {
                let a = a.mapv(|x| x as f64);
                DataValue::Vec(Vector::F64(a / b))
            }
            (Vector::F64(a), Vector::F32(b)) => {
                let b = b.mapv(|x| x as f64);
                DataValue::Vec(Vector::F64(a / b))
            }
        },
        (DataValue::Vec(a), b) => {
            let b = b.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "sub",
                    expected: "numbers to subtract from vectors",
                }
                .build()
            })?;
            match a.clone() {
                Vector::F32(mut v) => {
                    v /= b as f32;
                    DataValue::Vec(Vector::F32(v))
                }
                Vector::F64(mut v) => {
                    v /= b;
                    DataValue::Vec(Vector::F64(v))
                }
            }
        }
        (a, DataValue::Vec(b)) => {
            let a = a.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "sub",
                    expected: "vectors to subtract from numbers",
                }
                .build()
            })?;
            match b {
                Vector::F32(v) => DataValue::Vec(Vector::F32(a as f32 / v)),
                Vector::F64(v) => DataValue::Vec(Vector::F64(a / v)),
            }
        }
        _ => {
            return TypeMismatchSnafu {
                op: "div",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_minus(args: &[DataValue]) -> Result<DataValue> {
    Ok(match &args[0] {
        DataValue::Num(Num::Int(i)) => DataValue::Num(Num::Int(-(*i))),
        DataValue::Num(Num::Float(f)) => DataValue::Num(Num::Float(-(*f))),
        DataValue::Vec(Vector::F64(v)) => DataValue::Vec(Vector::F64(0. - v)),
        DataValue::Vec(Vector::F32(v)) => DataValue::Vec(Vector::F32(0. - v)),
        _ => {
            return TypeMismatchSnafu {
                op: "minus",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_abs(args: &[DataValue]) -> Result<DataValue> {
    Ok(match &args[0] {
        DataValue::Num(Num::Int(i)) => DataValue::Num(Num::Int(i.abs())),
        DataValue::Num(Num::Float(f)) => DataValue::Num(Num::Float(f.abs())),
        DataValue::Vec(Vector::F64(v)) => DataValue::Vec(Vector::F64(v.mapv(|x| x.abs()))),
        DataValue::Vec(Vector::F32(v)) => DataValue::Vec(Vector::F32(v.mapv(|x| x.abs()))),
        _ => {
            return TypeMismatchSnafu {
                op: "abs",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_signum(args: &[DataValue]) -> Result<DataValue> {
    Ok(match &args[0] {
        DataValue::Num(Num::Int(i)) => DataValue::Num(Num::Int(i.signum())),
        DataValue::Num(Num::Float(f)) => {
            if f.signum() < 0. {
                DataValue::from(-1)
            } else if *f == 0. {
                DataValue::from(0)
            } else if *f > 0. {
                DataValue::from(1)
            } else {
                DataValue::from(f64::NAN)
            }
        }
        _ => {
            return TypeMismatchSnafu {
                op: "signum",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_floor(args: &[DataValue]) -> Result<DataValue> {
    Ok(match &args[0] {
        DataValue::Num(Num::Int(i)) => DataValue::Num(Num::Int(*i)),
        DataValue::Num(Num::Float(f)) => DataValue::Num(Num::Float(f.floor())),
        _ => {
            return TypeMismatchSnafu {
                op: "floor",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_ceil(args: &[DataValue]) -> Result<DataValue> {
    Ok(match &args[0] {
        DataValue::Num(Num::Int(i)) => DataValue::Num(Num::Int(*i)),
        DataValue::Num(Num::Float(f)) => DataValue::Num(Num::Float(f.ceil())),
        _ => {
            return TypeMismatchSnafu {
                op: "ceil",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_round(args: &[DataValue]) -> Result<DataValue> {
    Ok(match &args[0] {
        DataValue::Num(Num::Int(i)) => DataValue::Num(Num::Int(*i)),
        DataValue::Num(Num::Float(f)) => DataValue::Num(Num::Float(f.round())),
        _ => {
            return TypeMismatchSnafu {
                op: "round",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_sqrt(args: &[DataValue]) -> Result<DataValue> {
    let a = match &args[0] {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.sqrt()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.sqrt()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "sqrt",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.sqrt())))
}

pub(crate) fn op_exp(args: &[DataValue]) -> Result<DataValue> {
    let a = match &args[0] {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.exp()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.exp()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "exp",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.exp())))
}

pub(crate) fn op_exp2(args: &[DataValue]) -> Result<DataValue> {
    let a = match &args[0] {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.exp2()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.exp2()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "exp2",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.exp2())))
}

pub(crate) fn op_ln(args: &[DataValue]) -> Result<DataValue> {
    let a = match &args[0] {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.ln()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.ln()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "ln",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.ln())))
}

pub(crate) fn op_log2(args: &[DataValue]) -> Result<DataValue> {
    let a = match &args[0] {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.log2()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.log2()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "log2",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.log2())))
}

pub(crate) fn op_log10(args: &[DataValue]) -> Result<DataValue> {
    let a = match &args[0] {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.log10()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.log10()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "log10",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.log10())))
}

pub(crate) fn op_pow(args: &[DataValue]) -> Result<DataValue> {
    let a = match &args[0] {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            let b = args[1].get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "pow",
                    expected: "numbers",
                }
                .build()
            })?;
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.powf(b as f32)))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            let b = args[1].get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "pow",
                    expected: "numbers",
                }
                .build()
            })?;
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.powf(b)))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "pow",
                expected: "numbers",
            }
            .fail();
        }
    };
    let b = match &args[1] {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        _ => {
            return TypeMismatchSnafu {
                op: "pow",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.powf(b))))
}

pub(crate) fn op_mod(args: &[DataValue]) -> Result<DataValue> {
    Ok(match (&args[0], &args[1]) {
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Int(b))) => {
            if *b == 0 {
                return TypeMismatchSnafu {
                    op: "mod",
                    expected: "non-zero divisor",
                }
                .fail();
            }
            DataValue::Num(Num::Int(a.rem(b)))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Float(b))) => {
            DataValue::Num(Num::Float(a.rem(*b)))
        }
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Float(b))) => {
            DataValue::Num(Num::Float((*a as f64).rem(b)))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Int(b))) => {
            DataValue::Num(Num::Float(a.rem(*b as f64)))
        }
        _ => {
            return TypeMismatchSnafu {
                op: "mod",
                expected: "numbers",
            }
            .fail();
        }
    })
}

pub(crate) fn op_is_in(args: &[DataValue]) -> Result<DataValue> {
    let left = &args[0];
    let right = args[1].get_slice().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "is_in",
            expected: "a list as right hand side",
        }
        .build()
    })?;
    Ok(DataValue::from(right.contains(left)))
}

pub(crate) fn op_coalesce(args: &[DataValue]) -> Result<DataValue> {
    for val in args {
        if *val != DataValue::Null {
            return Ok(val.clone());
        }
    }
    Ok(DataValue::Null)
}
