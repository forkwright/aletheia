//! Relation metadata and schema definitions.
use crate::engine::error::DbResult as Result;
use crate::{bail, ensure};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use compact_str::CompactString;
use itertools::Itertools;
use jiff::Timestamp;
use serde_json::json;
use std::cmp::Reverse;
use std::fmt::{Display, Formatter};
use std::mem;

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
    pub(crate) fn satisfied_by_required_col(&self, col: &ColumnDef) -> Result<()> {
        for target in self.keys.iter().chain(self.non_keys.iter()) {
            if target.name == col.name {
                return Ok(());
            }
        }
        if col.default_gen.is_none() {
            #[derive(Debug)]
            struct ColumnNotProvided(String);

            impl std::fmt::Display for ColumnNotProvided {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "required column {} not provided by input", self.0)
                }
            }

            impl std::error::Error for ColumnNotProvided {}

            bail!(ColumnNotProvided(col.name.to_string()))
        }
        Ok(())
    }
    pub(crate) fn compatible_with_col(&self, col: &ColumnDef) -> Result<()> {
        for target in self.keys.iter().chain(self.non_keys.iter()) {
            if target.name == col.name {
                #[derive(Debug)]
                struct IncompatibleTyping(String, NullableColType, NullableColType);

                impl std::fmt::Display for IncompatibleTyping {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(
                            f,
                            "requested column {} has typing {}, but the requested typing is {}",
                            self.0, self.1, self.2
                        )
                    }
                }

                impl std::error::Error for IncompatibleTyping {}

                if (!col.typing.nullable || col.typing.coltype != ColType::Any)
                    && target.typing != col.typing
                {
                    bail!(IncompatibleTyping(
                        col.name.to_string(),
                        target.typing.clone(),
                        col.typing.clone()
                    ))
                }

                return Ok(());
            }
        }

        #[derive(Debug)]
        struct ColumnNotFound(String);

        impl std::fmt::Display for ColumnNotFound {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "required column {} not found", self.0)
            }
        }

        impl std::error::Error for ColumnNotFound {}

        bail!(ColumnNotFound(col.name.to_string()))
    }
}

impl NullableColType {
    #[expect(clippy::map_err_ignore, reason = "error context preserved in returned error type")]
    pub(crate) fn coerce(&self, data: DataValue, cur_vld: ValidityTs) -> Result<DataValue> {
        if matches!(data, DataValue::Null) {
            return if self.nullable {
                Ok(data)
            } else {
                #[derive(Debug)]
                struct InvalidNullValue(NullableColType);

                impl std::fmt::Display for InvalidNullValue {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "encountered null value for non-null type {}", self.0)
                    }
                }

                impl std::error::Error for InvalidNullValue {}

                Err(InvalidNullValue(self.clone()).into())
            };
        }

        #[derive(Debug)]
        struct DataCoercionFailed(NullableColType, DataValue);

        impl std::fmt::Display for DataCoercionFailed {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    f,
                    "data coercion failed: expected type {}, got value {:?}",
                    self.0, self.1
                )
            }
        }

        impl std::error::Error for DataCoercionFailed {}

        #[derive(Debug)]
        struct BadListLength(NullableColType, usize);

        impl std::fmt::Display for BadListLength {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(
                    f,
                    "bad list length: expected datatype {}, got length {}",
                    self.0, self.1
                )
            }
        }

        impl std::error::Error for BadListLength {}

        let make_err = || DataCoercionFailed(self.clone(), data.clone());

        Ok(match &self.coltype {
            ColType::Any => match data {
                DataValue::Set(s) => DataValue::List(s.into_iter().collect_vec()),
                DataValue::Bot => {
                    #[derive(Debug)]
                    struct DataCoercionFromBot;

                    impl std::fmt::Display for DataCoercionFromBot {
                        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                            write!(f, "data coercion failed: internal type Bot not allowed")
                        }
                    }

                    impl std::error::Error for DataCoercionFromBot {}

                    bail!(DataCoercionFromBot)
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
                    bail!(make_err())
                }
            }
            ColType::Bytes => match data {
                d @ DataValue::Bytes(_) => d,
                DataValue::Str(s) => {
                    #[derive(Debug)]
                    struct BadBase64EncodedString(String);

                    impl std::fmt::Display for BadBase64EncodedString {
                        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                            write!(
                                f,
                                "cannot decode string as base64-encoded bytes: {}",
                                self.0
                            )
                        }
                    }

                    impl std::error::Error for BadBase64EncodedString {}

                    let b = STANDARD
                        .decode(s)
                        .map_err(|e| BadBase64EncodedString(e.to_string()))?;
                    DataValue::Bytes(b)
                }
                _ => bail!(make_err()),
            },
            ColType::Uuid => DataValue::Uuid(UuidWrapper(data.get_uuid().ok_or_else(make_err)?)),
            ColType::List { eltype, len } => {
                if let DataValue::List(l) = data {
                    if let Some(expected) = len {
                        ensure!(*expected == l.len(), BadListLength(self.clone(), l.len()))
                    }
                    DataValue::List(
                        l.into_iter()
                            .map(|el| eltype.coerce(el, cur_vld))
                            .try_collect()?,
                    )
                } else {
                    bail!(make_err())
                }
            }
            ColType::Vec { eltype, len } => match &data {
                DataValue::List(l) => {
                    if l.len() != *len {
                        bail!(BadListLength(self.clone(), l.len()))
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
                        bail!(make_err())
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
                                bail!(make_err())
                            }
                            // SAFETY: `bytes` is a base64-decoded f32 vector payload.
                            // The length check above ensures `f32_count == *len`, so
                            // the pointer covers exactly `f32_count` f32-aligned elements
                            // within the live `bytes` allocation. The view is immediately
                            // converted to an owned array before `bytes` is dropped.
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
                                bail!(make_err())
                            }
                            // SAFETY: `bytes` is a base64-decoded f64 vector payload.
                            // The length check above ensures `f64_count == *len`, so
                            // the pointer covers exactly `f64_count` f64-aligned elements
                            // within the live `bytes` allocation. The view is immediately
                            // converted to an owned array before `bytes` is dropped.
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
                _ => bail!(make_err()),
            },
            ColType::Tuple(typ) => {
                if let DataValue::List(l) = data {
                    ensure!(typ.len() == l.len(), BadListLength(self.clone(), l.len()));
                    DataValue::List(
                        l.into_iter()
                            .zip(typ.iter())
                            .map(|(el, t)| t.coerce(el, cur_vld))
                            .try_collect()?,
                    )
                } else {
                    bail!(make_err())
                }
            }
            ColType::Validity => {
                #[derive(Debug)]
                struct InvalidValidity(DataValue);

                impl std::fmt::Display for InvalidValidity {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "{} cannot be coerced into validity", self.0)
                    }
                }

                impl std::error::Error for InvalidValidity {}

                match data {
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
                            let ts: Timestamp = ts_str
                                .parse()
                                .map_err(|_| InvalidValidity(DataValue::Str(s.into())))?;
                            let microseconds = ts.as_microsecond();

                            if microseconds == i64::MAX || microseconds == i64::MIN {
                                bail!(InvalidValidity(DataValue::Str(s.into())))
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
                                    bail!(InvalidValidity(DataValue::List(l)))
                                }
                                return Ok(DataValue::Validity(Validity {
                                    timestamp: ValidityTs(Reverse(ts)),
                                    is_assert: Reverse(is_assert),
                                }));
                            }
                        }
                        bail!(InvalidValidity(DataValue::List(l)))
                    }
                    v => bail!(InvalidValidity(v)),
                }
            }
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
