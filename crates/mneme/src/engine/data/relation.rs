//! Relation metadata and schema definitions.
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::cmp::Reverse;
use std::fmt::{Display, Formatter};
use std::mem;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use compact_str::CompactString;
use itertools::Itertools;
use jiff::Timestamp;
use serde_json::json;

use super::error::*;
use crate::engine::data::expr::Expr;
use crate::engine::data::value::Num;
use crate::engine::data::value::{DataValue, JsonData, UuidWrapper, Validity, ValidityTs, Vector};

#[derive(Debug, Clone, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct NullableColType {
    pub coltype: ColType,
    pub nullable: bool,
}

impl Display for NullableColType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.coltype {
            ColType::Any => f.write_str("Any")?,
            ColType::Bool => f.write_str("Bool")?,
            ColType::Int => f.write_str("Int")?,
            ColType::Float => f.write_str("Float")?,
            ColType::String => f.write_str("String")?,
            ColType::Bytes => f.write_str("Bytes")?,
            ColType::Uuid => f.write_str("Uuid")?,
            ColType::Validity => f.write_str("Validity")?,
            ColType::List { eltype, len } => {
                f.write_str("[")?;
                write!(f, "{eltype}")?;
                if let Some(l) = len {
                    write!(f, ";{l}")?;
                }
                f.write_str("]")?;
            }
            ColType::Tuple(t) => {
                f.write_str("(")?;
                let l = t.len();
                for (i, el) in t.iter().enumerate() {
                    write!(f, "{el}")?;
                    if i != l - 1 {
                        f.write_str(",")?
                    }
                }
                f.write_str(")")?;
            }
            ColType::Vec { eltype, len } => {
                f.write_str("<")?;
                match eltype {
                    VecElementType::F32 => f.write_str("F32")?,
                    VecElementType::F64 => f.write_str("F64")?,
                }
                write!(f, ";{len}")?;
                f.write_str(">")?;
            }
            ColType::Json => {
                f.write_str("Json")?;
            }
        }
        if self.nullable {
            f.write_str("?")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub enum ColType {
    Any,
    Bool,
    Int,
    Float,
    String,
    Bytes,
    Uuid,
    List {
        eltype: Box<NullableColType>,
        len: Option<usize>,
    },
    Vec {
        eltype: VecElementType,
        len: usize,
    },
    Tuple(Vec<NullableColType>),
    Validity,
    Json,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub enum VecElementType {
    F32,
    F64,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct ColumnDef {
    pub(crate) name: CompactString,
    pub(crate) typing: NullableColType,
    pub(crate) default_gen: Option<Expr>,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct StoredRelationMetadata {
    pub(crate) keys: Vec<ColumnDef>,
    pub(crate) non_keys: Vec<ColumnDef>,
}

impl StoredRelationMetadata {
    pub(crate) fn satisfied_by_required_col(&self, col: &ColumnDef) -> DataResult<()> {
        for target in self.keys.iter().chain(self.non_keys.iter()) {
            if target.name == col.name {
                return Ok(());
            }
        }
        if col.default_gen.is_none() {
            return FieldNotFoundSnafu {
                message: format!("required column {} not provided by input", col.name),
            }
            .fail();
        }
        Ok(())
    }
    pub(crate) fn compatible_with_col(&self, col: &ColumnDef) -> DataResult<()> {
        for target in self.keys.iter().chain(self.non_keys.iter()) {
            if target.name == col.name {
                if (!col.typing.nullable || col.typing.coltype != ColType::Any)
                    && target.typing != col.typing
                {
                    return CoercionFailedSnafu {
                        message: format!(
                            "requested column {} has typing {}, but the requested typing is {}",
                            col.name, target.typing, col.typing
                        ),
                    }
                    .fail();
                }

                return Ok(());
            }
        }

        FieldNotFoundSnafu {
            message: format!("required column {} not found", col.name),
        }
        .fail()
    }
}

impl NullableColType {
    #[expect(
        clippy::map_err_ignore,
        reason = "error context preserved in returned error type"
    )]
    pub(crate) fn coerce(&self, data: DataValue, cur_vld: ValidityTs) -> DataResult<DataValue> {
        if matches!(data, DataValue::Null) {
            return if self.nullable {
                Ok(data)
            } else {
                CoercionFailedSnafu {
                    message: format!("encountered null value for non-null type {}", self),
                }
                .fail()
            };
        }

        let make_err = || {
            CoercionFailedSnafu {
                message: format!(
                    "data coercion failed: expected type {}, got value {:?}",
                    self, data
                ),
            }
            .build()
        };

        let make_bad_len = |len: usize| {
            CoercionFailedSnafu {
                message: format!(
                    "bad list length: expected datatype {}, got length {}",
                    self, len
                ),
            }
            .build()
        };

        Ok(match &self.coltype {
            ColType::Any => match data {
                DataValue::Set(s) => DataValue::List(s.into_iter().collect_vec()),
                DataValue::Bot => {
                    return CoercionFailedSnafu {
                        message: "data coercion failed: internal type Bot not allowed".to_string(),
                    }
                    .fail();
                }
                d => d,
            },
            ColType::Bool => DataValue::from(data.get_bool().ok_or_else(make_err)?),
            ColType::Int => DataValue::from(data.get_int().ok_or_else(make_err)?),
            ColType::Float => DataValue::from(data.get_float().ok_or_else(make_err)?),
            ColType::String => {
                if matches!(data, DataValue::Str(_)) {
                    data
                } else {
                    return Err(make_err());
                }
            }
            ColType::Bytes => match data {
                d @ DataValue::Bytes(_) => d,
                DataValue::Str(s) => {
                    let b = STANDARD.decode(s).map_err(|e| {
                        EncodingFailedSnafu {
                            message: format!("cannot decode string as base64-encoded bytes: {e}"),
                        }
                        .build()
                    })?;
                    DataValue::Bytes(b)
                }
                _ => return Err(make_err()),
            },
            ColType::Uuid => DataValue::Uuid(UuidWrapper(data.get_uuid().ok_or_else(make_err)?)),
            ColType::List { eltype, len } => {
                if let DataValue::List(l) = data {
                    if let Some(expected) = len {
                        snafu::ensure!(
                            *expected == l.len(),
                            CoercionFailedSnafu {
                                message: format!(
                                    "bad list length: expected datatype {}, got length {}",
                                    self,
                                    l.len()
                                ),
                            }
                        );
                    }
                    DataValue::List(
                        l.into_iter()
                            .map(|el| eltype.coerce(el, cur_vld))
                            .try_collect()?,
                    )
                } else {
                    return Err(make_err());
                }
            }
            ColType::Vec { eltype, len } => match &data {
                DataValue::List(l) => {
                    if l.len() != *len {
                        return Err(make_bad_len(l.len()));
                    }
                    match eltype {
                        VecElementType::F32 => {
                            let mut res_arr = ndarray::Array1::zeros(*len);
                            for (mut row, el) in
                                res_arr.axis_iter_mut(ndarray::Axis(0)).zip(l.iter())
                            {
                                let f = el.get_float().ok_or_else(make_err)? as f32;
                                row.fill(f);
                            }
                            DataValue::Vec(Vector::F32(res_arr))
                        }
                        VecElementType::F64 => {
                            let mut res_arr = ndarray::Array1::zeros(*len);
                            for (mut row, el) in
                                res_arr.axis_iter_mut(ndarray::Axis(0)).zip(l.iter())
                            {
                                let f = el.get_float().ok_or_else(make_err)?;
                                row.fill(f);
                            }
                            DataValue::Vec(Vector::F64(res_arr))
                        }
                    }
                }
                DataValue::Vec(arr) => {
                    if *eltype != arr.el_type() || *len != arr.len() {
                        return Err(make_err());
                    } else {
                        data
                    }
                }
                DataValue::Str(s) => {
                    let bytes = STANDARD.decode(s).map_err(|_| make_err())?;
                    match eltype {
                        VecElementType::F32 => {
                            let f32_count = bytes.len() / mem::size_of::<f32>();
                            if f32_count != *len {
                                return Err(make_err());
                            }
                            debug_assert_eq!(
                                bytes.as_ptr() as usize % mem::align_of::<f32>(),
                                0,
                                "Vec<u8> buffer must be aligned for f32 reinterpretation"
                            );
                            // SAFETY: `bytes` is a Vec<u8> produced by base64-decoding a
                            // serialised f32 vector. Three invariants hold:
                            // (1) Alignment: the global allocator guarantees the buffer is
                            //     aligned to at least max_align_t (≥ 16 B), which is larger
                            //     than align_of::<f32>() (4 B); the debug_assert above
                            //     verifies this at runtime in debug builds.
                            // (2) Length: the length check above ensures `f32_count == *len`,
                            //     so the pointer covers exactly `f32_count` initialised f32
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
                            DataValue::Vec(Vector::F32(arr.to_owned()))
                        }
                        VecElementType::F64 => {
                            let f64_count = bytes.len() / mem::size_of::<f64>();
                            if f64_count != *len {
                                return Err(make_err());
                            }
                            debug_assert_eq!(
                                bytes.as_ptr() as usize % mem::align_of::<f64>(),
                                0,
                                "Vec<u8> buffer must be aligned for f64 reinterpretation"
                            );
                            // SAFETY: `bytes` is a Vec<u8> produced by base64-decoding a
                            // serialised f64 vector. Three invariants hold:
                            // (1) Alignment: the global allocator guarantees the buffer is
                            //     aligned to at least max_align_t (≥ 16 B), which is larger
                            //     than align_of::<f64>() (8 B); the debug_assert above
                            //     verifies this at runtime in debug builds.
                            // (2) Length: the length check above ensures `f64_count == *len`,
                            //     so the pointer covers exactly `f64_count` initialised f64
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
                            DataValue::Vec(Vector::F64(arr.to_owned()))
                        }
                    }
                }
                _ => return Err(make_err()),
            },
            ColType::Tuple(typ) => {
                if let DataValue::List(l) = data {
                    snafu::ensure!(
                        typ.len() == l.len(),
                        CoercionFailedSnafu {
                            message: format!(
                                "bad list length: expected datatype {}, got length {}",
                                self,
                                l.len()
                            ),
                        }
                    );
                    DataValue::List(
                        l.into_iter()
                            .zip(typ.iter())
                            .map(|(el, t)| t.coerce(el, cur_vld))
                            .try_collect()?,
                    )
                } else {
                    return Err(make_err());
                }
            }
            ColType::Validity => match data {
                vld @ DataValue::Validity(_) => vld,
                DataValue::Str(s) => match &s as &str {
                    "ASSERT" => DataValue::Validity(Validity {
                        timestamp: cur_vld,
                        is_assert: Reverse(true),
                    }),
                    "RETRACT" => DataValue::Validity(Validity {
                        timestamp: cur_vld,
                        is_assert: Reverse(false),
                    }),
                    s => {
                        let (is_assert, ts_str) = match s.strip_prefix('~') {
                            None => (true, s),
                            Some(remaining) => (false, remaining),
                        };
                        let ts: Timestamp = ts_str.parse().map_err(|_| {
                            BadTimeSnafu {
                                message: format!(
                                    "{} cannot be coerced into validity",
                                    DataValue::Str(s.into())
                                ),
                            }
                            .build()
                        })?;
                        let microseconds = ts.as_microsecond();

                        if microseconds == i64::MAX || microseconds == i64::MIN {
                            return BadTimeSnafu {
                                message: format!(
                                    "{} cannot be coerced into validity",
                                    DataValue::Str(s.into())
                                ),
                            }
                            .fail();
                        }

                        DataValue::Validity(Validity {
                            timestamp: ValidityTs(Reverse(microseconds)),
                            is_assert: Reverse(is_assert),
                        })
                    }
                },
                DataValue::List(l) => {
                    if l.len() == 2 {
                        let o_ts = l[0].get_int();
                        let o_is_assert = l[1].get_bool();
                        if let (Some(ts), Some(is_assert)) = (o_ts, o_is_assert) {
                            if ts == i64::MAX || ts == i64::MIN {
                                return BadTimeSnafu {
                                    message: format!(
                                        "{} cannot be coerced into validity",
                                        DataValue::List(l)
                                    ),
                                }
                                .fail();
                            }
                            return Ok(DataValue::Validity(Validity {
                                timestamp: ValidityTs(Reverse(ts)),
                                is_assert: Reverse(is_assert),
                            }));
                        }
                    }
                    return BadTimeSnafu {
                        message: format!("{} cannot be coerced into validity", DataValue::List(l)),
                    }
                    .fail();
                }
                v => {
                    return BadTimeSnafu {
                        message: format!("{v} cannot be coerced into validity"),
                    }
                    .fail();
                }
            },
            ColType::Json => DataValue::Json(JsonData(match data {
                DataValue::Null => {
                    json!(null)
                }
                DataValue::Bool(b) => {
                    json!(b)
                }
                DataValue::Num(n) => match n {
                    Num::Int(i) => {
                        json!(i)
                    }
                    Num::Float(f) => {
                        json!(f)
                    }
                },
                DataValue::Str(s) => {
                    json!(s)
                }
                DataValue::Bytes(b) => {
                    json!(b)
                }
                DataValue::Uuid(u) => {
                    json!(u.0.as_bytes())
                }
                DataValue::Regex(r) => {
                    json!(r.0.as_str())
                }
                DataValue::List(l) => {
                    let mut arr = Vec::with_capacity(l.len());
                    for el in l {
                        arr.push(self.coerce(el, cur_vld)?);
                    }
                    arr.into()
                }
                DataValue::Set(l) => {
                    let mut arr = Vec::with_capacity(l.len());
                    for el in l {
                        arr.push(self.coerce(el, cur_vld)?);
                    }
                    arr.into()
                }
                DataValue::Vec(v) => {
                    let mut arr = Vec::with_capacity(v.len());
                    match v {
                        Vector::F32(a) => {
                            for el in a {
                                arr.push(json!(el));
                            }
                        }
                        Vector::F64(a) => {
                            for el in a {
                                arr.push(json!(el));
                            }
                        }
                    }
                    arr.into()
                }
                DataValue::Json(j) => j.0,
                DataValue::Validity(vld) => {
                    json!([vld.timestamp.0, vld.is_assert.0])
                }
                DataValue::Bot => {
                    json!(null)
                }
            })),
        })
    }
}
