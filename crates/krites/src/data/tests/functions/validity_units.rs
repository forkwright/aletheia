//! Regression tests for the Validity microsecond/second unit boundary.
#![expect(clippy::expect_used, reason = "test assertions")]

use crate::data::functions::*;
use crate::data::relation::{ColType, NullableColType};
use std::cmp::Reverse;

use crate::data::value::{DataValue, Num, Validity, ValidityTs};

/// WHY(#6656 / upstream cozo#312): `Validity` is denominated in microseconds, while `now()` and
/// `parse_timestamp()` return float **seconds**. The permissive `get_int` accepted any whole-valued
/// float and cast it, so a timestamp piped from one into the other was reinterpreted a million-fold
/// smaller — writing the row at 1970-01-01 permanently, and reading back as zero rows with no error.
///
/// These pin the boundary rather than the arithmetic: a float in a unit-carrying position must be
/// **rejected**, not silently coerced.
#[test]
fn float_seconds_are_rejected_where_microseconds_are_expected() {
    // parse_timestamp yields float seconds; feeding that to validity() is the corruption path.
    let secs = op_parse_timestamp(&[DataValue::Str("2026-01-01T00:00:00Z".into())])
        .expect("parse_timestamp");
    assert!(
        matches!(secs, DataValue::Num(Num::Float(_))),
        "parse_timestamp must return a float; if this changes, the whole hazard changes shape"
    );

    let err = op_validity(&[secs]);
    assert!(
        err.is_err(),
        "validity() must reject float seconds rather than reinterpreting them as microseconds"
    );
}

#[test]
fn integer_microseconds_are_still_accepted() {
    let v = op_validity(&[DataValue::Num(Num::Int(1_767_225_600_000_000))]).expect("validity");
    assert!(matches!(v, DataValue::Validity(_)));
}

/// A small integer is a legitimate `Validity` — it is an abstract logical clock, not wall time.
/// This is why the fix is a type check and not a magnitude heuristic: a "reject below 1e12" guard
/// would refuse this.
#[test]
fn small_integer_validity_is_legitimate() {
    let v = op_validity(&[DataValue::Num(Num::Int(250))]).expect("validity");
    match v {
        DataValue::Validity(Validity { timestamp, .. }) => {
            assert_eq!(timestamp.0.0, 250, "small logical-clock values must survive");
        }
        other => panic!("expected Validity, got {other:?}"),
    }
}

#[test]
fn list_coercion_rejects_float_timestamp() {
    let col = NullableColType {
        coltype: ColType::Validity,
        nullable: false,
    };
    // [float_seconds, is_assert] — the shape `:put` accepts for a validity column.
    let bad = DataValue::List(vec![
        DataValue::Num(Num::Float(1_767_225_600.0)),
        DataValue::Bool(true),
    ]);
    assert!(
        col.coerce(bad, ValidityTs(Reverse(0))).is_err(),
        "a float timestamp in a validity list must be rejected at coercion, before it is written"
    );

    let good = DataValue::List(vec![
        DataValue::Num(Num::Int(1_767_225_600_000_000)),
        DataValue::Bool(true),
    ]);
    assert!(
        col.coerce(good, ValidityTs(Reverse(0))).is_ok(),
        "integer microseconds still coerce"
    );
}

#[test]
fn get_int_strict_rejects_whole_floats_that_get_int_accepts() {
    let whole = Num::Float(42.0);
    assert_eq!(
        whole.get_int(),
        Some(42),
        "permissive accessor keeps its behaviour for unit-less contexts"
    );
    assert_eq!(
        whole.get_int_strict(),
        None,
        "strict accessor rejects it, which is the whole point"
    );
    assert_eq!(Num::Int(42).get_int_strict(), Some(42));
}
