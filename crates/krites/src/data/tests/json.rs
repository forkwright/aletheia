//! Tests for JSON round-tripping.
use serde_json::json;

use crate::data::json::JsonValue;
use crate::data::value::DataValue;

#[test]
fn special_float_json_serialization() {
    // Verify that serde_json serializes special float values as null
    // (standard JSON does not support Infinity or NaN)
    let infinity_json = json!(f64::INFINITY);
    let neg_infinity_json = json!(f64::NEG_INFINITY);
    let nan_json = json!(f64::NAN);

    // serde_json serializes Infinity and NaN as null by default
    assert!(infinity_json.is_null(), "Infinity should serialize to null");
    assert!(
        neg_infinity_json.is_null(),
        "Negative Infinity should serialize to null"
    );
    assert!(nan_json.is_null(), "NaN should serialize to null");

    // Verify DataValue -> JsonValue conversion for special floats
    // JsonValue uses string representation for special floats
    let dv_infinity = DataValue::from(f64::INFINITY);
    let dv_neg_infinity = DataValue::from(f64::NEG_INFINITY);
    let dv_nan = DataValue::from(f64::NAN);

    let jv_infinity: JsonValue = dv_infinity.into();
    let jv_neg_infinity: JsonValue = dv_neg_infinity.into();
    let jv_nan: JsonValue = dv_nan.into();

    // JsonValue serializes special floats as string constants
    assert_eq!(
        serde_json::to_string(&jv_infinity).expect("serialize"),
        r#""INFINITY""#,
        "JsonValue Infinity should serialize to string constant"
    );
    assert_eq!(
        serde_json::to_string(&jv_neg_infinity).expect("serialize"),
        r#""NEGATIVE_INFINITY""#,
        "JsonValue Negative Infinity should serialize to string constant"
    );
    assert_eq!(
        serde_json::to_string(&jv_nan).expect("serialize"),
        "null",
        "JsonValue NaN should serialize to null"
    );
}
