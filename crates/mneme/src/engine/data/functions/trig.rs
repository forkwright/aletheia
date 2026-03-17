//! Trigonometric, hyperbolic, and geographic functions.
use crate::engine::data::error::*;
type Result<T> = DataResult<T>;
use super::arg;
use crate::engine::data::value::{DataValue, Num, Vector};

pub(crate) fn op_sin(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.sin()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.sin()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "sin",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.sin())))
}

pub(crate) fn op_cos(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.cos()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.cos()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "cos",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.cos())))
}

pub(crate) fn op_tan(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.tan()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.tan()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "tan",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.tan())))
}

pub(crate) fn op_asin(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.asin()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.asin()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "asin",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.asin())))
}

pub(crate) fn op_acos(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.acos()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.acos()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "acos",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.acos())))
}

pub(crate) fn op_atan(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.atan()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.atan()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "atan",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.atan())))
}

pub(crate) fn op_atan2(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        _ => {
            return TypeMismatchSnafu {
                op: "atan2",
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
                op: "atan2",
                expected: "numbers",
            }
            .fail();
        }
    };

    Ok(DataValue::Num(Num::Float(a.atan2(b))))
}

pub(crate) fn op_sinh(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.sinh()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.sinh()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "sinh",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.sinh())))
}

pub(crate) fn op_cosh(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.cosh()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.cosh()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "cosh",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.cosh())))
}

pub(crate) fn op_tanh(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.tanh()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.tanh()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "tanh",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.tanh())))
}

pub(crate) fn op_asinh(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.asinh()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.asinh()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "asinh",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.asinh())))
}

pub(crate) fn op_acosh(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.acosh()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.acosh()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "acosh",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.acosh())))
}

pub(crate) fn op_atanh(args: &[DataValue]) -> Result<DataValue> {
    let a = match arg(args, 0)? {
        DataValue::Num(Num::Int(i)) => *i as f64,
        DataValue::Num(Num::Float(f)) => *f,
        DataValue::Vec(Vector::F32(v)) => {
            return Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x.atanh()))));
        }
        DataValue::Vec(Vector::F64(v)) => {
            return Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x.atanh()))));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "atanh",
                expected: "numbers",
            }
            .fail();
        }
    };
    Ok(DataValue::Num(Num::Float(a.atanh())))
}

pub(crate) fn op_haversine(args: &[DataValue]) -> Result<DataValue> {
    let make_err = || {
        TypeMismatchSnafu {
            op: "haversine",
            expected: "numbers",
        }
        .build()
    };
    let lat1 = arg(args, 0)?.get_float().ok_or_else(make_err)?;
    let lon1 = arg(args, 1)?.get_float().ok_or_else(make_err)?;
    let lat2 = arg(args, 2)?.get_float().ok_or_else(make_err)?;
    let lon2 = arg(args, 3)?.get_float().ok_or_else(make_err)?;
    let ret = 2.
        * f64::asin(f64::sqrt(
            f64::sin((lat1 - lat2) / 2.).powi(2)
                + f64::cos(lat1) * f64::cos(lat2) * f64::sin((lon1 - lon2) / 2.).powi(2),
        ));
    Ok(DataValue::from(ret))
}

pub(crate) fn op_haversine_deg_input(args: &[DataValue]) -> Result<DataValue> {
    let make_err = || {
        TypeMismatchSnafu {
            op: "haversine_deg_input",
            expected: "numbers",
        }
        .build()
    };
    let lat1 = arg(args, 0)?.get_float().ok_or_else(make_err)? * std::f64::consts::PI / 180.;
    let lon1 = arg(args, 1)?.get_float().ok_or_else(make_err)? * std::f64::consts::PI / 180.;
    let lat2 = arg(args, 2)?.get_float().ok_or_else(make_err)? * std::f64::consts::PI / 180.;
    let lon2 = arg(args, 3)?.get_float().ok_or_else(make_err)? * std::f64::consts::PI / 180.;
    let ret = 2.
        * f64::asin(f64::sqrt(
            f64::sin((lat1 - lat2) / 2.).powi(2)
                + f64::cos(lat1) * f64::cos(lat2) * f64::sin((lon1 - lon2) / 2.).powi(2),
        ));
    Ok(DataValue::from(ret))
}

pub(crate) fn op_deg_to_rad(args: &[DataValue]) -> Result<DataValue> {
    let x = arg(args, 0)?.get_float().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "deg_to_rad",
            expected: "numbers",
        }
        .build()
    })?;
    Ok(DataValue::from(x * std::f64::consts::PI / 180.))
}

pub(crate) fn op_rad_to_deg(args: &[DataValue]) -> Result<DataValue> {
    let x = arg(args, 0)?.get_float().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "rad_to_deg",
            expected: "numbers",
        }
        .build()
    })?;
    Ok(DataValue::from(x * 180. / std::f64::consts::PI))
}
