//! List construction, concatenation, get/set, and JSON merge functions.
#![expect(
    clippy::as_conversions,
    reason = "numeric aggregation requires i64/usize/f64 casts — values are engine-internal"
)]
#![expect(
    clippy::unnecessary_wraps,
    reason = "op_list and op_maybe_get return Result for API consistency with other builtins"
)]
#![expect(
    clippy::redundant_else,
    reason = "else after early return keeps the fallback visually grouped"
)]

use itertools::Itertools;
use serde_json::{Value, json};

use crate::data::error::*;
type Result<T> = DataResult<T>;
use crate::data::json::JsonValue;
use crate::data::value::{DataValue, JsonData};

use super::arg;

/// Deep-merge two JSON values: objects merge recursively, arrays concatenate,
/// otherwise the second value wins.
pub(crate) fn deep_merge_json(value1: JsonValue, value2: JsonValue) -> JsonValue {
    match (value1, value2) {
        (JsonValue::Object(mut obj1), JsonValue::Object(obj2)) => {
            for (key, value2) in obj2 {
                let value1 = obj1.remove(&key);
                obj1.insert(key, deep_merge_json(value1.unwrap_or(Value::Null), value2));
            }
            JsonValue::Object(obj1)
        }
        (JsonValue::Array(mut arr1), JsonValue::Array(arr2)) => {
            arr1.extend(arr2);
            JsonValue::Array(arr1)
        }
        (_, value2) => value2,
    }
}

/// Resolve a possibly-negative index against a total length.
///
/// Negative indices count from the end. Returns an error if out of bounds.
/// `is_upper` controls whether `index == total` is valid (for exclusive upper bounds).
pub(crate) fn get_index(mut i: i64, total: usize, is_upper: bool) -> Result<usize> {
    if i < 0 {
        #[expect(clippy::cast_possible_wrap, reason = "length fits i64")]
        let total_i64 = total as i64;
        i += total_i64;
    }
    Ok(if i >= 0 {
        let i = usize::try_from(i).map_err(|_e| IndexOutOfBoundsSnafu { index: i }.build())?;
        if i > total || (!is_upper && i == total) {
            return IndexOutOfBoundsSnafu {
                index: i64::try_from(i).unwrap_or(i64::MAX),
            }
            .fail();
        } else {
            i
        }
    } else {
        return IndexOutOfBoundsSnafu { index: i }.fail();
    })
}

/// Convert a `serde_json` [`Value`] to a [`DataValue`], wrapping arrays and objects as JSON.
pub(crate) fn json2val(res: Value) -> DataValue {
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

fn get_json_path_immutable_local<'a>(
    mut pointer: &'a JsonValue,
    path: &[DataValue],
) -> Result<&'a JsonValue> {
    use super::utility::val2str;
    for key in path {
        match pointer {
            JsonValue::Object(obj) => {
                let key = val2str(key);
                let entry = obj.get(&key).ok_or_else(|| {
                    JsonPathSnafu {
                        message: "json path does not exist",
                    }
                    .build()
                })?;
                pointer = entry;
            }
            JsonValue::Array(arr) => {
                let key = key.get_int().ok_or_else(|| {
                    JsonPathSnafu {
                        message: "json path must be a string or a number",
                    }
                    .build()
                })?;
                let key = usize::try_from(key).map_err(|_e| {
                    JsonPathSnafu {
                        message: "json array index out of range",
                    }
                    .build()
                })?;

                let val = arr.get(key).ok_or_else(|| {
                    JsonPathSnafu {
                        message: "json path does not exist",
                    }
                    .build()
                })?;
                pointer = val;
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

/// Get element by index (list) or key/path (JSON). Used by `op_get` and `op_maybe_get`.
pub(crate) fn get_impl(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::List(l) => {
            let n = arg(args, 1)?.get_int().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "get",
                    expected: "an integer as second argument",
                }
                .build()
            })?;
            let idx = get_index(n, l.len(), false)?;
            Ok(l.get(idx)
                .ok_or_else(|| {
                    IndexOutOfBoundsSnafu {
                        index: i64::try_from(idx).unwrap_or(i64::MAX),
                    }
                    .build()
                })?
                .clone())
        }
        DataValue::Json(json) => {
            let res = match arg(args, 1)? {
                DataValue::Str(s) => json
                    .get(s as &str)
                    .ok_or_else(|| {
                        InvalidValueSnafu {
                            message: format!("key '{s}' not found in json"),
                        }
                        .build()
                    })?
                    .clone(),
                DataValue::Num(i) => {
                    let i = i.get_int().ok_or_else(|| {
                        InvalidValueSnafu {
                            message: format!("index '{i}' not found in json"),
                        }
                        .build()
                    })?;
                    let idx = usize::try_from(i).map_err(|_e| {
                        InvalidValueSnafu {
                            message: format!("index '{i}' out of range for json array"),
                        }
                        .build()
                    })?;
                    json.get(idx)
                        .ok_or_else(|| {
                            InvalidValueSnafu {
                                message: format!("index '{i}' not found in json"),
                            }
                            .build()
                        })?
                        .clone()
                }
                DataValue::List(l) => get_json_path_immutable_local(json, l)?.clone(),
                _ => {
                    return TypeMismatchSnafu {
                        op: "get",
                        expected: "a string or integer as second argument",
                    }
                    .fail();
                }
            };
            let res = json2val(res);
            Ok(res)
        }
        _ => TypeMismatchSnafu {
            op: "get",
            expected: "a list or json as first argument",
        }
        .fail(),
    }
}

/// Construct a list from all arguments.
pub(crate) fn op_list(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::List(args.to_vec()))
}

/// Concatenate strings, lists, or JSON objects.
pub(crate) fn op_concat(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Str(_) => {
            let mut ret = String::new();
            for arg in args {
                if let DataValue::Str(s) = arg {
                    ret += s;
                } else {
                    return TypeMismatchSnafu {
                        op: "concat",
                        expected: "strings, or lists",
                    }
                    .fail();
                }
            }
            Ok(DataValue::from(ret))
        }
        DataValue::List(_) | DataValue::Set(_) => {
            let mut ret = vec![];
            for arg in args {
                if let DataValue::List(l) = arg {
                    ret.extend_from_slice(l);
                } else if let DataValue::Set(s) = arg {
                    ret.extend(s.iter().cloned());
                } else {
                    return TypeMismatchSnafu {
                        op: "concat",
                        expected: "strings, or lists",
                    }
                    .fail();
                }
            }
            Ok(DataValue::List(ret))
        }
        DataValue::Json(_) => {
            let mut ret = json!(null);
            for arg in args {
                if let DataValue::Json(j) = arg {
                    ret = deep_merge_json(ret, j.0.clone());
                } else {
                    return TypeMismatchSnafu {
                        op: "concat",
                        expected: "strings, lists, or JSON objects",
                    }
                    .fail();
                }
            }
            Ok(DataValue::Json(JsonData(ret)))
        }
        _ => TypeMismatchSnafu {
            op: "concat",
            expected: "strings, lists, or JSON objects",
        }
        .fail(),
    }
}

/// Append an element to the end of a list.
pub(crate) fn op_append(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::List(l) => {
            let mut l = l.clone();
            l.push(arg(args, 1)?.clone());
            Ok(DataValue::List(l))
        }
        DataValue::Set(l) => {
            let mut l = l.iter().cloned().collect_vec();
            l.push(arg(args, 1)?.clone());
            Ok(DataValue::List(l))
        }
        _ => TypeMismatchSnafu {
            op: "append",
            expected: "first argument to be a list",
        }
        .fail(),
    }
}

/// Prepend an element to the beginning of a list.
pub(crate) fn op_prepend(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::List(pl) => {
            let mut l = vec![arg(args, 1)?.clone()];
            l.extend_from_slice(pl);
            Ok(DataValue::List(l))
        }
        DataValue::Set(pl) => {
            let mut l = vec![arg(args, 1)?.clone()];
            l.extend(pl.iter().cloned());
            Ok(DataValue::List(l))
        }
        _ => TypeMismatchSnafu {
            op: "prepend",
            expected: "first argument to be a list",
        }
        .fail(),
    }
}

/// Get element with fallback default on error.
pub(crate) fn op_get(args: &[DataValue]) -> Result<DataValue> {
    match get_impl(args) {
        Ok(res) => Ok(res),
        Err(err) => {
            if let Some(default) = args.get(2) {
                Ok(default.clone())
            } else {
                Err(err)
            }
        }
    }
}

/// Get element, returning `Null` on any error (missing key, out of bounds, etc.).
pub(crate) fn op_maybe_get(args: &[DataValue]) -> Result<DataValue> {
    match get_impl(args) {
        Ok(res) => Ok(res),
        Err(_) => Ok(DataValue::Null),
    }
}
