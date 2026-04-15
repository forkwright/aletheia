//! Collection, set, range, and assertion functions.
#![expect(clippy::as_conversions, reason = "collection functions require i64/usize casts — engine-internal")]
#![expect(clippy::mutable_key_type, reason = "DataValue contains interior-mutable Regex; BTreeSet usage is engine-internal")]
use std::collections::BTreeSet;

use itertools::Itertools;

use super::arg;
use crate::data::error::*;
type Result<T> = DataResult<T>;
use crate::data::value::DataValue;

use super::aggregate::get_index;

/// Set union of one or more lists.
///
/// # Contract
/// All arguments must be lists or sets. Returns a list of unique elements
/// in sorted order.
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

/// Set difference: first list minus elements in subsequent lists.
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

/// Set intersection of one or more lists.
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

/// Length of a list, set, string, byte sequence, or vector.
pub(crate) fn op_length(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::from(match arg(args, 0)? {
        DataValue::Set(s) => {
            #[expect(clippy::cast_possible_wrap, reason = "length fits i64")]
            let len = s.len() as i64;
            len
        }
        DataValue::List(l) => {
            #[expect(clippy::cast_possible_wrap, reason = "length fits i64")]
            let len = l.len() as i64;
            len
        }
        DataValue::Str(s) => {
            #[expect(clippy::cast_possible_wrap, reason = "length fits i64")]
            let len = s.chars().count() as i64;
            len
        }
        DataValue::Bytes(b) => {
            #[expect(clippy::cast_possible_wrap, reason = "length fits i64")]
            let len = b.len() as i64;
            len
        }
        DataValue::Vec(v) => {
            #[expect(clippy::cast_possible_wrap, reason = "length fits i64")]
            let len = v.len() as i64;
            len
        }
        _ => {
            return TypeMismatchSnafu {
                op: "length",
                expected: "lists",
            }
            .fail();
        }
    }))
}

/// First element of a list, or `Null` if empty.
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

/// Last element of a list, or `Null` if empty.
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

/// Return a sorted copy of a list.
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

/// Return a reversed copy of a list.
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

/// Split a list into chunks of size `n` (last chunk may be shorter).
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

/// Split a list into exact chunks of size `n` (remainder discarded).
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

/// Sliding windows of size `n` over a list.
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

/// Extract a sub-list from index `m` (inclusive) to `n` (exclusive).
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
            .ok_or_else(|| {
                IndexOutOfBoundsSnafu {
                    index: i64::try_from(n).unwrap_or(i64::MAX),
                }
                .build()
            })?
            .to_vec(),
    ))
}

/// Generate a list of integers. Supports 1-3 arguments: `(end)`, `(start, end)`, `(start, end, step)`.
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

/// Assert that the first argument is `true`. Returns an error otherwise.
pub(crate) fn op_assert(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Bool(true) => Ok(DataValue::from(true)),
        _ => AssertionFailedSnafu {
            message: format!("{args:?}"),
        }
        .fail(),
    }
}
