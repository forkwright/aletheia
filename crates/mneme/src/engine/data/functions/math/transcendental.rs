//! Transcendental and utility math operators.
use std::ops::Rem;

use super::arg;
use crate::engine::data::error::*;
type Result<T> = DataResult<T>;
use crate::engine::data::value::{DataValue, Num, Vector};

pub(crate) fn op_sqrt(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
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
    let a = match arg(args, 0)? {
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
    let a = match arg(args, 0)? {
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
    let a = match arg(args, 0)? {
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
    let a = match arg(args, 0)? {
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
    let a = match arg(args, 0)? {
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
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            let b = arg(args, 1)?.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "pow",
                    expected: "numbers",
                }
                .build()
            })?;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
            )]
            let b = b as f32;
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.powf(b)))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            let b = arg(args, 1)?.get_float().ok_or_else(|| {
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
    let b = match arg(args, 1)? {
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
    Ok(match (arg(args, 0)?, arg(args, 1)?) {
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
    let left = arg(args, 0)?;
    let right = arg(args, 1)?.get_slice().ok_or_else(|| {
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
