//! Tests for arithmetic, comparison, boolean, and bit operations.
#![expect(clippy::expect_used, reason = "test assertions")]
use std::f64::consts::{E, PI};

use crate::engine::data::functions::*;
use crate::engine::data::value::DataValue;

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
