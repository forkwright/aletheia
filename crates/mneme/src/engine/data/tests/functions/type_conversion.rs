//! Tests for type conversion, coalesce, and range functions.
#![expect(clippy::expect_used, reason = "test assertions")]
use serde_json::json;

use crate::engine::DbInstance;
use crate::engine::data::functions::*;
use crate::engine::data::value::DataValue;

#[test]
fn test_now() {
    let now = op_now(&[]).expect("test assertion");
    assert!(
        matches!(now, DataValue::Num(_)),
        "op_now should return a numeric timestamp"
    );
    let s = op_format_timestamp(&[now]).expect("test assertion");
    let _dt = op_parse_timestamp(&[s]).expect("test assertion");
}

#[test]
fn test_to_bool() {
    assert_eq!(
        op_to_bool(&[DataValue::Null]).expect("test assertion"),
        DataValue::from(false),
        "to_bool of Null should be false"
    );
    assert_eq!(
        op_to_bool(&[DataValue::from(true)]).expect("test assertion"),
        DataValue::from(true),
        "to_bool of true should be true"
    );
    assert_eq!(
        op_to_bool(&[DataValue::from(false)]).expect("test assertion"),
        DataValue::from(false),
        "to_bool of false should be false"
    );
    assert_eq!(
        op_to_bool(&[DataValue::from(0)]).expect("test assertion"),
        DataValue::from(false),
        "to_bool of 0 should be false"
    );
    assert_eq!(
        op_to_bool(&[DataValue::from(0.0)]).expect("test assertion"),
        DataValue::from(false),
        "to_bool of 0.0 should be false"
    );
    assert_eq!(
        op_to_bool(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "to_bool of nonzero int should be true"
    );
    assert_eq!(
        op_to_bool(&[DataValue::from("")]).expect("test assertion"),
        DataValue::from(false),
        "to_bool of empty string should be false"
    );
    assert_eq!(
        op_to_bool(&[DataValue::from("a")]).expect("test assertion"),
        DataValue::from(true),
        "to_bool of non-empty string should be true"
    );
    assert_eq!(
        op_to_bool(&[DataValue::List(vec![])]).expect("test assertion"),
        DataValue::from(false),
        "to_bool of empty list should be false"
    );
    assert_eq!(
        op_to_bool(&[DataValue::List(vec![DataValue::from(0)])]).expect("test assertion"),
        DataValue::from(true),
        "to_bool of non-empty list should be true"
    );
}

#[test]
fn test_coalesce() {
    let db = DbInstance::default();
    let res = db
        .run_default("?[a] := a = null ~ 1 ~ 2")
        .expect("test assertion")
        .rows;
    assert_eq!(
        res[0][0],
        DataValue::from(1),
        "null ~ 1 ~ 2 should coalesce to 1"
    );
    let res = db
        .run_default("?[a] := a = null ~ null ~ null")
        .expect("test assertion")
        .rows;
    assert_eq!(
        res[0][0],
        DataValue::Null,
        "null ~ null ~ null should coalesce to Null"
    );
    let res = db
        .run_default("?[a] := a = 2 ~ null ~ 1")
        .expect("test assertion")
        .rows;
    assert_eq!(
        res[0][0],
        DataValue::from(2),
        "2 ~ null ~ 1 should coalesce to 2 (first non-null)"
    );
}

#[test]
fn test_range() {
    let db = DbInstance::default();
    let res = db
        .run_default("?[a] := a = int_range(1, 5)")
        .expect("test assertion")
        .into_json();
    assert_eq!(
        res["rows"][0][0],
        json!([1, 2, 3, 4]),
        "int_range(1, 5) should produce [1, 2, 3, 4]"
    );
    let res = db
        .run_default("?[a] := a = int_range(5)")
        .expect("test assertion")
        .into_json();
    assert_eq!(
        res["rows"][0][0],
        json!([0, 1, 2, 3, 4]),
        "int_range(5) should produce [0, 1, 2, 3, 4]"
    );
    let res = db
        .run_default("?[a] := a = int_range(15, 3, -2)")
        .expect("test assertion")
        .into_json();
    assert_eq!(
        res["rows"][0][0],
        json!([15, 13, 11, 9, 7, 5]),
        "int_range with negative step should produce descending range"
    );
}
