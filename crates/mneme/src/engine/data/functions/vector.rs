//! Vector creation and distance operations.
use std::mem;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::Rng;

use super::arg;
use crate::engine::data::error::*;
type Result<T> = DataResult<T>;
use crate::engine::data::relation::VecElementType;
use crate::engine::data::value::{DataValue, Vector};

#[expect(clippy::map_err_ignore, reason = "error context preserved in message")]
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
                        row.fill(f as f32);
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
                    row.fill(f as f32);
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
                Ok(DataValue::Vec(Vector::F32(v.mapv(|x| x as f32))))
            }
            (VecElementType::F64, Vector::F32(v)) => {
                Ok(DataValue::Vec(Vector::F64(v.mapv(|x| x as f64))))
            }
        },
        DataValue::Str(s) => {
            let bytes = STANDARD.decode(s).map_err(|_| {
                EncodingFailedSnafu {
                    message: "Data is not base64 encoded",
                }
                .build()
            })?;
            match t {
                VecElementType::F32 => {
                    let f32_count = bytes.len() / mem::size_of::<f32>();
                    // Rust's global allocator guarantees allocations are aligned
                    // to at least max_align_t (≥ 16 bytes on 64-bit platforms),
                    // satisfying align_of::<f32>() == 4.
                    debug_assert_eq!(
                        bytes.as_ptr() as usize % mem::align_of::<f32>(),
                        0,
                        "Vec<u8> buffer must be aligned for f32 reinterpretation"
                    );
                    // SAFETY: `bytes` is a Vec<u8> produced by base64-decoding a
                    // serialised f32 vector (written by `Vector::serialize`). Three
                    // invariants hold:
                    // (1) Alignment: the global allocator guarantees the buffer is
                    //     aligned to at least max_align_t (≥ 16 B), which is larger
                    //     than align_of::<f32>() (4 B); the debug_assert above
                    //     verifies this at runtime in debug builds.
                    // (2) Length: `f32_count` is `bytes.len() / size_of::<f32>()`, so
                    //     the pointer covers exactly `f32_count` fully-initialised f32
                    //     elements within the live `bytes` allocation.
                    // (3) Lifetime: `arr` is immediately converted to an owned array
                    //     via `to_owned()` before `bytes` is dropped, so the view
                    //     never outlives the backing buffer.
                    // Violating any of these would cause UB: misaligned read,
                    // out-of-bounds access, or dangling pointer respectively.
                    let arr = unsafe {
                        ndarray::ArrayView1::from_shape_ptr(
                            ndarray::Dim([f32_count]),
                            bytes.as_ptr() as *const f32,
                        )
                    };
                    Ok(DataValue::Vec(Vector::F32(arr.to_owned())))
                }
                VecElementType::F64 => {
                    let f64_count = bytes.len() / mem::size_of::<f64>();
                    // Rust's global allocator guarantees allocations are aligned
                    // to at least max_align_t (≥ 16 bytes on 64-bit platforms),
                    // satisfying align_of::<f64>() == 8.
                    debug_assert_eq!(
                        bytes.as_ptr() as usize % mem::align_of::<f64>(),
                        0,
                        "Vec<u8> buffer must be aligned for f64 reinterpretation"
                    );
                    // SAFETY: `bytes` is a Vec<u8> produced by base64-decoding a
                    // serialised f64 vector (written by `Vector::serialize`). Three
                    // invariants hold:
                    // (1) Alignment: the global allocator guarantees the buffer is
                    //     aligned to at least max_align_t (≥ 16 B), which is larger
                    //     than align_of::<f64>() (8 B); the debug_assert above
                    //     verifies this at runtime in debug builds.
                    // (2) Length: `f64_count` is `bytes.len() / size_of::<f64>()`, so
                    //     the pointer covers exactly `f64_count` fully-initialised f64
                    //     elements within the live `bytes` allocation.
                    // (3) Lifetime: `arr` is immediately converted to an owned array
                    //     via `to_owned()` before `bytes` is dropped, so the view
                    //     never outlives the backing buffer.
                    // Violating any of these would cause UB: misaligned read,
                    // out-of-bounds access, or dangling pointer respectively.
                    let arr = unsafe {
                        ndarray::ArrayView1::from_shape_ptr(
                            ndarray::Dim([f64_count]),
                            bytes.as_ptr() as *const f64,
                        )
                    };
                    Ok(DataValue::Vec(Vector::F64(arr.to_owned())))
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
    let len = arg(args, 0)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "rand_vec",
            expected: "an integer",
        }
        .build()
    })? as usize;
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
                row.fill(rng.random::<f64>() as f32);
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
            Ok(DataValue::from(diff.dot(&diff) as f64))
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
            Ok(DataValue::from(1. - dot as f64))
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
            let a_norm = a.dot(a) as f64;
            let b_norm = b.dot(b) as f64;
            let dot = a.dot(b) as f64;
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
