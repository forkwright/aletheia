//! Vector creation and distance operations.
#![expect(
    clippy::as_conversions,
    reason = "vector operations require numeric casts (f64 → f32) for element conversion"
)]
#![expect(
    clippy::map_err_ignore,
    reason = "bytemuck PodCastError detail is not useful to the caller; the message describes the failure"
)]
#![expect(
    clippy::too_many_lines,
    reason = "op_vec handles multiple input types and vector element types"
)]

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::Rng;

use super::arg;
use crate::data::error::*;
type Result<T> = DataResult<T>;
use crate::data::relation::VecElementType;
use crate::data::value::{DataValue, Vector};

pub(crate) fn op_vec(args: &[DataValue]) -> Result<DataValue> {
    let t = match args.get(1) {
        Some(DataValue::Str(s)) => match s as &str {
            "F32" | "Float" => VecElementType::F32,
            "F64" | "Double" => VecElementType::F64,
            _ => {
                return InvalidValueSnafu {
                    message: format!("'vec' does not recognize type {s}"),
                }
                .fail();
            }
        },
        None => VecElementType::F32,
        _ => {
            return TypeMismatchSnafu {
                op: "vec",
                expected: "a string as second argument",
            }
            .fail();
        }
    };

    match arg(args, 0)? {
        DataValue::Json(j) => {
            let arr = j.0.as_array().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "vec",
                    expected: "a JSON array",
                }
                .build()
            })?;
            match t {
                VecElementType::F32 => {
                    let mut res_arr = ndarray::Array1::zeros(arr.len());
                    for (mut row, el) in res_arr.axis_iter_mut(ndarray::Axis(0)).zip(arr.iter()) {
                        let f = el.as_f64().ok_or_else(|| {
                            TypeMismatchSnafu {
                                op: "vec",
                                expected: "a list of numbers",
                            }
                            .build()
                        })?;
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "f64 to f32: intentional precision reduction"
                        )]
                        let f = f as f32;
                        row.fill(f);
                    }
                    Ok(DataValue::Vec(Vector::F32(res_arr)))
                }
                VecElementType::F64 => {
                    let mut res_arr = ndarray::Array1::zeros(arr.len());
                    for (mut row, el) in res_arr.axis_iter_mut(ndarray::Axis(0)).zip(arr.iter()) {
                        let f = el.as_f64().ok_or_else(|| {
                            TypeMismatchSnafu {
                                op: "vec",
                                expected: "a list of numbers",
                            }
                            .build()
                        })?;
                        row.fill(f);
                    }
                    Ok(DataValue::Vec(Vector::F64(res_arr)))
                }
            }
        }
        DataValue::List(l) => match t {
            VecElementType::F32 => {
                let mut res_arr = ndarray::Array1::zeros(l.len());
                for (mut row, el) in res_arr.axis_iter_mut(ndarray::Axis(0)).zip(l.iter()) {
                    let f = el.get_float().ok_or_else(|| {
                        TypeMismatchSnafu {
                            op: "vec",
                            expected: "a list of numbers",
                        }
                        .build()
                    })?;
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "f64 to f32: intentional precision reduction"
                    )]
                    let f = f as f32;
                    row.fill(f);
                }
                Ok(DataValue::Vec(Vector::F32(res_arr)))
            }
            VecElementType::F64 => {
                let mut res_arr = ndarray::Array1::zeros(l.len());
                for (mut row, el) in res_arr.axis_iter_mut(ndarray::Axis(0)).zip(l.iter()) {
                    let f = el.get_float().ok_or_else(|| {
                        TypeMismatchSnafu {
                            op: "vec",
                            expected: "a list of numbers",
                        }
                        .build()
                    })?;
                    row.fill(f);
                }
                Ok(DataValue::Vec(Vector::F64(res_arr)))
            }
        },
        DataValue::Vec(v) => match (t, v) {
            (VecElementType::F32, Vector::F32(v)) => Ok(DataValue::Vec(Vector::F32(v.clone()))),
            (VecElementType::F64, Vector::F64(v)) => Ok(DataValue::Vec(Vector::F64(v.clone()))),
            (VecElementType::F32, Vector::F64(v)) => {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "f64 to f32: intentional precision reduction"
                )]
                let result = v.mapv(|x| x as f32);
                Ok(DataValue::Vec(Vector::F32(result)))
            }
            (VecElementType::F64, Vector::F32(v)) => {
                Ok(DataValue::Vec(Vector::F64(v.mapv(f64::from))))
            }
        },
        DataValue::Str(s) => {
            let bytes = STANDARD.decode(s).map_err(|_e| {
                EncodingFailedSnafu {
                    message: "Data is not base64 encoded",
                }
                .build()
            })?;
            match t {
                VecElementType::F32 => {
                    let floats: &[f32] = bytemuck::try_cast_slice(&bytes).map_err(|_| {
                        EncodingFailedSnafu {
                            message: "f32 vector data is not properly aligned or sized",
                        }
                        .build()
                    })?;
                    Ok(DataValue::Vec(Vector::F32(ndarray::Array1::from(
                        floats.to_vec(),
                    ))))
                }
                VecElementType::F64 => {
                    let floats: &[f64] = bytemuck::try_cast_slice(&bytes).map_err(|_| {
                        EncodingFailedSnafu {
                            message: "f64 vector data is not properly aligned or sized",
                        }
                        .build()
                    })?;
                    Ok(DataValue::Vec(Vector::F64(ndarray::Array1::from(
                        floats.to_vec(),
                    ))))
                }
            }
        }
        _ => TypeMismatchSnafu {
            op: "vec",
            expected: "a list or a vector",
        }
        .fail(),
    }
}

pub(crate) fn op_rand_vec(args: &[DataValue]) -> Result<DataValue> {
    let len_i64 = arg(args, 0)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "rand_vec",
            expected: "an integer",
        }
        .build()
    })?;
    let len = usize::try_from(len_i64).map_err(|_e| {
        InvalidValueSnafu {
            message: format!("rand_vec length must be non-negative, got {len_i64}"),
        }
        .build()
    })?;
    let t = match args.get(1) {
        Some(DataValue::Str(s)) => match s as &str {
            "F32" | "Float" => VecElementType::F32,
            "F64" | "Double" => VecElementType::F64,
            _ => {
                return InvalidValueSnafu {
                    message: format!("'vec' does not recognize type {s}"),
                }
                .fail();
            }
        },
        None => VecElementType::F32,
        _ => {
            return TypeMismatchSnafu {
                op: "vec",
                expected: "a string as second argument",
            }
            .fail();
        }
    };

    let mut rng = rand::rng();
    match t {
        VecElementType::F32 => {
            let mut res_arr = ndarray::Array1::zeros(len);
            for mut row in res_arr.axis_iter_mut(ndarray::Axis(0)) {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "f64 to f32: intentional precision reduction"
                )]
                let val = rng.random::<f64>() as f32;
                row.fill(val);
            }
            Ok(DataValue::Vec(Vector::F32(res_arr)))
        }
        VecElementType::F64 => {
            let mut res_arr = ndarray::Array1::zeros(len);
            for mut row in res_arr.axis_iter_mut(ndarray::Axis(0)) {
                row.fill(rng.random::<f64>());
            }
            Ok(DataValue::Vec(Vector::F64(res_arr)))
        }
    }
}

pub(crate) fn op_l2_normalize(args: &[DataValue]) -> Result<DataValue> {
    let a = arg(args, 0)?;
    match a {
        DataValue::Vec(Vector::F32(a)) => {
            let norm = a.dot(a).sqrt();
            Ok(DataValue::Vec(Vector::F32(a / norm)))
        }
        DataValue::Vec(Vector::F64(a)) => {
            let norm = a.dot(a).sqrt();
            Ok(DataValue::Vec(Vector::F64(a / norm)))
        }
        _ => TypeMismatchSnafu {
            op: "l2_normalize",
            expected: "a vector",
        }
        .fail(),
    }
}

pub(crate) fn op_l2_dist(args: &[DataValue]) -> Result<DataValue> {
    let a = arg(args, 0)?;
    let b = arg(args, 1)?;
    match (a, b) {
        (DataValue::Vec(Vector::F32(a)), DataValue::Vec(Vector::F32(b))) => {
            if a.len() != b.len() {
                return TypeMismatchSnafu {
                    op: "l2_dist",
                    expected: "two vectors of the same length",
                }
                .fail();
            }
            let diff = a - b;
            Ok(DataValue::from(f64::from(diff.dot(&diff))))
        }
        (DataValue::Vec(Vector::F64(a)), DataValue::Vec(Vector::F64(b))) => {
            if a.len() != b.len() {
                return TypeMismatchSnafu {
                    op: "l2_dist",
                    expected: "two vectors of the same length",
                }
                .fail();
            }
            let diff = a - b;
            Ok(DataValue::from(diff.dot(&diff)))
        }
        _ => TypeMismatchSnafu {
            op: "l2_dist",
            expected: "two vectors of the same type",
        }
        .fail(),
    }
}

pub(crate) fn op_ip_dist(args: &[DataValue]) -> Result<DataValue> {
    let a = arg(args, 0)?;
    let b = arg(args, 1)?;
    match (a, b) {
        (DataValue::Vec(Vector::F32(a)), DataValue::Vec(Vector::F32(b))) => {
            if a.len() != b.len() {
                return TypeMismatchSnafu {
                    op: "ip_dist",
                    expected: "two vectors of the same length",
                }
                .fail();
            }
            let dot = a.dot(b);
            Ok(DataValue::from(1. - f64::from(dot)))
        }
        (DataValue::Vec(Vector::F64(a)), DataValue::Vec(Vector::F64(b))) => {
            if a.len() != b.len() {
                return TypeMismatchSnafu {
                    op: "ip_dist",
                    expected: "two vectors of the same length",
                }
                .fail();
            }
            let dot = a.dot(b);
            Ok(DataValue::from(1. - dot))
        }
        _ => TypeMismatchSnafu {
            op: "ip_dist",
            expected: "two vectors of the same type",
        }
        .fail(),
    }
}

pub(crate) fn op_cos_dist(args: &[DataValue]) -> Result<DataValue> {
    let a = arg(args, 0)?;
    let b = arg(args, 1)?;
    match (a, b) {
        (DataValue::Vec(Vector::F32(a)), DataValue::Vec(Vector::F32(b))) => {
            if a.len() != b.len() {
                return TypeMismatchSnafu {
                    op: "cos_dist",
                    expected: "two vectors of the same length",
                }
                .fail();
            }
            let a_norm = f64::from(a.dot(a));
            let b_norm = f64::from(b.dot(b));
            let dot = f64::from(a.dot(b));
            Ok(DataValue::from(1. - dot / (a_norm * b_norm).sqrt()))
        }
        (DataValue::Vec(Vector::F64(a)), DataValue::Vec(Vector::F64(b))) => {
            if a.len() != b.len() {
                return TypeMismatchSnafu {
                    op: "cos_dist",
                    expected: "two vectors of the same length",
                }
                .fail();
            }
            let a_norm = a.dot(a);
            let b_norm = b.dot(b);
            let dot = a.dot(b);
            Ok(DataValue::from(1. - dot / (a_norm * b_norm).sqrt()))
        }
        _ => TypeMismatchSnafu {
            op: "cos_dist",
            expected: "two vectors of the same type",
        }
        .fail(),
    }
}
