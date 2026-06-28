//! JSON serialization and deserialization for data values.
use std::num::FpCategory;

use koina::base64;
pub(crate) use serde_json::Value as JsonValue;
use serde_json::json;

use crate::data::value::JsonData;
use crate::data::value::{DataValue, Num, Vector};

impl From<JsonValue> for DataValue {
    fn from(v: JsonValue) -> Self {
        match v {
            JsonValue::Null => DataValue::Null,
            JsonValue::Bool(b) => DataValue::Bool(b),
            JsonValue::Number(n) => match n.as_i64() {
                Some(i) => DataValue::from(i),
                None => match n.as_f64() {
                    Some(f) => DataValue::from(f),
                    None => DataValue::from(n.to_string()),
                },
            },
            JsonValue::String(s) => DataValue::from(s),
            JsonValue::Array(arr) => DataValue::List(arr.iter().map(DataValue::from).collect()),
            JsonValue::Object(d) => DataValue::Json(JsonData(JsonValue::Object(d))),
        }
    }
}

impl<'a> From<&'a JsonValue> for DataValue {
    fn from(v: &'a JsonValue) -> Self {
        match v {
            JsonValue::Null => DataValue::Null,
            JsonValue::Bool(b) => DataValue::Bool(*b),
            JsonValue::Number(n) => match n.as_i64() {
                Some(i) => DataValue::from(i),
                None => match n.as_f64() {
                    Some(f) => DataValue::from(f),
                    None => DataValue::from(n.to_string()),
                },
            },
            JsonValue::String(s) => DataValue::Str(s.into()),
            JsonValue::Array(arr) => DataValue::List(arr.iter().map(DataValue::from).collect()),
            JsonValue::Object(d) => DataValue::Json(JsonData(JsonValue::Object(d.clone()))),
        }
    }
}

impl From<DataValue> for JsonValue {
    fn from(v: DataValue) -> Self {
        match v {
            DataValue::Null | DataValue::Bot => JsonValue::Null,
            DataValue::Bool(b) => JsonValue::Bool(b),
            DataValue::Num(Num::Int(i)) => JsonValue::Number(i.into()),
            DataValue::Num(Num::Float(f)) => match f.classify() {
                FpCategory::Nan => JsonValue::Null,
                FpCategory::Infinite => {
                    if f.is_sign_negative() {
                        json!("NEGATIVE_INFINITY")
                    } else {
                        json!("INFINITY")
                    }
                }
                FpCategory::Zero | FpCategory::Subnormal | FpCategory::Normal => json!(f),
            },
            DataValue::Str(t) => JsonValue::String(t.into()),
            DataValue::Bytes(bytes) => JsonValue::String(base64::encode(&bytes)),
            DataValue::List(l) => JsonValue::Array(l.into_iter().map(JsonValue::from).collect()),
            DataValue::Set(l) => JsonValue::Array(l.into_iter().map(JsonValue::from).collect()),
            DataValue::Regex(r) => {
                json!(r.0.as_str())
            }
            DataValue::Uuid(u) => {
                json!(u.0)
            }
            DataValue::Vec(arr) => match arr {
                Vector::F32(a) => {
                    json!(a.as_slice().unwrap_or(&[]))
                }
                Vector::F64(a) => {
                    json!(a.as_slice().unwrap_or(&[]))
                }
            },
            DataValue::Validity(v) => {
                json!([v.timestamp.0, v.is_assert])
            }
            DataValue::Json(j) => j.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: `DataValue::Bot` previously panicked during JSON export.
    /// Verified fix: Bot must map to `JsonValue::Null`, not unreachable.
    #[test]
    fn data_value_bot_exports_as_null_not_panic() {
        assert_eq!(JsonValue::from(DataValue::Bot), JsonValue::Null);
    }

    #[test]
    fn data_value_null_exports_as_null() {
        assert_eq!(JsonValue::from(DataValue::Null), JsonValue::Null);
    }

    /// Regression for #4607: float JSON export must be exhaustive without a
    /// process-wide panic branch.
    #[test]
    fn float_json_export_handles_every_f64_category() {
        let subnormal = f64::from_bits(1);
        let cases: &[(f64, FpCategory, JsonValue)] = &[
            (0.0, FpCategory::Zero, json!(0.0)),
            (subnormal, FpCategory::Subnormal, json!(subnormal)),
            (1.5, FpCategory::Normal, json!(1.5)),
            (f64::NAN, FpCategory::Nan, JsonValue::Null),
            (f64::INFINITY, FpCategory::Infinite, json!("INFINITY")),
            (
                f64::NEG_INFINITY,
                FpCategory::Infinite,
                json!("NEGATIVE_INFINITY"),
            ),
        ];
        for (value, category, expected) in cases {
            assert_eq!(value.classify(), *category);
            assert_eq!(JsonValue::from(DataValue::from(*value)), *expected);
        }
    }
}
