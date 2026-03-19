//! Tests for predicates, collection operations, geographic functions, and UUIDs.
#![expect(clippy::expect_used, reason = "test assertions")]
use std::f64::consts::PI;

use crate::engine::data::functions::*;
use crate::engine::data::value::DataValue;

#[test]
fn test_predicates() {
    assert_eq!(
        op_is_null(&[DataValue::Null]).expect("test assertion"),
        DataValue::from(true),
        "Null should be null"
    );
    assert_eq!(
        op_is_null(&[DataValue::Bot]).expect("test assertion"),
        DataValue::from(false),
        "Bot should not be null"
    );
    assert_eq!(
        op_is_int(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "integer value should be an int"
    );
    assert_eq!(
        op_is_int(&[DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(false),
        "float value should not be an int"
    );
    assert_eq!(
        op_is_float(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(false),
        "integer value should not be a float"
    );
    assert_eq!(
        op_is_float(&[DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(true),
        "float value should be a float"
    );
    assert_eq!(
        op_is_num(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "integer should be numeric"
    );
    assert_eq!(
        op_is_num(&[DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(true),
        "float should be numeric"
    );
    assert_eq!(
        op_is_num(&[DataValue::Null]).expect("test assertion"),
        DataValue::from(false),
        "null should not be numeric"
    );
    assert_eq!(
        op_is_bytes(&[DataValue::Bytes([0b1].into())]).expect("test assertion"),
        DataValue::from(true),
        "bytes value should be bytes"
    );
    assert_eq!(
        op_is_bytes(&[DataValue::Null]).expect("test assertion"),
        DataValue::from(false),
        "null should not be bytes"
    );
    assert_eq!(
        op_is_list(&[DataValue::List(vec![])]).expect("test assertion"),
        DataValue::from(true),
        "list value should be a list"
    );
    assert_eq!(
        op_is_list(&[DataValue::Null]).expect("test assertion"),
        DataValue::from(false),
        "null should not be a list"
    );
    assert_eq!(
        op_is_string(&[DataValue::Str("".into())]).expect("test assertion"),
        DataValue::from(true),
        "string value should be a string"
    );
    assert_eq!(
        op_is_string(&[DataValue::Null]).expect("test assertion"),
        DataValue::from(false),
        "null should not be a string"
    );
    assert_eq!(
        op_is_finite(&[DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(true),
        "finite float should be finite"
    );
    assert_eq!(
        op_is_finite(&[DataValue::from(f64::INFINITY)]).expect("test assertion"),
        DataValue::from(false),
        "infinity should not be finite"
    );
    assert_eq!(
        op_is_finite(&[DataValue::from(f64::NAN)]).expect("test assertion"),
        DataValue::from(false),
        "NaN should not be finite"
    );
    assert_eq!(
        op_is_infinite(&[DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(false),
        "finite float should not be infinite"
    );
    assert_eq!(
        op_is_infinite(&[DataValue::from(f64::INFINITY)]).expect("test assertion"),
        DataValue::from(true),
        "positive infinity should be infinite"
    );
    assert_eq!(
        op_is_infinite(&[DataValue::from(f64::NEG_INFINITY)]).expect("test assertion"),
        DataValue::from(true),
        "negative infinity should be infinite"
    );
    assert_eq!(
        op_is_infinite(&[DataValue::from(f64::NAN)]).expect("test assertion"),
        DataValue::from(false),
        "NaN should not be infinite"
    );
    assert_eq!(
        op_is_nan(&[DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(false),
        "finite float should not be NaN"
    );
    assert_eq!(
        op_is_nan(&[DataValue::from(f64::INFINITY)]).expect("test assertion"),
        DataValue::from(false),
        "infinity should not be NaN"
    );
    assert_eq!(
        op_is_nan(&[DataValue::from(f64::NEG_INFINITY)]).expect("test assertion"),
        DataValue::from(false),
        "negative infinity should not be NaN"
    );
    assert_eq!(
        op_is_nan(&[DataValue::from(f64::NAN)]).expect("test assertion"),
        DataValue::from(true),
        "NaN should be NaN"
    );
}

#[test]
fn test_prepend_append() {
    assert_eq!(
        op_prepend(&[
            DataValue::List(vec![DataValue::from(1), DataValue::from(2)]),
            DataValue::Null,
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::Null,
            DataValue::from(1),
            DataValue::from(2),
        ]),
        "prepend should insert element at front of list"
    );
    assert_eq!(
        op_append(&[
            DataValue::List(vec![DataValue::from(1), DataValue::from(2)]),
            DataValue::Null,
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::from(1),
            DataValue::from(2),
            DataValue::Null,
        ]),
        "append should add element at end of list"
    );
}

#[test]
fn test_length() {
    assert_eq!(
        op_length(&[DataValue::Str("abc".into())]).expect("test assertion"),
        DataValue::from(3),
        "length of 3-char string should be 3"
    );
    assert_eq!(
        op_length(&[DataValue::List(vec![])]).expect("test assertion"),
        DataValue::from(0),
        "length of empty list should be 0"
    );
    assert_eq!(
        op_length(&[DataValue::Bytes([].into())]).expect("test assertion"),
        DataValue::from(0),
        "length of empty bytes should be 0"
    );
}

#[test]
fn test_unicode_normalize() {
    assert_eq!(
        op_unicode_normalize(&[DataValue::Str("abc".into()), DataValue::Str("nfc".into())])
            .expect("test assertion"),
        DataValue::Str("abc".into()),
        "normalizing pure ASCII under NFC should be a no-op"
    )
}

#[test]
fn test_sort_reverse() {
    assert_eq!(
        op_sorted(&[DataValue::List(vec![
            DataValue::from(2.0),
            DataValue::from(1),
            DataValue::from(2),
            DataValue::Null,
        ])])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::Null,
            DataValue::from(1),
            DataValue::from(2),
            DataValue::from(2.0),
        ]),
        "sorted should order Null < int < float ascending"
    );
    assert_eq!(
        op_reverse(&[DataValue::List(vec![
            DataValue::from(2.0),
            DataValue::from(1),
            DataValue::from(2),
            DataValue::Null,
        ])])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::Null,
            DataValue::from(2),
            DataValue::from(1),
            DataValue::from(2.0),
        ]),
        "reverse should reverse the list order"
    )
}

#[test]
fn test_haversine() {
    let d = op_haversine_deg_input(&[
        DataValue::from(0),
        DataValue::from(0),
        DataValue::from(0),
        DataValue::from(180),
    ])
    .expect("test assertion")
    .get_float()
    .expect("test assertion");
    assert!(
        ((d) - (PI)).abs() < 1e-5,
        "haversine(0,0,0,180) should be approximately π"
    );

    let d = op_haversine_deg_input(&[
        DataValue::from(90),
        DataValue::from(0),
        DataValue::from(0),
        DataValue::from(123),
    ])
    .expect("test assertion")
    .get_float()
    .expect("test assertion");
    assert!(
        ((d) - (PI / 2.)).abs() < 1e-5,
        "haversine(90,0,0,123) should be approximately π/2"
    );

    let d = op_haversine(&[
        DataValue::from(0),
        DataValue::from(0),
        DataValue::from(0),
        DataValue::from(PI),
    ])
    .expect("test assertion")
    .get_float()
    .expect("test assertion");
    assert!(
        ((d) - (PI)).abs() < 1e-5,
        "haversine(0,0,0,π) in radians should be approximately π"
    );
}

#[test]
fn test_deg_rad() {
    assert_eq!(
        op_deg_to_rad(&[DataValue::from(180)]).expect("test assertion"),
        DataValue::from(PI),
        "180 degrees should convert to π radians"
    );
    assert_eq!(
        op_rad_to_deg(&[DataValue::from(PI)]).expect("test assertion"),
        DataValue::from(180.0),
        "π radians should convert to 180 degrees"
    );
}

#[test]
fn test_first_last() {
    assert_eq!(
        op_first(&[DataValue::List(vec![])]).expect("test assertion"),
        DataValue::Null,
        "first of empty list should return Null"
    );
    assert_eq!(
        op_last(&[DataValue::List(vec![])]).expect("test assertion"),
        DataValue::Null,
        "last of empty list should return Null"
    );
    assert_eq!(
        op_first(&[DataValue::List(vec![
            DataValue::from(1),
            DataValue::from(2),
        ])])
        .expect("test assertion"),
        DataValue::from(1),
        "first should return the first element"
    );
    assert_eq!(
        op_last(&[DataValue::List(vec![
            DataValue::from(1),
            DataValue::from(2),
        ])])
        .expect("test assertion"),
        DataValue::from(2),
        "last should return the last element"
    );
}

#[test]
fn test_chunks() {
    assert_eq!(
        op_chunks(&[
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
                DataValue::from(4),
                DataValue::from(5),
            ]),
            DataValue::from(2),
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::List(vec![DataValue::from(1), DataValue::from(2)]),
            DataValue::List(vec![DataValue::from(3), DataValue::from(4)]),
            DataValue::List(vec![DataValue::from(5)]),
        ]),
        "chunks of size 2 from 5 elements should produce two full and one partial chunk"
    );
    assert_eq!(
        op_chunks_exact(&[
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
                DataValue::from(4),
                DataValue::from(5),
            ]),
            DataValue::from(2),
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::List(vec![DataValue::from(1), DataValue::from(2)]),
            DataValue::List(vec![DataValue::from(3), DataValue::from(4)]),
        ]),
        "chunks_exact of size 2 from 5 elements should drop remainder"
    );
    assert_eq!(
        op_windows(&[
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
                DataValue::from(4),
                DataValue::from(5),
            ]),
            DataValue::from(3),
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
            ]),
            DataValue::List(vec![
                DataValue::from(2),
                DataValue::from(3),
                DataValue::from(4),
            ]),
            DataValue::List(vec![
                DataValue::from(3),
                DataValue::from(4),
                DataValue::from(5),
            ]),
        ]),
        "windows of size 3 from 5 elements should produce 3 overlapping windows"
    )
}

#[test]
fn test_get() {
    assert!(
        op_get(&[DataValue::List(vec![]), DataValue::from(0)]).is_err(),
        "getting index 0 from empty list should error"
    );
    assert_eq!(
        op_get(&[
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
            ]),
            DataValue::from(1)
        ])
        .expect("test assertion"),
        DataValue::from(2),
        "get at index 1 should return the second element"
    );
    assert_eq!(
        op_maybe_get(&[DataValue::List(vec![]), DataValue::from(0)]).expect("test assertion"),
        DataValue::Null,
        "maybe_get on empty list should return Null"
    );
    assert_eq!(
        op_maybe_get(&[
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
            ]),
            DataValue::from(1)
        ])
        .expect("test assertion"),
        DataValue::from(2),
        "maybe_get at index 1 should return the second element"
    );
}

#[test]
fn test_slice() {
    assert!(
        op_slice(&[
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
            ]),
            DataValue::from(1),
            DataValue::from(4)
        ])
        .is_err(),
        "slice with end beyond length should error"
    );

    assert!(
        op_slice(&[
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
            ]),
            DataValue::from(1),
            DataValue::from(3)
        ])
        .is_ok(),
        "slice within bounds should succeed"
    );

    assert_eq!(
        op_slice(&[
            DataValue::List(vec![
                DataValue::from(1),
                DataValue::from(2),
                DataValue::from(3),
            ]),
            DataValue::from(1),
            DataValue::from(-1)
        ])
        .expect("test assertion"),
        DataValue::List(vec![DataValue::from(2)]),
        "slice with negative end should exclude last element"
    );
}

#[test]
fn test_chars() {
    assert_eq!(
        op_from_substrings(&[op_chars(&[DataValue::Str("abc".into())]).expect("test assertion")])
            .expect("test assertion"),
        DataValue::Str("abc".into()),
        "chars then from_substrings should round-trip the string"
    )
}

#[test]
fn test_encode_decode() {
    assert_eq!(
        op_decode_base64(&[
            op_encode_base64(&[DataValue::Bytes([1, 2, 3].into())]).expect("test assertion")
        ])
        .expect("test assertion"),
        DataValue::Bytes([1, 2, 3].into()),
        "base64 encode then decode should round-trip the bytes"
    )
}

#[test]
fn test_to_string() {
    assert_eq!(
        op_to_string(&[DataValue::from(false)]).expect("test assertion"),
        DataValue::Str("false".into()),
        "to_string of false should produce the string 'false'"
    );
}

#[test]
fn test_to_unity() {
    assert_eq!(
        op_to_unity(&[DataValue::Null]).expect("test assertion"),
        DataValue::from(0),
        "to_unity of Null should be 0"
    );
    assert_eq!(
        op_to_unity(&[DataValue::from(false)]).expect("test assertion"),
        DataValue::from(0),
        "to_unity of false should be 0"
    );
    assert_eq!(
        op_to_unity(&[DataValue::from(true)]).expect("test assertion"),
        DataValue::from(1),
        "to_unity of true should be 1"
    );
    assert_eq!(
        op_to_unity(&[DataValue::from(10)]).expect("test assertion"),
        DataValue::from(1),
        "to_unity of nonzero int should be 1"
    );
    assert_eq!(
        op_to_unity(&[DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(1),
        "to_unity of nonzero float should be 1"
    );
    assert_eq!(
        op_to_unity(&[DataValue::from(f64::NAN)]).expect("test assertion"),
        DataValue::from(1),
        "to_unity of NaN should be 1 (truthy)"
    );
    assert_eq!(
        op_to_unity(&[DataValue::Str("0".into())]).expect("test assertion"),
        DataValue::from(1),
        "to_unity of non-empty string should be 1"
    );
    assert_eq!(
        op_to_unity(&[DataValue::Str("".into())]).expect("test assertion"),
        DataValue::from(0),
        "to_unity of empty string should be 0"
    );
    assert_eq!(
        op_to_unity(&[DataValue::List(vec![])]).expect("test assertion"),
        DataValue::from(0),
        "to_unity of empty list should be 0"
    );
    assert_eq!(
        op_to_unity(&[DataValue::List(vec![DataValue::Null])]).expect("test assertion"),
        DataValue::from(1),
        "to_unity of non-empty list should be 1"
    );
}

#[test]
fn test_to_float() {
    assert_eq!(
        op_to_float(&[DataValue::Null]).expect("test assertion"),
        DataValue::from(0.0),
        "to_float of Null should be 0.0"
    );
    assert_eq!(
        op_to_float(&[DataValue::from(false)]).expect("test assertion"),
        DataValue::from(0.0),
        "to_float of false should be 0.0"
    );
    assert_eq!(
        op_to_float(&[DataValue::from(true)]).expect("test assertion"),
        DataValue::from(1.0),
        "to_float of true should be 1.0"
    );
    assert_eq!(
        op_to_float(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(1.0),
        "to_float of int 1 should be 1.0"
    );
    assert_eq!(
        op_to_float(&[DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(1.0),
        "to_float of float 1.0 should be 1.0"
    );
    assert!(
        op_to_float(&[DataValue::Str("NAN".into())])
            .expect("test assertion")
            .get_float()
            .expect("test assertion")
            .is_nan(),
        "to_float of NAN string should produce NaN"
    );
    assert!(
        op_to_float(&[DataValue::Str("INF".into())])
            .expect("test assertion")
            .get_float()
            .expect("test assertion")
            .is_infinite(),
        "to_float of INF string should produce infinity"
    );
    assert!(
        op_to_float(&[DataValue::Str("NEG_INF".into())])
            .expect("test assertion")
            .get_float()
            .expect("test assertion")
            .is_infinite(),
        "to_float of NEG_INF string should produce negative infinity"
    );
    assert_eq!(
        op_to_float(&[DataValue::Str("3".into())])
            .expect("test assertion")
            .get_float()
            .expect("test assertion"),
        3.,
        "to_float of numeric string should produce 3.0"
    );
}

#[test]
fn test_rand() {
    let n = op_rand_float(&[])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!(n >= 0., "random float should be >= 0");
    assert!(n <= 1., "random float should be <= 1");
    assert_eq!(
        op_rand_bernoulli(&[DataValue::from(0)]).expect("test assertion"),
        DataValue::from(false),
        "bernoulli with p=0 should always return false"
    );
    assert_eq!(
        op_rand_bernoulli(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "bernoulli with p=1 should always return true"
    );
    assert!(
        op_rand_bernoulli(&[DataValue::from(2)]).is_err(),
        "bernoulli with p>1 should error"
    );
    let n = op_rand_int(&[DataValue::from(100), DataValue::from(200)])
        .expect("test assertion")
        .get_int()
        .expect("test assertion");
    assert!(n >= 100, "random int should be within lower bound");
    assert!(n <= 200, "random int should be within upper bound");
    assert_eq!(
        op_rand_choose(&[DataValue::List(vec![])]).expect("test assertion"),
        DataValue::Null,
        "choosing from empty list should return Null"
    );
    assert_eq!(
        op_rand_choose(&[DataValue::List(vec![DataValue::from(123)])]).expect("test assertion"),
        DataValue::from(123),
        "choosing from single-element list should return that element"
    );
}

#[test]
fn test_set_ops() {
    assert_eq!(
        op_union(&[
            DataValue::List([1, 2, 3].into_iter().map(DataValue::from).collect()),
            DataValue::List([2, 3, 4].into_iter().map(DataValue::from).collect()),
            DataValue::List([3, 4, 5].into_iter().map(DataValue::from).collect())
        ])
        .expect("test assertion"),
        DataValue::List([1, 2, 3, 4, 5].into_iter().map(DataValue::from).collect()),
        "union of overlapping sets should contain all unique elements"
    );
    assert_eq!(
        op_intersection(&[
            DataValue::List(
                [1, 2, 3, 4, 5, 6]
                    .into_iter()
                    .map(DataValue::from)
                    .collect(),
            ),
            DataValue::List([2, 3, 4].into_iter().map(DataValue::from).collect()),
            DataValue::List([3, 4, 5].into_iter().map(DataValue::from).collect())
        ])
        .expect("test assertion"),
        DataValue::List([3, 4].into_iter().map(DataValue::from).collect()),
        "intersection should contain only elements present in all sets"
    );
    assert_eq!(
        op_difference(&[
            DataValue::List(
                [1, 2, 3, 4, 5, 6]
                    .into_iter()
                    .map(DataValue::from)
                    .collect(),
            ),
            DataValue::List([2, 3, 4].into_iter().map(DataValue::from).collect()),
            DataValue::List([3, 4, 5].into_iter().map(DataValue::from).collect())
        ])
        .expect("test assertion"),
        DataValue::List([1, 6].into_iter().map(DataValue::from).collect()),
        "difference should contain elements in first set but not in others"
    );
}

#[test]
fn test_uuid() {
    let v1 = op_rand_uuid_v1(&[]).expect("test assertion");
    let v4 = op_rand_uuid_v4(&[]).expect("test assertion");
    assert!(
        op_is_uuid(&[v4])
            .expect("test assertion")
            .get_bool()
            .expect("test assertion"),
        "generated v4 uuid should be recognized as a uuid"
    );
    assert!(
        op_uuid_timestamp(&[v1])
            .expect("test assertion")
            .get_float()
            .is_some(),
        "v1 uuid timestamp should be extractable"
    );
    assert!(
        op_to_uuid(&[DataValue::from("")]).is_err(),
        "empty string should not parse as uuid"
    );
    assert!(
        op_to_uuid(&[DataValue::from("f3b4958c-52a1-11e7-802a-010203040506")]).is_ok(),
        "valid uuid string should parse successfully"
    );
}
