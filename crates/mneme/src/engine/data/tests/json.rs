// Originally derived from CozoDB v0.7.6 (MPL-2.0).
// Copyright 2022, The Cozo Project Authors — see NOTICE for details.

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
