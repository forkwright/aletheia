//! Type checking, conversion, and JSON operation functions.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]

use std::str::FromStr;

use serde_json::{Value, json};
use snafu::ResultExt;

use super::arg;
use crate::data::error::*;
type Result<T> = DataResult<T>;
use crate::data::json::JsonValue;
use crate::data::value::{DataValue, JsonData, Num, Vector};

pub(crate) fn to_json(d: &DataValue) -> JsonValue {
    match d {
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
                arr.push(to_json(el));
            }
            arr.into()
        }
        DataValue::Set(l) => {
            let mut arr = Vec::with_capacity(l.len());
            for el in l {
                arr.push(to_json(el));
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
        DataValue::Json(j) => j.0.clone(),
        DataValue::Validity(vld) => {
            json!([vld.timestamp.0, vld.is_assert.0])
        }
        DataValue::Bot => {
            json!(null)
        }
    }
}

pub(crate) fn get_json_path<'a>(
    mut pointer: &'a mut JsonValue,
    path: &[DataValue],
) -> Result<&'a mut JsonValue> {
    for key in path {
        match pointer {
            JsonValue::Object(obj) => {
                let key = val2str(key);
                let entry = obj.entry(key).or_insert(json!({}));
                pointer = entry;
            }
            JsonValue::Array(arr) => {
                let key = key.get_int().ok_or_else(|| {
                    JsonPathSnafu {
                        message: "json path must be a string or a number",
                    }
                    .build()
                })? as usize;
                if arr.len() <= key + 1 {
                    arr.resize_with(key + 1, || JsonValue::Null);
                }

                pointer = &mut arr[key];
            }
            _ => {
                return JsonPathSnafu {
                    message: "json path does not exist",
                }
                .fail();
            }
        }
    }
    Ok(pointer)
}

pub(crate) fn val2str(arg: &DataValue) -> String {
    match arg {
        DataValue::Str(s) => s.to_string(),
        DataValue::Json(JsonData(JsonValue::String(s))) => s.clone(),
        v => {
            let jv = to_json(v);
            jv.to_string()
        }
    }
}

fn json2val_local(res: Value) -> DataValue {
    use compact_str::CompactString;
    match res {
        Value::Null => DataValue::Null,
        Value::Bool(b) => DataValue::Bool(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                DataValue::from(i)
            } else if let Some(f) = n.as_f64() {
                DataValue::from(f)
            } else {
                DataValue::Null
            }
        }
        Value::String(s) => DataValue::Str(CompactString::from(s)),
        Value::Array(arr) => DataValue::Json(JsonData(json!(arr))),
        Value::Object(obj) => DataValue::Json(JsonData(json!(obj))),
    }
}

pub(crate) fn op_is_null(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(arg(args, 0)?, DataValue::Null)))
}

pub(crate) fn op_is_int(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(
        arg(args, 0)?,
        DataValue::Num(Num::Int(_))
    )))
}

pub(crate) fn op_is_float(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(
        arg(args, 0)?,
        DataValue::Num(Num::Float(_))
    )))
}

pub(crate) fn op_is_num(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(
        arg(args, 0)?,
        DataValue::Num(Num::Int(_)) | DataValue::Num(Num::Float(_))
    )))
}

pub(crate) fn op_is_finite(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match arg(args, 0)? {
        DataValue::Num(Num::Int(_)) => true,
        DataValue::Num(Num::Float(f)) => f.is_finite(),
        _ => false,
    }))
}

pub(crate) fn op_is_infinite(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match arg(args, 0)? {
        DataValue::Num(Num::Float(f)) => f.is_infinite(),
        _ => false,
    }))
}

pub(crate) fn op_is_nan(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match arg(args, 0)? {
        DataValue::Num(Num::Float(f)) => f.is_nan(),
        _ => false,
    }))
}

pub(crate) fn op_is_string(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(arg(args, 0)?, DataValue::Str(_))))
}

pub(crate) fn op_is_list(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(
        arg(args, 0)?,
        DataValue::List(_) | DataValue::Set(_)
    )))
}

pub(crate) fn op_is_vec(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(arg(args, 0)?, DataValue::Vec(_))))
}

pub(crate) fn op_is_bytes(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(
        arg(args, 0)?,
        DataValue::Bytes(_)
    )))
}

pub(crate) fn op_is_uuid(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(arg(args, 0)?, DataValue::Uuid(_))))
}

pub(crate) fn op_is_json(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(matches!(arg(args, 0)?, DataValue::Json(_))))
}

pub(crate) fn op_to_bool(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match arg(args, 0)? {
        DataValue::Null => false,
        DataValue::Bool(b) => *b,
        DataValue::Num(n) => n.get_int() != Some(0),
        DataValue::Str(s) => !s.is_empty(),
        DataValue::Bytes(b) => !b.is_empty(),
        DataValue::Uuid(u) => !u.0.is_nil(),
        DataValue::Regex(r) => !r.0.as_str().is_empty(),
        DataValue::List(l) => !l.is_empty(),
        DataValue::Set(s) => !s.is_empty(),
        DataValue::Vec(_) => true,
        DataValue::Validity(vld) => vld.is_assert.0,
        DataValue::Bot => false,
        DataValue::Json(json) => match &json.0 {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_i64() != Some(0),
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
        },
    }))
}

pub(crate) fn op_to_unity(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match arg(args, 0)? {
        DataValue::Null => 0,
        DataValue::Bool(b) => *b as i64,
        DataValue::Num(n) => (n.get_float() != 0.) as i64,
        DataValue::Str(s) => i64::from(!s.is_empty()),
        DataValue::Bytes(b) => i64::from(!b.is_empty()),
        DataValue::Uuid(u) => i64::from(!u.0.is_nil()),
        DataValue::Regex(r) => i64::from(!r.0.as_str().is_empty()),
        DataValue::List(l) => i64::from(!l.is_empty()),
        DataValue::Set(s) => i64::from(!s.is_empty()),
        DataValue::Vec(_) => 1,
        DataValue::Validity(vld) => i64::from(vld.is_assert.0),
        DataValue::Bot => 0,
        DataValue::Json(json) => match &json.0 {
            Value::Null => 0,
            Value::Bool(b) => *b as i64,
            Value::Number(n) => (n.as_i64() != Some(0)) as i64,
            Value::String(s) => !s.is_empty() as i64,
            Value::Array(a) => !a.is_empty() as i64,
            Value::Object(o) => !o.is_empty() as i64,
        },
    }))
}

pub(crate) fn op_to_int(args: &[DataValue]) -> Result<DataValue> {
    Ok(match arg(args, 0)? {
        DataValue::Num(n) => match n.get_int() {
            None => {
                let f = n.get_float();
                DataValue::Num(Num::Int(f as i64))
            }
            Some(i) => DataValue::Num(Num::Int(i)),
        },
        DataValue::Null => DataValue::from(0),
        DataValue::Bool(b) => DataValue::from(if *b { 1 } else { 0 }),
        DataValue::Str(t) => {
            let s = t as &str;
            i64::from_str(s)
                .map_err(|e| {
                    ParseFailedSnafu {
                        target: format!("int: {e}"),
                    }
                    .build()
                })?
                .into()
        }
        DataValue::Validity(vld) => DataValue::Num(Num::Int(vld.timestamp.0.0)),
        v => {
            return TypeMismatchSnafu {
                op: "to_int",
                expected: format!("recognized type, got {v:?}"),
            }
            .fail();
        }
    })
}

pub(crate) fn op_to_float(args: &[DataValue]) -> Result<DataValue> {
    Ok(match arg(args, 0)? {
        DataValue::Num(n) => n.get_float().into(),
        DataValue::Null => DataValue::from(0.0),
        DataValue::Bool(b) => DataValue::from(if *b { 1.0 } else { 0.0 }),
        DataValue::Str(t) => match t as &str {
            "PI" => std::f64::consts::PI.into(),
            "E" => std::f64::consts::E.into(),
            "NAN" => f64::NAN.into(),
            "INF" => f64::INFINITY.into(),
            "NEG_INF" => f64::NEG_INFINITY.into(),
            s => f64::from_str(s)
                .map_err(|e| {
                    ParseFailedSnafu {
                        target: format!("float: {e}"),
                    }
                    .build()
                })?
                .into(),
        },
        v => {
            return TypeMismatchSnafu {
                op: "to_float",
                expected: format!("recognized type, got {v:?}"),
            }
            .fail();
        }
    })
}

pub(crate) fn op_to_string(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::Str(val2str(arg(args, 0)?).into()))
}

pub(crate) fn op_to_uuid(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        d @ DataValue::Uuid(_u) => Ok(d.clone()),
        DataValue::Str(s) => {
            let id = uuid::Uuid::try_parse(s).map_err(|e| {
                ParseFailedSnafu {
                    target: format!("UUID: {e}"),
                }
                .build()
            })?;
            Ok(DataValue::uuid(id))
        }
        _ => TypeMismatchSnafu {
            op: "to_uuid",
            expected: "a string",
        }
        .fail(),
    }
}

pub(crate) fn op_json_to_scalar(args: &[DataValue]) -> Result<DataValue> {
    Ok(match arg(args, 0)? {
        DataValue::Json(JsonData(j)) => json2val_local(j.clone()),
        d => d.clone(),
    })
}

pub(crate) fn op_json(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::Json(JsonData(to_json(arg(args, 0)?))))
}

pub(crate) fn op_json_object(args: &[DataValue]) -> Result<DataValue> {
    snafu::ensure!(
        args.len().is_multiple_of(2),
        InvalidValueSnafu {
            message: "json_object requires an even number of arguments"
        }
    );
    let mut obj = serde_json::Map::with_capacity(args.len() / 2);
    for pair in args.chunks_exact(2) {
        #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
        let key = val2str(&pair[0]);
        #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
        let value = to_json(&pair[1]);
        obj.insert(key.to_string(), value);
    }
    Ok(DataValue::Json(JsonData(Value::Object(obj))))
}

pub(crate) fn op_parse_json(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)?.get_str() {
        Some(s) => {
            let value: serde_json::Value = serde_json::from_str(s).context(JsonSnafu)?;
            Ok(DataValue::Json(JsonData(value)))
        }
        None => TypeMismatchSnafu {
            op: "parse_json",
            expected: "a string",
        }
        .fail(),
    }
}

pub(crate) fn op_dump_json(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Json(j) => Ok(DataValue::Str(j.0.to_string().into())),
        _ => TypeMismatchSnafu {
            op: "dump_json",
            expected: "a json value",
        }
        .fail(),
    }
}

pub(crate) fn op_set_json_path(args: &[DataValue]) -> Result<DataValue> {
    let mut result = to_json(arg(args, 0)?);
    let path = arg(args, 1)?.get_slice().ok_or_else(|| {
        JsonPathSnafu {
            message: "json path must be a string",
        }
        .build()
    })?;
    let pointer = get_json_path(&mut result, path)?;
    let new_val = to_json(arg(args, 2)?);
    *pointer = new_val;
    Ok(DataValue::Json(JsonData(result)))
}

pub(crate) fn op_remove_json_path(args: &[DataValue]) -> Result<DataValue> {
    let mut result = to_json(arg(args, 0)?);
    let path = arg(args, 1)?.get_slice().ok_or_else(|| {
        JsonPathSnafu {
            message: "json path must be a string",
        }
        .build()
    })?;
    let (last, path) = path.split_last().ok_or_else(|| {
        JsonPathSnafu {
            message: "json path must not be empty",
        }
        .build()
    })?;
    let pointer = get_json_path(&mut result, path)?;
    match pointer {
        JsonValue::Object(obj) => {
            let key = val2str(last);
            obj.remove(&key);
        }
        JsonValue::Array(arr) => {
            let key = last.get_int().ok_or_else(|| {
                JsonPathSnafu {
                    message: "json path must be a string or a number",
                }
                .build()
            })? as usize;
            arr.remove(key);
        }
        _ => {
            return JsonPathSnafu {
                message: "json path does not exist",
            }
            .fail();
        }
    }
    Ok(DataValue::Json(JsonData(result)))
}
