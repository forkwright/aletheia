//! Property tests for `DataValue` memcmp and serde serialization round-trips.
//!
//! Generates random `DataValue` variants, serializes with memcmp encoding,
//! deserializes, and verifies equality. Also tests rmp-serde round-trips
//! and sort-order preservation.
#![expect(clippy::expect_used, reason = "test assertions")]

use std::collections::BTreeSet;

use compact_str::CompactString;
use ndarray::Array1;
use proptest::prelude::*;
use uuid::Uuid;

use crate::data::memcmp::MemCmpEncoder;
use crate::data::value::{DataValue, JsonData, Num, UuidWrapper, Validity, Vector};

// ── Proptest strategies for DataValue variants ──────────────────────────────

fn arb_num() -> impl Strategy<Value = Num> {
    prop_oneof![
        any::<i64>().prop_map(Num::Int),
        any::<f64>().prop_map(Num::Float),
    ]
}

fn arb_uuid() -> impl Strategy<Value = UuidWrapper> {
    any::<[u8; 16]>().prop_map(|bytes| UuidWrapper(Uuid::from_bytes(bytes)))
}

fn arb_validity() -> impl Strategy<Value = Validity> {
    (any::<i64>(), any::<bool>()).prop_map(|(ts, assert)| Validity::from((ts, assert)))
}

fn arb_vector_f32(max_len: usize) -> impl Strategy<Value = Vector> {
    proptest::collection::vec(any::<f32>(), 0..=max_len).prop_map(|v| Vector::F32(Array1::from(v)))
}

fn arb_vector_f64(max_len: usize) -> impl Strategy<Value = Vector> {
    proptest::collection::vec(any::<f64>(), 0..=max_len).prop_map(|v| Vector::F64(Array1::from(v)))
}

fn arb_vector() -> impl Strategy<Value = Vector> {
    prop_oneof![arb_vector_f32(8), arb_vector_f64(8),]
}

/// Leaf `DataValue` strategy -- no recursive List/Set.
fn arb_datavalue_leaf() -> impl Strategy<Value = DataValue> {
    prop_oneof![
        Just(DataValue::Null),
        any::<bool>().prop_map(DataValue::Bool),
        arb_num().prop_map(DataValue::Num),
        "[a-zA-Z0-9 ]{0,32}".prop_map(|s| DataValue::Str(CompactString::from(s))),
        proptest::collection::vec(any::<u8>(), 0..16).prop_map(DataValue::Bytes),
        arb_uuid().prop_map(DataValue::Uuid),
        arb_vector().prop_map(DataValue::Vec),
        arb_validity().prop_map(DataValue::Validity),
        Just(DataValue::Bot),
    ]
}

/// JSON leaf strategy -- objects that survive serde round-trip.
fn arb_json_datavalue() -> impl Strategy<Value = DataValue> {
    prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        (-1000i64..1000i64).prop_map(|n| serde_json::Value::Number(n.into())),
        "[a-z]{0,8}".prop_map(serde_json::Value::String),
    ]
    .prop_map(|j| DataValue::Json(JsonData(j)))
}

/// Full `DataValue` strategy with 1-level nesting.
fn arb_datavalue() -> impl Strategy<Value = DataValue> {
    arb_datavalue_leaf().prop_recursive(1, 16, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(DataValue::List),
            proptest::collection::btree_set(inner, 0..4).prop_map(DataValue::Set),
        ]
    })
}

// ── Memcmp round-trip tests ─────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Every DataValue leaf variant survives memcmp encode/decode.
    #[test]
    fn memcmp_roundtrip_leaf(val in arb_datavalue_leaf()) {
        let mut buf = vec![];
        buf.encode_datavalue(&val);
        let (decoded, rest) = DataValue::decode_from_key(&buf);
        prop_assert!(rest.is_empty(), "remaining bytes should be empty after decode");
        prop_assert_eq!(decoded, val);
    }

    /// DataValue with nesting survives memcmp encode/decode.
    #[test]
    fn memcmp_roundtrip_nested(val in arb_datavalue()) {
        let mut buf = vec![];
        buf.encode_datavalue(&val);
        let (decoded, rest) = DataValue::decode_from_key(&buf);
        prop_assert!(rest.is_empty(), "remaining bytes should be empty after decode");
        prop_assert_eq!(decoded, val);
    }

    /// JSON DataValue survives memcmp encode/decode.
    #[test]
    fn memcmp_roundtrip_json(val in arb_json_datavalue()) {
        let mut buf = vec![];
        buf.encode_datavalue(&val);
        let (decoded, rest) = DataValue::decode_from_key(&buf);
        prop_assert!(rest.is_empty(), "remaining bytes should be empty after decode");
        prop_assert_eq!(decoded, val);
    }

    /// Num encode/decode preserves exact value.
    #[test]
    fn memcmp_roundtrip_num(n in arb_num()) {
        let mut buf = vec![];
        buf.encode_num(n);
        let (decoded, rest) = Num::decode_from_key(&buf);
        prop_assert!(rest.is_empty(), "remaining bytes should be empty");
        prop_assert_eq!(decoded, n);
    }

    /// Multiple values concatenated decode correctly.
    #[test]
    fn memcmp_roundtrip_concatenated(
        a in arb_datavalue_leaf(),
        b in arb_datavalue_leaf(),
    ) {
        let mut buf = vec![];
        buf.encode_datavalue(&a);
        buf.encode_datavalue(&b);
        let (decoded_a, rest) = DataValue::decode_from_key(&buf);
        let (decoded_b, rest) = DataValue::decode_from_key(rest);
        prop_assert!(rest.is_empty(), "remaining bytes should be empty");
        prop_assert_eq!(decoded_a, a);
        prop_assert_eq!(decoded_b, b);
    }
}

// ── Sort-order preservation ─────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Memcmp byte ordering matches DataValue Ord ordering for same-type pairs.
    #[test]
    fn memcmp_preserves_sort_order_nums(a in arb_num(), b in arb_num()) {
        let mut buf_a = vec![];
        let mut buf_b = vec![];
        buf_a.encode_num(a);
        buf_b.encode_num(b);
        let ord_val = a.cmp(&b);
        let ord_bytes = buf_a.cmp(&buf_b);
        prop_assert_eq!(ord_val, ord_bytes,
            "Num Ord and memcmp byte order must agree: {:?} vs {:?}", a, b);
    }

    /// Memcmp byte ordering matches DataValue Ord for string values.
    #[test]
    fn memcmp_preserves_sort_order_strings(
        a in "[a-z]{0,8}",
        b in "[a-z]{0,8}",
    ) {
        let va = DataValue::Str(CompactString::from(&a));
        let vb = DataValue::Str(CompactString::from(&b));
        let mut buf_a = vec![];
        let mut buf_b = vec![];
        buf_a.encode_datavalue(&va);
        buf_b.encode_datavalue(&vb);
        prop_assert_eq!(va.cmp(&vb), buf_a.cmp(&buf_b),
            "String Ord and memcmp byte order must agree");
    }
}

// ── Serde (rmp) round-trip tests ────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// DataValue survives rmp-serde serialize/deserialize round-trip.
    /// Regex and Set are excluded because Regex is transient (not serializable)
    /// and Set is internal-only.
    #[test]
    fn rmp_serde_roundtrip_leaf(val in prop_oneof![
        Just(DataValue::Null),
        any::<bool>().prop_map(DataValue::Bool),
        arb_num().prop_map(DataValue::Num),
        "[a-zA-Z0-9]{0,16}".prop_map(|s| DataValue::Str(CompactString::from(s))),
        proptest::collection::vec(any::<u8>(), 0..16).prop_map(DataValue::Bytes),
        arb_uuid().prop_map(DataValue::Uuid),
        arb_vector().prop_map(DataValue::Vec),
        arb_validity().prop_map(DataValue::Validity),
        Just(DataValue::Bot),
    ]) {
        let serialized = rmp_serde::to_vec(&val)
            .expect("rmp serialization should succeed");
        let deserialized: DataValue = rmp_serde::from_slice(&serialized)
            .expect("rmp deserialization should succeed");
        prop_assert_eq!(deserialized, val);
    }

    /// DataValue with nested list survives rmp-serde round-trip.
    #[test]
    fn rmp_serde_roundtrip_list(vals in proptest::collection::vec(
        prop_oneof![
            Just(DataValue::Null),
            any::<bool>().prop_map(DataValue::Bool),
            any::<i64>().prop_map(|i| DataValue::Num(Num::Int(i))),
            "[a-z]{0,8}".prop_map(|s| DataValue::Str(CompactString::from(s))),
        ],
        0..6
    )) {
        let val = DataValue::List(vals);
        let serialized = rmp_serde::to_vec(&val)
            .expect("rmp serialization of list should succeed");
        let deserialized: DataValue = rmp_serde::from_slice(&serialized)
            .expect("rmp deserialization of list should succeed");
        prop_assert_eq!(deserialized, val);
    }
}

// ── Deterministic edge-case tests ───────────────────────────────────────────

#[test]
fn memcmp_roundtrip_empty_string() {
    let val = DataValue::Str(CompactString::from(""));
    let mut buf = vec![];
    buf.encode_datavalue(&val);
    let (decoded, rest) = DataValue::decode_from_key(&buf);
    assert!(rest.is_empty());
    assert_eq!(decoded, val);
}

#[test]
fn memcmp_roundtrip_empty_bytes() {
    let val = DataValue::Bytes(vec![]);
    let mut buf = vec![];
    buf.encode_datavalue(&val);
    let (decoded, rest) = DataValue::decode_from_key(&buf);
    assert!(rest.is_empty());
    assert_eq!(decoded, val);
}

#[test]
fn memcmp_roundtrip_empty_list() {
    let val = DataValue::List(vec![]);
    let mut buf = vec![];
    buf.encode_datavalue(&val);
    let (decoded, rest) = DataValue::decode_from_key(&buf);
    assert!(rest.is_empty());
    assert_eq!(decoded, val);
}

#[test]
fn memcmp_roundtrip_empty_set() {
    let val = DataValue::Set(BTreeSet::new());
    let mut buf = vec![];
    buf.encode_datavalue(&val);
    let (decoded, rest) = DataValue::decode_from_key(&buf);
    assert!(rest.is_empty());
    assert_eq!(decoded, val);
}

#[test]
fn memcmp_roundtrip_empty_vectors() {
    for val in [
        DataValue::Vec(Vector::F32(Array1::from(vec![]))),
        DataValue::Vec(Vector::F64(Array1::from(vec![]))),
    ] {
        let mut buf = vec![];
        buf.encode_datavalue(&val);
        let (decoded, rest) = DataValue::decode_from_key(&buf);
        assert!(rest.is_empty());
        assert_eq!(decoded, val);
    }
}

#[test]
fn memcmp_roundtrip_extreme_ints() {
    for v in [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX] {
        let val = DataValue::from(v);
        let mut buf = vec![];
        buf.encode_datavalue(&val);
        let (decoded, rest) = DataValue::decode_from_key(&buf);
        assert!(rest.is_empty(), "remaining bytes for int {v}");
        assert_eq!(decoded, val, "round-trip failed for int {v}");
    }
}

#[test]
fn memcmp_roundtrip_special_floats() {
    for f in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -0.0] {
        let val = DataValue::from(f);
        let mut buf = vec![];
        buf.encode_datavalue(&val);
        let (decoded, rest) = DataValue::decode_from_key(&buf);
        assert!(rest.is_empty(), "remaining bytes for float {f}");
        assert_eq!(decoded, val, "round-trip failed for float {f}");
    }
}

#[test]
fn memcmp_tag_ordering_within_type_is_consistent() {
    // Within each DataValue variant, memcmp encoding preserves the Ord order.
    // Cross-variant ordering may differ between memcmp tags and Rust's derive(Ord)
    // because the memcmp tag assignment is optimized for storage layout, not for
    // matching the Rust enum discriminant order.
    let pairs: Vec<(DataValue, DataValue)> = vec![
        (DataValue::Bool(false), DataValue::Bool(true)),
        (DataValue::from(0i64), DataValue::from(1i64)),
        (DataValue::from(-1i64), DataValue::from(0i64)),
        (DataValue::from("a"), DataValue::from("b")),
        (DataValue::from(""), DataValue::from("a")),
        (DataValue::Bytes(vec![0]), DataValue::Bytes(vec![1])),
        (DataValue::Bytes(vec![]), DataValue::Bytes(vec![0])),
    ];

    for (a, b) in &pairs {
        let mut buf_a = vec![];
        let mut buf_b = vec![];
        buf_a.encode_datavalue(a);
        buf_b.encode_datavalue(b);
        assert!(
            buf_a < buf_b,
            "memcmp byte order violated within type: {a:?} should sort before {b:?}"
        );
        assert!(
            a < b,
            "DataValue Ord violated: {a:?} should sort before {b:?}"
        );
    }
}

#[test]
fn memcmp_null_sorts_before_everything() {
    // Null has tag 0x01, which is the lowest non-INIT tag. It should sort before
    // all other DataValue variants in memcmp encoding.
    let null = DataValue::Null;
    let others = vec![
        DataValue::Bool(false),
        DataValue::from(0i64),
        DataValue::from(""),
        DataValue::Bytes(vec![]),
        DataValue::Bot,
    ];

    let mut null_buf = vec![];
    null_buf.encode_datavalue(&null);
    for other in &others {
        let mut buf = vec![];
        buf.encode_datavalue(other);
        assert!(
            null_buf < buf,
            "Null should sort before {other:?} in memcmp encoding"
        );
    }
}

#[test]
fn memcmp_bot_sorts_after_everything() {
    // Bot has tag 0xFF, which should sort after all other variants.
    let bot = DataValue::Bot;
    let others = vec![
        DataValue::Null,
        DataValue::Bool(true),
        DataValue::from(i64::MAX),
        DataValue::from("zzz"),
        DataValue::Bytes(vec![0xFF]),
    ];

    let mut bot_buf = vec![];
    bot_buf.encode_datavalue(&bot);
    for other in &others {
        let mut buf = vec![];
        buf.encode_datavalue(other);
        assert!(
            buf < bot_buf,
            "{other:?} should sort before Bot in memcmp encoding"
        );
    }
}

#[test]
fn rmp_serde_roundtrip_bot_and_null() {
    for val in [DataValue::Null, DataValue::Bot] {
        let serialized =
            rmp_serde::to_vec(&val).expect("rmp serialization should succeed for Null/Bot");
        let deserialized: DataValue = rmp_serde::from_slice(&serialized)
            .expect("rmp deserialization should succeed for Null/Bot");
        assert_eq!(deserialized, val);
    }
}
