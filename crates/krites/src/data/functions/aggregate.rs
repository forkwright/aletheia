//! List, collection, set operations, and range functions.
#![expect(
    clippy::as_conversions,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeSet;

use itertools::Itertools;
use serde_json::{Value, json};

use crate::data::error::*;
type Result<T> = DataResult<T>;
use crate::data::json::JsonValue;
use crate::data::value::{DataValue, JsonData};

use super::arg;

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

pub(crate) fn get_index(mut i: i64, total: usize, is_upper: bool) -> Result<usize> {
    if i < 0 {
        i += total as i64;
    }
    Ok(if i >= 0 {
        let i = usize::try_from(i).map_err(|_e| IndexOutOfBoundsSnafu { index: i }.build())?;
        if i > total || (!is_upper && i == total) {
            return IndexOutOfBoundsSnafu { index: i as i64 }.fail();
        } else {
            i
        }
    } else {
        return IndexOutOfBoundsSnafu { index: i }.fail();
    })
}

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
                .ok_or_else(|| IndexOutOfBoundsSnafu { index: idx as i64 }.build())?
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

pub(crate) fn op_list(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::List(args.to_vec()))
}

pub(crate) fn op_concat(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Str(_) => {
            let mut ret: String = Default::default();
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

pub(crate) fn op_union(args: &[DataValue]) -> Result<DataValue> {
    let mut ret = BTreeSet::new();
    for arg in args {
        match arg {
            DataValue::List(l) => {
                ret.extend(l.iter().cloned());
            }
            DataValue::Set(s) => {
                ret.extend(s.iter().cloned());
            }
            _ => {
                return TypeMismatchSnafu {
                    op: "union",
                    expected: "lists",
                }
                .fail();
            }
        }
    }
    Ok(DataValue::List(ret.into_iter().collect()))
}

pub(crate) fn op_difference(args: &[DataValue]) -> Result<DataValue> {
    let mut start: BTreeSet<_> = match arg(args, 0)? {
        DataValue::List(l) => l.iter().cloned().collect(),
        DataValue::Set(s) => s.iter().cloned().collect(),
        _ => {
            return TypeMismatchSnafu {
                op: "difference",
                expected: "lists",
            }
            .fail();
        }
    };
    for arg in args.get(1..).unwrap_or_default() {
        match arg {
            DataValue::List(l) => {
                for el in l {
                    start.remove(el);
                }
            }
            DataValue::Set(s) => {
                for el in s {
                    start.remove(el);
                }
            }
            _ => {
                return TypeMismatchSnafu {
                    op: "difference",
                    expected: "lists",
                }
                .fail();
            }
        }
    }
    Ok(DataValue::List(start.into_iter().collect()))
}

pub(crate) fn op_intersection(args: &[DataValue]) -> Result<DataValue> {
    let mut start: BTreeSet<_> = match arg(args, 0)? {
        DataValue::List(l) => l.iter().cloned().collect(),
        DataValue::Set(s) => s.iter().cloned().collect(),
        _ => {
            return TypeMismatchSnafu {
                op: "intersection",
                expected: "lists",
            }
            .fail();
        }
    };
    for arg in args.get(1..).unwrap_or_default() {
        match arg {
            DataValue::List(l) => {
                let other: BTreeSet<_> = l.iter().cloned().collect();
                start = start.intersection(&other).cloned().collect();
            }
            DataValue::Set(s) => start = start.intersection(s).cloned().collect(),
            _ => {
                return TypeMismatchSnafu {
                    op: "intersection",
                    expected: "lists",
                }
                .fail();
            }
        }
    }
    Ok(DataValue::List(start.into_iter().collect()))
}

pub(crate) fn op_length(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match arg(args, 0)? {
        DataValue::Set(s) => s.len() as i64,
        DataValue::List(l) => l.len() as i64,
        DataValue::Str(s) => s.chars().count() as i64,
        DataValue::Bytes(b) => b.len() as i64,
        DataValue::Vec(v) => v.len() as i64,
        _ => {
            return TypeMismatchSnafu {
                op: "length",
                expected: "lists",
            }
            .fail();
        }
    }))
}

pub(crate) fn op_first(args: &[DataValue]) -> Result<DataValue> {
    Ok(arg(args, 0)?
        .get_slice()
        .ok_or_else(|| {
            TypeMismatchSnafu {
                op: "first",
                expected: "lists",
            }
            .build()
        })?
        .first()
        .cloned()
        .unwrap_or(DataValue::Null))
}

pub(crate) fn op_last(args: &[DataValue]) -> Result<DataValue> {
    Ok(arg(args, 0)?
        .get_slice()
        .ok_or_else(|| {
            TypeMismatchSnafu {
                op: "last",
                expected: "lists",
            }
            .build()
        })?
        .last()
        .cloned()
        .unwrap_or(DataValue::Null))
}

pub(crate) fn op_sorted(args: &[DataValue]) -> Result<DataValue> {
    let mut a = arg(args, 0)?
        .get_slice()
        .ok_or_else(|| {
            TypeMismatchSnafu {
                op: "sort",
                expected: "lists",
            }
            .build()
        })?
        .to_vec();
    a.sort();
    Ok(DataValue::List(a))
}

pub(crate) fn op_reverse(args: &[DataValue]) -> Result<DataValue> {
    let mut a = arg(args, 0)?
        .get_slice()
        .ok_or_else(|| {
            TypeMismatchSnafu {
                op: "reverse",
                expected: "lists",
            }
            .build()
        })?
        .to_vec();
    a.reverse();
    Ok(DataValue::List(a))
}

pub(crate) fn op_chunks(args: &[DataValue]) -> Result<DataValue> {
    let a = arg(args, 0)?.get_slice().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "chunks",
            expected: "a list as first argument",
        }
        .build()
    })?;
    let n = arg(args, 1)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "chunks",
            expected: "an integer as second argument",
        }
        .build()
    })?;
    snafu::ensure!(
        n > 0,
        InvalidValueSnafu {
            message: "second argument to 'chunks' must be positive"
        }
    );
    let n = usize::try_from(n).map_err(|_e| {
        InvalidValueSnafu {
            message: "second argument to 'chunks' out of range",
        }
        .build()
    })?;
    let res = a
        .chunks(n)
        .map(|el| DataValue::List(el.to_vec()))
        .collect_vec();
    Ok(DataValue::List(res))
}

pub(crate) fn op_chunks_exact(args: &[DataValue]) -> Result<DataValue> {
    let a = arg(args, 0)?.get_slice().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "chunks_exact",
            expected: "a list as first argument",
        }
        .build()
    })?;
    let n = arg(args, 1)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "chunks_exact",
            expected: "an integer as second argument",
        }
        .build()
    })?;
    snafu::ensure!(
        n > 0,
        InvalidValueSnafu {
            message: "second argument to 'chunks_exact' must be positive"
        }
    );
    let n = usize::try_from(n).map_err(|_e| {
        InvalidValueSnafu {
            message: "second argument to 'chunks_exact' out of range",
        }
        .build()
    })?;
    let res = a
        .chunks_exact(n)
        .map(|el| DataValue::List(el.to_vec()))
        .collect_vec();
    Ok(DataValue::List(res))
}

pub(crate) fn op_windows(args: &[DataValue]) -> Result<DataValue> {
    let a = arg(args, 0)?.get_slice().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "windows",
            expected: "a list as first argument",
        }
        .build()
    })?;
    let n = arg(args, 1)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "windows",
            expected: "an integer as second argument",
        }
        .build()
    })?;
    snafu::ensure!(
        n > 0,
        InvalidValueSnafu {
            message: "second argument to 'windows' must be positive"
        }
    );
    let n = usize::try_from(n).map_err(|_e| {
        InvalidValueSnafu {
            message: "second argument to 'windows' out of range",
        }
        .build()
    })?;
    let res = a
        .windows(n)
        .map(|el| DataValue::List(el.to_vec()))
        .collect_vec();
    Ok(DataValue::List(res))
}

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

pub(crate) fn op_maybe_get(args: &[DataValue]) -> Result<DataValue> {
    match get_impl(args) {
        Ok(res) => Ok(res),
        Err(_) => Ok(DataValue::Null),
    }
}

pub(crate) fn op_slice(args: &[DataValue]) -> Result<DataValue> {
    let l = arg(args, 0)?.get_slice().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "slice",
            expected: "a list as first argument",
        }
        .build()
    })?;
    let m = arg(args, 1)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "slice",
            expected: "an integer as second argument",
        }
        .build()
    })?;
    let n = arg(args, 2)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "slice",
            expected: "an integer as third argument",
        }
        .build()
    })?;
    let m = get_index(m, l.len(), false)?;
    let n = get_index(n, l.len(), true)?;
    Ok(DataValue::List(
        l.get(m..n)
            .ok_or_else(|| IndexOutOfBoundsSnafu { index: n as i64 }.build())?
            .to_vec(),
    ))
}

pub(crate) fn op_int_range(args: &[DataValue]) -> Result<DataValue> {
    let [start, end] = match args.len() {
        1 => {
            let end = arg(args, 0)?.get_int().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "int_range",
                    expected: "an integer for end",
                }
                .build()
            })?;
            [0, end]
        }
        2 => {
            let start = arg(args, 0)?.get_int().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "int_range",
                    expected: "an integer for start",
                }
                .build()
            })?;
            let end = arg(args, 1)?.get_int().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "int_range",
                    expected: "an integer for end",
                }
                .build()
            })?;
            [start, end]
        }
        3 => {
            let start = arg(args, 0)?.get_int().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "int_range",
                    expected: "an integer for start",
                }
                .build()
            })?;
            let end = arg(args, 1)?.get_int().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "int_range",
                    expected: "an integer for end",
                }
                .build()
            })?;
            let step = arg(args, 2)?.get_int().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "int_range",
                    expected: "an integer for step",
                }
                .build()
            })?;
            let mut current = start;
            let mut result = vec![];
            if step > 0 {
                while current < end {
                    result.push(DataValue::from(current));
                    current += step;
                }
            } else {
                while current > end {
                    result.push(DataValue::from(current));
                    current += step;
                }
            }
            return Ok(DataValue::List(result));
        }
        _ => {
            return TypeMismatchSnafu {
                op: "int_range",
                expected: "1 to 3 argument",
            }
            .fail();
        }
    };
    Ok(DataValue::List((start..end).map(DataValue::from).collect()))
}

pub(crate) fn op_assert(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Bool(true) => Ok(DataValue::from(true)),
        _ => AssertionFailedSnafu {
            message: format!("{args:?}"),
        }
        .fail(),
    }
}
