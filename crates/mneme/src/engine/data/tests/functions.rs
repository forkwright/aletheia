//! Tests for built-in functions.
#![expect(clippy::expect_used, reason = "test assertions")]
use regex::Regex;
use serde_json::json;
use std::f64::consts::{E, PI};

use crate::engine::DbInstance;
use crate::engine::data::functions::*;
use crate::engine::data::value::{DataValue, RegexWrapper};

#[test]
fn op_add_sums_integers_and_floats() {
    assert_eq!(
        op_add(&[]).expect("test assertion"),
        DataValue::from(0),
        "empty args should sum to 0"
    );
    assert_eq!(
        op_add(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(1),
        "single int arg should return itself"
    );
    assert_eq!(
        op_add(&[DataValue::from(1), DataValue::from(2)]).expect("test assertion"),
        DataValue::from(3),
        "int addition should sum correctly"
    );
    assert_eq!(
        op_add(&[DataValue::from(1), DataValue::from(2.5)]).expect("test assertion"),
        DataValue::from(3.5),
        "int plus float should produce float sum"
    );
    assert_eq!(
        op_add(&[DataValue::from(1.5), DataValue::from(2.5)]).expect("test assertion"),
        DataValue::from(4.0),
        "float addition should sum correctly"
    );
}

#[test]
fn test_sub() {
    assert_eq!(
        op_sub(&[DataValue::from(1), DataValue::from(2)]).expect("test assertion"),
        DataValue::from(-1),
        "int subtraction should produce negative result when smaller minus larger"
    );
    assert_eq!(
        op_sub(&[DataValue::from(1), DataValue::from(2.5)]).expect("test assertion"),
        DataValue::from(-1.5),
        "int minus float should produce float result"
    );
    assert_eq!(
        op_sub(&[DataValue::from(1.5), DataValue::from(2.5)]).expect("test assertion"),
        DataValue::from(-1.0),
        "float subtraction should produce correct negative float"
    );
}

#[test]
fn test_mul() {
    assert_eq!(
        op_mul(&[]).expect("test assertion"),
        DataValue::from(1),
        "empty args should produce identity 1"
    );
    assert_eq!(
        op_mul(&[DataValue::from(2), DataValue::from(3)]).expect("test assertion"),
        DataValue::from(6),
        "int multiplication should produce correct product"
    );
    assert_eq!(
        op_mul(&[DataValue::from(0.5), DataValue::from(0.25)]).expect("test assertion"),
        DataValue::from(0.125),
        "float multiplication should produce correct fractional product"
    );
    assert_eq!(
        op_mul(&[DataValue::from(0.5), DataValue::from(3)]).expect("test assertion"),
        DataValue::from(1.5),
        "float times int should produce correct float product"
    );
}

#[test]
fn test_div() {
    assert_eq!(
        op_div(&[DataValue::from(1), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(1.0),
        "dividing by self should produce 1.0"
    );
    assert_eq!(
        op_div(&[DataValue::from(1), DataValue::from(2)]).expect("test assertion"),
        DataValue::from(0.5),
        "1 / 2 should produce 0.5"
    );
    assert_eq!(
        op_div(&[DataValue::from(7.0), DataValue::from(0.5)]).expect("test assertion"),
        DataValue::from(14.0),
        "dividing by 0.5 should double the value"
    );
    assert!(
        op_div(&[DataValue::from(1), DataValue::from(0)]).is_ok(),
        "integer division by zero should not error"
    );
}

#[test]
fn test_eq_neq() {
    assert_eq!(
        op_eq(&[DataValue::from(1), DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(true),
        "int and float with same value should be equal"
    );
    assert_eq!(
        op_eq(&[DataValue::from(123), DataValue::from(123)]).expect("test assertion"),
        DataValue::from(true),
        "identical ints should be equal"
    );
    assert_eq!(
        op_neq(&[DataValue::from(1), DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(false),
        "int and float with same value should not be unequal"
    );
    assert_eq!(
        op_neq(&[DataValue::from(123), DataValue::from(123.0)]).expect("test assertion"),
        DataValue::from(false),
        "int and matching float should not be unequal"
    );
    assert_eq!(
        op_eq(&[DataValue::from(123), DataValue::from(123.1)]).expect("test assertion"),
        DataValue::from(false),
        "int and float with different values should not be equal"
    );
}

#[test]
fn test_list() {
    assert_eq!(
        op_list(&[]).expect("test assertion"),
        DataValue::List(vec![]),
        "empty args should produce empty list"
    );
    assert_eq!(
        op_list(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::List(vec![DataValue::from(1)]),
        "single element should produce single-element list"
    );
    assert_eq!(
        op_list(&[DataValue::from(1), DataValue::List(vec![])]).expect("test assertion"),
        DataValue::List(vec![DataValue::from(1), DataValue::List(vec![])]),
        "mixed args should produce list preserving nested structure"
    );
}

#[test]
fn test_is_in() {
    assert_eq!(
        op_is_in(&[
            DataValue::from(1),
            DataValue::List(vec![DataValue::from(1), DataValue::from(2)])
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "element present in list should return true"
    );
    assert_eq!(
        op_is_in(&[
            DataValue::from(3),
            DataValue::List(vec![DataValue::from(1), DataValue::from(2)])
        ])
        .expect("test assertion"),
        DataValue::from(false),
        "element absent from list should return false"
    );
    assert_eq!(
        op_is_in(&[DataValue::from(3), DataValue::List(vec![])]).expect("test assertion"),
        DataValue::from(false),
        "element checked against empty list should return false"
    );
}

#[test]
fn test_comparators() {
    assert_eq!(
        op_ge(&[DataValue::from(2), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "2 >= 1 should be true"
    );
    assert_eq!(
        op_ge(&[DataValue::from(2.), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "2.0 >= 1 should be true"
    );
    assert_eq!(
        op_ge(&[DataValue::from(2), DataValue::from(1.)]).expect("test assertion"),
        DataValue::from(true),
        "2 >= 1.0 should be true"
    );

    assert_eq!(
        op_ge(&[DataValue::from(1), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "equal ints: 1 >= 1 should be true"
    );
    assert_eq!(
        op_ge(&[DataValue::from(1), DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(true),
        "int equal to float: 1 >= 1.0 should be true"
    );
    assert_eq!(
        op_ge(&[DataValue::from(1), DataValue::from(2)]).expect("test assertion"),
        DataValue::from(false),
        "1 >= 2 should be false"
    );
    assert!(
        op_ge(&[DataValue::Null, DataValue::from(true)]).is_err(),
        "ge with null operand should error"
    );
    assert_eq!(
        op_gt(&[DataValue::from(2), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "2 > 1 should be true"
    );
    assert_eq!(
        op_gt(&[DataValue::from(2.), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "2.0 > 1 should be true"
    );
    assert_eq!(
        op_gt(&[DataValue::from(2), DataValue::from(1.)]).expect("test assertion"),
        DataValue::from(true),
        "2 > 1.0 should be true"
    );
    assert_eq!(
        op_gt(&[DataValue::from(1), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(false),
        "1 > 1 should be false (not strictly greater)"
    );
    assert_eq!(
        op_gt(&[DataValue::from(1), DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(false),
        "1 > 1.0 should be false (not strictly greater)"
    );
    assert_eq!(
        op_gt(&[DataValue::from(1), DataValue::from(2)]).expect("test assertion"),
        DataValue::from(false),
        "1 > 2 should be false"
    );
    assert!(
        op_gt(&[DataValue::Null, DataValue::from(true)]).is_err(),
        "gt with null operand should error"
    );
    assert_eq!(
        op_le(&[DataValue::from(2), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(false),
        "2 <= 1 should be false"
    );
    assert_eq!(
        op_le(&[DataValue::from(2.), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(false),
        "2.0 <= 1 should be false"
    );
    assert_eq!(
        op_le(&[DataValue::from(2), DataValue::from(1.)]).expect("test assertion"),
        DataValue::from(false),
        "2 <= 1.0 should be false"
    );
    assert_eq!(
        op_le(&[DataValue::from(1), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(true),
        "1 <= 1 should be true (equal)"
    );
    assert_eq!(
        op_le(&[DataValue::from(1), DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(true),
        "1 <= 1.0 should be true (equal)"
    );
    assert_eq!(
        op_le(&[DataValue::from(1), DataValue::from(2)]).expect("test assertion"),
        DataValue::from(true),
        "1 <= 2 should be true"
    );
    assert!(
        op_le(&[DataValue::Null, DataValue::from(true)]).is_err(),
        "le with null operand should error"
    );
    assert_eq!(
        op_lt(&[DataValue::from(2), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(false),
        "2 < 1 should be false"
    );
    assert_eq!(
        op_lt(&[DataValue::from(2.), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(false),
        "2.0 < 1 should be false"
    );
    assert_eq!(
        op_lt(&[DataValue::from(2), DataValue::from(1.)]).expect("test assertion"),
        DataValue::from(false),
        "2 < 1.0 should be false"
    );
    assert_eq!(
        op_lt(&[DataValue::from(1), DataValue::from(1)]).expect("test assertion"),
        DataValue::from(false),
        "1 < 1 should be false (not strictly less)"
    );
    assert_eq!(
        op_lt(&[DataValue::from(1), DataValue::from(1.0)]).expect("test assertion"),
        DataValue::from(false),
        "1 < 1.0 should be false (not strictly less)"
    );
    assert_eq!(
        op_lt(&[DataValue::from(1), DataValue::from(2)]).expect("test assertion"),
        DataValue::from(true),
        "1 < 2 should be true"
    );
    assert!(
        op_lt(&[DataValue::Null, DataValue::from(true)]).is_err(),
        "lt with null operand should error"
    );
}

#[test]
fn test_max_min() {
    assert_eq!(
        op_max(&[DataValue::from(1),]).expect("test assertion"),
        DataValue::from(1),
        "max of single element should return that element"
    );
    assert_eq!(
        op_max(&[
            DataValue::from(1),
            DataValue::from(2),
            DataValue::from(3),
            DataValue::from(4)
        ])
        .expect("test assertion"),
        DataValue::from(4),
        "max of ints should return the largest"
    );
    assert_eq!(
        op_max(&[
            DataValue::from(1.0),
            DataValue::from(2),
            DataValue::from(3),
            DataValue::from(4)
        ])
        .expect("test assertion"),
        DataValue::from(4),
        "max with mixed types should return the largest value"
    );
    assert_eq!(
        op_max(&[
            DataValue::from(1),
            DataValue::from(2),
            DataValue::from(3),
            DataValue::from(4.0)
        ])
        .expect("test assertion"),
        DataValue::from(4.0),
        "max with float largest should return float"
    );
    assert!(
        op_max(&[DataValue::from(true)]).is_err(),
        "max with boolean operand should error"
    );

    assert_eq!(
        op_min(&[DataValue::from(1),]).expect("test assertion"),
        DataValue::from(1),
        "min of single element should return that element"
    );
    assert_eq!(
        op_min(&[
            DataValue::from(1),
            DataValue::from(2),
            DataValue::from(3),
            DataValue::from(4)
        ])
        .expect("test assertion"),
        DataValue::from(1),
        "min of ints should return the smallest"
    );
    assert_eq!(
        op_min(&[
            DataValue::from(1.0),
            DataValue::from(2),
            DataValue::from(3),
            DataValue::from(4)
        ])
        .expect("test assertion"),
        DataValue::from(1.0),
        "min with float smallest should return float"
    );
    assert_eq!(
        op_min(&[
            DataValue::from(1),
            DataValue::from(2),
            DataValue::from(3),
            DataValue::from(4.0)
        ])
        .expect("test assertion"),
        DataValue::from(1),
        "min with int smallest should return int"
    );
    assert!(
        op_max(&[DataValue::from(true)]).is_err(),
        "max with boolean operand should error"
    );
}

#[test]
fn test_minus() {
    assert_eq!(
        op_minus(&[DataValue::from(-1)]).expect("test assertion"),
        DataValue::from(1),
        "negation of -1 should yield 1"
    );
    assert_eq!(
        op_minus(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(-1),
        "negation of 1 should yield -1"
    );
    assert_eq!(
        op_minus(&[DataValue::from(f64::INFINITY)]).expect("test assertion"),
        DataValue::from(f64::NEG_INFINITY),
        "negation of infinity should yield negative infinity"
    );
    assert_eq!(
        op_minus(&[DataValue::from(f64::NEG_INFINITY)]).expect("test assertion"),
        DataValue::from(f64::INFINITY),
        "negation of negative infinity should yield infinity"
    );
}

#[test]
fn test_abs() {
    assert_eq!(
        op_abs(&[DataValue::from(-1)]).expect("test assertion"),
        DataValue::from(1),
        "abs of negative int should return positive"
    );
    assert_eq!(
        op_abs(&[DataValue::from(1)]).expect("test assertion"),
        DataValue::from(1),
        "abs of positive int should return same value"
    );
    assert_eq!(
        op_abs(&[DataValue::from(-1.5)]).expect("test assertion"),
        DataValue::from(1.5),
        "abs of negative float should return positive"
    );
}

#[test]
fn test_signum() {
    assert_eq!(
        op_signum(&[DataValue::from(0.1)]).expect("test assertion"),
        DataValue::from(1),
        "signum of positive value should be 1"
    );
    assert_eq!(
        op_signum(&[DataValue::from(-0.1)]).expect("test assertion"),
        DataValue::from(-1),
        "signum of negative value should be -1"
    );
    assert_eq!(
        op_signum(&[DataValue::from(0.0)]).expect("test assertion"),
        DataValue::from(0),
        "signum of positive zero should be 0"
    );
    assert_eq!(
        op_signum(&[DataValue::from(-0.0)]).expect("test assertion"),
        DataValue::from(-1),
        "signum of negative zero should be -1"
    );
    assert_eq!(
        op_signum(&[DataValue::from(-3)]).expect("test assertion"),
        DataValue::from(-1),
        "signum of negative int should be -1"
    );
    assert_eq!(
        op_signum(&[DataValue::from(f64::NEG_INFINITY)]).expect("test assertion"),
        DataValue::from(-1),
        "signum of negative infinity should be -1"
    );
    assert!(
        op_signum(&[DataValue::from(f64::NAN)])
            .expect("test assertion")
            .get_float()
            .expect("test assertion")
            .is_nan(),
        "signum of NaN should return NaN"
    );
}

#[test]
fn test_floor_ceil() {
    assert_eq!(
        op_floor(&[DataValue::from(-1)]).expect("test assertion"),
        DataValue::from(-1),
        "floor of negative int should return same value"
    );
    assert_eq!(
        op_floor(&[DataValue::from(-1.5)]).expect("test assertion"),
        DataValue::from(-2.0),
        "floor of -1.5 should be -2.0"
    );
    assert_eq!(
        op_floor(&[DataValue::from(1.5)]).expect("test assertion"),
        DataValue::from(1.0),
        "floor of 1.5 should be 1.0"
    );
    assert_eq!(
        op_ceil(&[DataValue::from(-1)]).expect("test assertion"),
        DataValue::from(-1),
        "ceil of negative int should return same value"
    );
    assert_eq!(
        op_ceil(&[DataValue::from(-1.5)]).expect("test assertion"),
        DataValue::from(-1.0),
        "ceil of -1.5 should be -1.0"
    );
    assert_eq!(
        op_ceil(&[DataValue::from(1.5)]).expect("test assertion"),
        DataValue::from(2.0),
        "ceil of 1.5 should be 2.0"
    );
}

#[test]
fn test_round() {
    assert_eq!(
        op_round(&[DataValue::from(0.6)]).expect("test assertion"),
        DataValue::from(1.0),
        "0.6 should round up to 1.0"
    );
    assert_eq!(
        op_round(&[DataValue::from(0.5)]).expect("test assertion"),
        DataValue::from(1.0),
        "0.5 should round up to 1.0"
    );
    assert_eq!(
        op_round(&[DataValue::from(1.5)]).expect("test assertion"),
        DataValue::from(2.0),
        "1.5 should round up to 2.0"
    );
    assert_eq!(
        op_round(&[DataValue::from(-0.6)]).expect("test assertion"),
        DataValue::from(-1.0),
        "-0.6 should round to -1.0"
    );
    assert_eq!(
        op_round(&[DataValue::from(-0.5)]).expect("test assertion"),
        DataValue::from(-1.0),
        "-0.5 should round to -1.0"
    );
    assert_eq!(
        op_round(&[DataValue::from(-1.5)]).expect("test assertion"),
        DataValue::from(-2.0),
        "-1.5 should round to -2.0"
    );
}

#[test]
fn test_exp() {
    let n = op_exp(&[DataValue::from(1)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!(((n) - (E)).abs() < 1E-5, "exp(1) should approximate e");

    let n = op_exp(&[DataValue::from(50.1)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!(
        ((n) - (50.1_f64.exp())).abs() < 1E-5,
        "exp(50.1) should match std exp result"
    );
}

#[test]
fn test_exp2() {
    let n = op_exp2(&[DataValue::from(10.)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert_eq!(n, 1024., "2^10 should equal 1024");
}

#[test]
fn test_ln() {
    assert_eq!(
        op_ln(&[DataValue::from(E)]).expect("test assertion"),
        DataValue::from(1.0),
        "ln(e) should be 1.0"
    );
}

#[test]
fn test_log2() {
    assert_eq!(
        op_log2(&[DataValue::from(1024)]).expect("test assertion"),
        DataValue::from(10.),
        "log2(1024) should be 10"
    );
}

#[test]
fn test_log10() {
    assert_eq!(
        op_log10(&[DataValue::from(1000)]).expect("test assertion"),
        DataValue::from(3.0),
        "log10(1000) should be 3.0"
    );
}

#[test]
fn test_trig() {
    let v = op_sin(&[DataValue::from(PI / 2.)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!((v - 1.0).abs() < 1e-5, "sin(π/2) should be ~1.0");
    let v = op_cos(&[DataValue::from(PI / 2.)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!((v - 0.0).abs() < 1e-5, "cos(π/2) should be ~0.0");
    let v = op_tan(&[DataValue::from(PI / 4.)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!((v - 1.0).abs() < 1e-5, "tan(π/4) should be ~1.0");
}

#[test]
fn test_inv_trig() {
    let v = op_asin(&[DataValue::from(1.0)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!((v - PI / 2.).abs() < 1e-5, "asin(1.0) should be ~π/2");
    let v = op_acos(&[DataValue::from(0)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!((v - PI / 2.).abs() < 1e-5, "acos(0) should be ~π/2");
    let v = op_atan(&[DataValue::from(1)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!((v - PI / 4.).abs() < 1e-5, "atan(1) should be ~π/4");
    let v = op_atan2(&[DataValue::from(-1), DataValue::from(-1)])
        .expect("test assertion")
        .get_float()
        .expect("test assertion");
    assert!(
        (v - (-3. * PI / 4.)).abs() < 1e-5,
        "atan2(-1, -1) should be ~-3π/4"
    );
}

#[test]
fn test_pow() {
    assert_eq!(
        op_pow(&[DataValue::from(2), DataValue::from(10)]).expect("test assertion"),
        DataValue::from(1024.0),
        "2^10 should be 1024.0"
    );
}

#[test]
fn test_mod() {
    assert_eq!(
        op_mod(&[DataValue::from(-10), DataValue::from(7)]).expect("test assertion"),
        DataValue::from(-3),
        "-10 mod 7 should be -3 (truncated remainder)"
    );
    assert!(
        op_mod(&[DataValue::from(5), DataValue::from(0.)]).is_ok(),
        "int mod float zero should succeed (produces NaN)"
    );
    assert!(
        op_mod(&[DataValue::from(5.), DataValue::from(0.)]).is_ok(),
        "float mod float zero should succeed (produces NaN)"
    );
    assert!(
        op_mod(&[DataValue::from(5.), DataValue::from(0)]).is_ok(),
        "float mod int zero should succeed (produces NaN)"
    );
    assert!(
        op_mod(&[DataValue::from(5), DataValue::from(0)]).is_err(),
        "int mod int zero should error"
    );
}

#[test]
fn test_boolean() {
    assert_eq!(
        op_and(&[]).expect("test assertion"),
        DataValue::from(true),
        "and with no args should be vacuously true"
    );
    assert_eq!(
        op_and(&[DataValue::from(true), DataValue::from(false)]).expect("test assertion"),
        DataValue::from(false),
        "true and false should be false"
    );
    assert_eq!(
        op_or(&[]).expect("test assertion"),
        DataValue::from(false),
        "or with no args should be vacuously false"
    );
    assert_eq!(
        op_or(&[DataValue::from(true), DataValue::from(false)]).expect("test assertion"),
        DataValue::from(true),
        "true or false should be true"
    );
    assert_eq!(
        op_negate(&[DataValue::from(false)]).expect("test assertion"),
        DataValue::from(true),
        "not false should be true"
    );
}

#[test]
fn test_bits() {
    assert_eq!(
        op_bit_and(&[
            DataValue::Bytes([0b111000].into()),
            DataValue::Bytes([0b010101].into())
        ])
        .expect("test assertion"),
        DataValue::Bytes([0b010000].into()),
        "bitwise AND should produce intersection of set bits"
    );
    assert_eq!(
        op_bit_or(&[
            DataValue::Bytes([0b111000].into()),
            DataValue::Bytes([0b010101].into())
        ])
        .expect("test assertion"),
        DataValue::Bytes([0b111101].into()),
        "bitwise OR should produce union of set bits"
    );
    assert_eq!(
        op_bit_not(&[DataValue::Bytes([0b00111000].into())]).expect("test assertion"),
        DataValue::Bytes([0b11000111].into()),
        "bitwise NOT should flip all bits"
    );
    assert_eq!(
        op_bit_xor(&[
            DataValue::Bytes([0b111000].into()),
            DataValue::Bytes([0b010101].into())
        ])
        .expect("test assertion"),
        DataValue::Bytes([0b101101].into()),
        "bitwise XOR should produce bits set in exactly one operand"
    );
}

#[test]
fn test_pack_bits() {
    assert_eq!(
        op_pack_bits(&[DataValue::List(vec![DataValue::from(true)])]).expect("test assertion"),
        DataValue::Bytes([0b10000000].into()),
        "packing [true] should set the MSB"
    )
}

#[test]
fn test_unpack_bits() {
    assert_eq!(
        op_unpack_bits(&[DataValue::Bytes([0b10101010].into())]).expect("test assertion"),
        DataValue::List(
            [true, false, true, false, true, false, true, false]
                .into_iter()
                .map(DataValue::Bool)
                .collect()
        ),
        "unpacking alternating bits should produce alternating bool list"
    )
}

#[test]
fn test_concat() {
    assert_eq!(
        op_concat(&[DataValue::Str("abc".into()), DataValue::Str("def".into())])
            .expect("test assertion"),
        DataValue::Str("abcdef".into()),
        "string concatenation should join strings"
    );

    assert_eq!(
        op_concat(&[
            DataValue::List(vec![DataValue::from(true), DataValue::from(false)]),
            DataValue::List(vec![DataValue::from(true)])
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::from(true),
            DataValue::from(false),
            DataValue::from(true),
        ]),
        "list concatenation should join lists"
    );
}

#[test]
fn test_str_includes() {
    assert_eq!(
        op_str_includes(&[
            DataValue::Str("abcdef".into()),
            DataValue::Str("bcd".into())
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "str_includes should return true when substring is present"
    );
    assert_eq!(
        op_str_includes(&[DataValue::Str("abcdef".into()), DataValue::Str("bd".into())])
            .expect("test assertion"),
        DataValue::from(false),
        "str_includes should return false when substring is absent"
    );
}

#[test]
fn test_casings() {
    assert_eq!(
        op_lowercase(&[DataValue::Str("NAÏVE".into())]).expect("test assertion"),
        DataValue::Str("naïve".into()),
        "lowercase should handle unicode correctly"
    );
    assert_eq!(
        op_uppercase(&[DataValue::Str("naïve".into())]).expect("test assertion"),
        DataValue::Str("NAÏVE".into()),
        "uppercase should handle unicode correctly"
    );
}

#[test]
fn test_trim() {
    assert_eq!(
        op_trim(&[DataValue::Str(" a ".into())]).expect("test assertion"),
        DataValue::Str("a".into()),
        "trim should remove leading and trailing whitespace"
    );
    assert_eq!(
        op_trim_start(&[DataValue::Str(" a ".into())]).expect("test assertion"),
        DataValue::Str("a ".into()),
        "trim_start should remove only leading whitespace"
    );
    assert_eq!(
        op_trim_end(&[DataValue::Str(" a ".into())]).expect("test assertion"),
        DataValue::Str(" a".into()),
        "trim_end should remove only trailing whitespace"
    );
}

#[test]
fn test_starts_ends_with() {
    assert_eq!(
        op_starts_with(&[
            DataValue::Str("abcdef".into()),
            DataValue::Str("abc".into())
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "string starting with given prefix should return true"
    );
    assert_eq!(
        op_starts_with(&[DataValue::Str("abcdef".into()), DataValue::Str("bc".into())])
            .expect("test assertion"),
        DataValue::from(false),
        "string not starting with given prefix should return false"
    );
    assert_eq!(
        op_ends_with(&[
            DataValue::Str("abcdef".into()),
            DataValue::Str("def".into())
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "string ending with given suffix should return true"
    );
    assert_eq!(
        op_ends_with(&[DataValue::Str("abcdef".into()), DataValue::Str("bc".into())])
            .expect("test assertion"),
        DataValue::from(false),
        "string not ending with given suffix should return false"
    );
}

#[test]
fn test_regex() {
    assert_eq!(
        op_regex_matches(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("c.e").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "regex matching interior substring should return true"
    );

    assert_eq!(
        op_regex_matches(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("c.ef$").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::from(true),
        "regex matching at end of string should return true"
    );

    assert_eq!(
        op_regex_matches(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("c.e$").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::from(false),
        "regex requiring end anchor that does not match should return false"
    );

    assert_eq!(
        op_regex_replace(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("[be]").expect("test assertion"))),
            DataValue::Str("x".into())
        ])
        .expect("test assertion"),
        DataValue::Str("axcdef".into()),
        "regex_replace should replace only the first match"
    );

    assert_eq!(
        op_regex_replace_all(&[
            DataValue::Str("abcdef".into()),
            DataValue::Regex(RegexWrapper(Regex::new("[be]").expect("test assertion"))),
            DataValue::Str("x".into())
        ])
        .expect("test assertion"),
        DataValue::Str("axcdxf".into()),
        "regex_replace_all should replace all matches"
    );
    assert_eq!(
        op_regex_extract(&[
            DataValue::Str("abCDefGH".into()),
            DataValue::Regex(RegexWrapper(
                Regex::new("[xayef]|(GH)").expect("test assertion")
            ))
        ])
        .expect("test assertion"),
        DataValue::List(vec![
            DataValue::Str("a".into()),
            DataValue::Str("e".into()),
            DataValue::Str("f".into()),
            DataValue::Str("GH".into()),
        ]),
        "regex_extract should return all matches in order"
    );
    assert_eq!(
        op_regex_extract_first(&[
            DataValue::Str("abCDefGH".into()),
            DataValue::Regex(RegexWrapper(
                Regex::new("[xayef]|(GH)").expect("test assertion")
            ))
        ])
        .expect("test assertion"),
        DataValue::Str("a".into()),
        "regex_extract_first should return first match"
    );
    assert_eq!(
        op_regex_extract(&[
            DataValue::Str("abCDefGH".into()),
            DataValue::Regex(RegexWrapper(Regex::new("xyz").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::List(vec![]),
        "regex_extract with no matches should return empty list"
    );

    assert_eq!(
        op_regex_extract_first(&[
            DataValue::Str("abCDefGH".into()),
            DataValue::Regex(RegexWrapper(Regex::new("xyz").expect("test assertion")))
        ])
        .expect("test assertion"),
        DataValue::Null,
        "regex_extract_first with no matches should return Null"
    );
}

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
