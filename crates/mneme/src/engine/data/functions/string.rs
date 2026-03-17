//! String manipulation, regex, unicode, and encoding functions.
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use compact_str::CompactString;
use itertools::Itertools;
use snafu::ResultExt;
use unicode_normalization::UnicodeNormalization;

use super::arg;
use crate::engine::data::error::*;
type Result<T> = DataResult<T>;
use crate::engine::data::value::{DataValue, RegexWrapper};

pub(crate) fn op_str_includes(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Str(l), DataValue::Str(r)) => Ok(DataValue::from(l.find(r as &str).is_some())),
        _ => TypeMismatchSnafu {
            op: "str_includes",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_lowercase(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Str(s) => Ok(DataValue::from(s.to_lowercase())),
        _ => TypeMismatchSnafu {
            op: "lowercase",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_uppercase(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Str(s) => Ok(DataValue::from(s.to_uppercase())),
        _ => TypeMismatchSnafu {
            op: "uppercase",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_trim(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Str(s) => Ok(DataValue::from(s.trim())),
        _ => TypeMismatchSnafu {
            op: "trim",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_trim_start(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Str(s) => Ok(DataValue::from(s.trim_start())),
        v => TypeMismatchSnafu {
            op: "trim_start",
            expected: format!("strings, got {v}"),
        }
        .fail(),
    }
}

pub(crate) fn op_trim_end(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Str(s) => Ok(DataValue::from(s.trim_end())),
        _ => TypeMismatchSnafu {
            op: "trim_end",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_starts_with(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Str(l), DataValue::Str(r)) => Ok(DataValue::from(l.starts_with(r as &str))),
        (DataValue::Bytes(l), DataValue::Bytes(r)) => {
            Ok(DataValue::from(l.starts_with(r as &[u8])))
        }
        _ => TypeMismatchSnafu {
            op: "starts_with",
            expected: "strings or bytes",
        }
        .fail(),
    }
}

pub(crate) fn op_ends_with(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Str(l), DataValue::Str(r)) => Ok(DataValue::from(l.ends_with(r as &str))),
        (DataValue::Bytes(l), DataValue::Bytes(r)) => Ok(DataValue::from(l.ends_with(r as &[u8]))),
        _ => TypeMismatchSnafu {
            op: "ends_with",
            expected: "strings or bytes",
        }
        .fail(),
    }
}

pub(crate) fn op_regex(args: &[DataValue]) -> Result<DataValue> {
    Ok(match arg(args, 0)? {
        r @ DataValue::Regex(_) => r.clone(),
        DataValue::Str(s) => DataValue::Regex(RegexWrapper(
            regex::Regex::new(s).context(InvalidRegexSnafu)?,
        )),
        _ => {
            return TypeMismatchSnafu {
                op: "regex",
                expected: "strings",
            }
            .fail();
        }
    })
}

pub(crate) fn op_regex_matches(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Str(s), DataValue::Regex(r)) => Ok(DataValue::from(r.0.is_match(s))),
        _ => TypeMismatchSnafu {
            op: "regex_matches",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_regex_replace(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?, arg(args, 2)?) {
        (DataValue::Str(s), DataValue::Regex(r), DataValue::Str(rp)) => {
            Ok(DataValue::Str(r.0.replace(s, rp as &str).into()))
        }
        _ => TypeMismatchSnafu {
            op: "regex_replace",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_regex_replace_all(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?, arg(args, 2)?) {
        (DataValue::Str(s), DataValue::Regex(r), DataValue::Str(rp)) => {
            Ok(DataValue::Str(r.0.replace_all(s, rp as &str).into()))
        }
        _ => TypeMismatchSnafu {
            op: "regex_replace",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_regex_extract(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Str(s), DataValue::Regex(r)) => {
            let found =
                r.0.find_iter(s)
                    .map(|v| DataValue::from(v.as_str()))
                    .collect_vec();
            Ok(DataValue::List(found))
        }
        _ => TypeMismatchSnafu {
            op: "regex_extract",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_regex_extract_first(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Str(s), DataValue::Regex(r)) => {
            let found = r.0.find(s).map(|v| DataValue::from(v.as_str()));
            Ok(found.unwrap_or(DataValue::Null))
        }
        _ => TypeMismatchSnafu {
            op: "regex_extract_first",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_unicode_normalize(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Str(s), DataValue::Str(n)) => Ok(DataValue::Str(match n as &str {
            "nfc" => s.nfc().collect(),
            "nfd" => s.nfd().collect(),
            "nfkc" => s.nfkc().collect(),
            "nfkd" => s.nfkd().collect(),
            u => {
                return InvalidValueSnafu {
                    message: format!("unknown normalization {u} for 'unicode_normalize'"),
                }
                .fail();
            }
        })),
        _ => TypeMismatchSnafu {
            op: "unicode_normalize",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_encode_base64(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Bytes(b) => {
            let s = STANDARD.encode(b);
            Ok(DataValue::from(s))
        }
        _ => TypeMismatchSnafu {
            op: "encode_base64",
            expected: "bytes",
        }
        .fail(),
    }
}

pub(crate) fn op_decode_base64(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Str(s) => {
            let b = STANDARD.decode(s).map_err(|e| {
                EncodingFailedSnafu {
                    message: format!("Data is not properly encoded: {e}"),
                }
                .build()
            })?;
            Ok(DataValue::Bytes(b))
        }
        _ => TypeMismatchSnafu {
            op: "decode_base64",
            expected: "strings",
        }
        .fail(),
    }
}

pub(crate) fn op_chars(args: &[DataValue]) -> Result<DataValue> {
    Ok(DataValue::List(
        arg(args, 0)?
            .get_str()
            .ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "chars",
                    expected: "strings",
                }
                .build()
            })?
            .chars()
            .map(|c| {
                let mut s = CompactString::default();
                s.push(c);
                DataValue::Str(s)
            })
            .collect_vec(),
    ))
}

pub(crate) fn op_slice_string(args: &[DataValue]) -> Result<DataValue> {
    let s = arg(args, 0)?.get_str().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "slice_string",
            expected: "a string as first argument",
        }
        .build()
    })?;
    let m = arg(args, 1)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "slice_string",
            expected: "an integer as second argument",
        }
        .build()
    })?;
    snafu::ensure!(
        m >= 0,
        InvalidValueSnafu {
            message: "second argument to 'slice_string' mut be a positive integer"
        }
    );
    let n = arg(args, 2)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "slice_string",
            expected: "an integer as third argument",
        }
        .build()
    })?;
    snafu::ensure!(
        n >= m,
        InvalidValueSnafu {
            message: "third argument to 'slice_string' mut be a positive integer greater than the second argument"
        }
    );
    Ok(DataValue::Str(
        s.chars().skip(m as usize).take((n - m) as usize).collect(),
    ))
}

pub(crate) fn op_from_substrings(args: &[DataValue]) -> Result<DataValue> {
    let mut ret = String::new();
    match arg(args, 0)? {
        DataValue::List(ss) => {
            for arg in ss {
                if let DataValue::Str(s) = arg {
                    ret.push_str(s);
                } else {
                    return TypeMismatchSnafu {
                        op: "from_substring",
                        expected: "a list of strings",
                    }
                    .fail();
                }
            }
        }
        DataValue::Set(ss) => {
            for arg in ss {
                if let DataValue::Str(s) = arg {
                    ret.push_str(s);
                } else {
                    return TypeMismatchSnafu {
                        op: "from_substring",
                        expected: "a list of strings",
                    }
                    .fail();
                }
            }
        }
        _ => {
            return TypeMismatchSnafu {
                op: "from_substring",
                expected: "a list of strings",
            }
            .fail();
        }
    }
    Ok(DataValue::from(ret))
}
