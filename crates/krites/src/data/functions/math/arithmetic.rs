//! Basic arithmetic operators.

use super::{arg, mul_vecs};
use crate::data::error::*;
type Result<T> = DataResult<T>;
use crate::data::value::{DataValue, Num, Vector};

pub(crate) fn op_sub(args: &[DataValue]) -> Result<DataValue> {
    Ok(match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Int(b))) => {
            DataValue::Num(Num::Int(*a - *b))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Float(b))) => {
            DataValue::Num(Num::Float(*a - *b))
        }
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Float(b))) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "i64 to f64: precision loss acceptable"
            )]
            let lhs = *a as f64;
            DataValue::Num(Num::Float(lhs - b))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Int(b))) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "i64 to f64: precision loss acceptable"
            )]
            let rhs = *b as f64;
            DataValue::Num(Num::Float(a - rhs))
        }
        (DataValue::Vec(a), DataValue::Vec(b)) => match (a, b) {
            (Vector::F32(a), Vector::F32(b)) => DataValue::Vec(Vector::F32(a - b)),
            (Vector::F64(a), Vector::F64(b)) => DataValue::Vec(Vector::F64(a - b)),
            (Vector::F32(a), Vector::F64(b)) => {
                let a = a.mapv(f64::from);
                DataValue::Vec(Vector::F64(a - b))
            }
            (Vector::F64(a), Vector::F32(b)) => {
                let b = b.mapv(f64::from);
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
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
                    )]
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "f64 to f32: intentional precision reduction"
                    )]
                    let b = b as f32;
                    v -= b;
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
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
                    )]
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "f64 to f32: intentional precision reduction"
                    )]
                    let a = a as f32;
                    v -= a;
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
        #[expect(
            clippy::cast_precision_loss,
            reason = "i64 to f64: precision loss acceptable"
        )]
        let acc = i_accum as f64;
        Ok(DataValue::Num(Num::Float(acc * f_accum)))
    }
}

pub(crate) fn op_div(args: &[DataValue]) -> Result<DataValue> {
    Ok(match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Int(b))) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "i64 to f64: precision loss acceptable"
            )]
            let lhs = *a as f64;
            #[expect(
                clippy::cast_precision_loss,
                reason = "i64 to f64: precision loss acceptable"
            )]
            let rhs = *b as f64;
            DataValue::Num(Num::Float(lhs / rhs))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Float(b))) => {
            DataValue::Num(Num::Float(*a / *b))
        }
        (DataValue::Num(Num::Int(a)), DataValue::Num(Num::Float(b))) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "i64 to f64: precision loss acceptable"
            )]
            let lhs = *a as f64;
            DataValue::Num(Num::Float(lhs / b))
        }
        (DataValue::Num(Num::Float(a)), DataValue::Num(Num::Int(b))) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "i64 to f64: precision loss acceptable"
            )]
            let rhs = *b as f64;
            DataValue::Num(Num::Float(a / rhs))
        }
        (DataValue::Vec(a), DataValue::Vec(b)) => match (a, b) {
            (Vector::F32(a), Vector::F32(b)) => DataValue::Vec(Vector::F32(a / b)),
            (Vector::F64(a), Vector::F64(b)) => DataValue::Vec(Vector::F64(a / b)),
            (Vector::F32(a), Vector::F64(b)) => {
                let a = a.mapv(f64::from);
                DataValue::Vec(Vector::F64(a / b))
            }
            (Vector::F64(a), Vector::F32(b)) => {
                let b = b.mapv(f64::from);
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
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
                    )]
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "f64 to f32: intentional precision reduction"
                    )]
                    let b = b as f32;
                    v /= b;
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
                Vector::F32(v) => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "intentional F64→F32 reduction for mixed-precision vector arithmetic"
                    )]
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "f64 to f32: intentional precision reduction"
                    )]
                    let a = a as f32;
                    DataValue::Vec(Vector::F32(a / v))
                }
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
    Ok(match arg(args, 0)? {
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
    Ok(match arg(args, 0)? {
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
    Ok(match arg(args, 0)? {
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
    Ok(match arg(args, 0)? {
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
    Ok(match arg(args, 0)? {
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
    Ok(match arg(args, 0)? {
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
