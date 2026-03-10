//! Tests for JSON round-tripping.
use serde_json::json;

use crate::engine::data::json::JsonValue;
use crate::engine::data::value::DataValue;

#[test]
fn bad_values() {
    println!("{}", json!(f64::INFINITY));
    println!("{}", JsonValue::from(DataValue::from(f64::INFINITY)));
    println!("{}", JsonValue::from(DataValue::from(f64::NEG_INFINITY)));
    println!("{}", JsonValue::from(DataValue::from(f64::NAN)));
}
