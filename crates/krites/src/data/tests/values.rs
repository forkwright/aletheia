//! Tests for core value type.
#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::{BTreeMap, HashMap};
use std::mem::size_of;

use crate::data::symb::Symbol;
use crate::data::value::DataValue;

#[test]
fn type_size_consistency() {
    // Verify that type sizes are as expected for memory layout optimization.
    // These assertions catch unexpected size changes from dependency updates
    // or struct modifications.
    let datavalue_size = size_of::<DataValue>();
    let symbol_size = size_of::<Symbol>();
    let string_size = size_of::<String>();
    let hashmap_size = size_of::<HashMap<String, String>>();
    let btreemap_size = size_of::<BTreeMap<String, String>>();

    // DataValue is an enum with multiple variants; expect reasonable size
    assert!(
        datavalue_size >= 16 && datavalue_size <= 64,
        "DataValue size {datavalue_size} seems unreasonable"
    );

    // Symbol contains a CompactString and SourceSpan
    assert!(
        symbol_size >= 16 && symbol_size <= 48,
        "Symbol size {symbol_size} seems unreasonable"
    );

    // Standard String is 24 bytes on 64-bit systems (ptr + len + capacity)
    assert_eq!(
        string_size, 24,
        "String size should be 24 bytes on 64-bit systems"
    );

    // HashMap has some overhead for the hash table
    assert!(
        hashmap_size >= 32 && hashmap_size <= 64,
        "HashMap size {hashmap_size} seems unreasonable"
    );

    // BTreeMap has node overhead
    assert!(
        btreemap_size >= 16 && btreemap_size <= 48,
        "BTreeMap size {btreemap_size} seems unreasonable"
    );
}

#[test]
fn utf8() {
    let c = char::from_u32(0x10FFFF).expect("test assertion");
    let mut s = String::new();
    s.push(c);
    println!("{}", s);
    println!(
        "{:b} {:b} {:b} {:b}",
        s.as_bytes()[0],
        s.as_bytes()[1],
        s.as_bytes()[2],
        s.as_bytes()[3]
    );
    println!("{s:?}");
}

#[test]
fn display_datavalues() {
    // Verify Display trait implementations for DataValue variants
    assert_eq!(format!("{}", DataValue::Null), "null");
    assert_eq!(format!("{}", DataValue::from(true)), "true");
    assert_eq!(format!("{}", DataValue::from(-1)), "-1");
    assert_eq!(
        format!("{}", DataValue::from(-1_121_212_121.331_212_f64)),
        "-1121212121.331212"
    );
    // Special floats display as function calls that reconstruct them
    assert_eq!(
        format!("{}", DataValue::from(f64::NAN)),
        r#"to_float("NAN")"#
    );
    assert_eq!(
        format!("{}", DataValue::from(f64::NEG_INFINITY)),
        r#"to_float("NEG_INF")"#
    );

    // List formatting with mixed content including special characters
    let list = DataValue::List(vec![
        DataValue::from(false),
        DataValue::from(r###"abc"你"好'啊👌"###),
        DataValue::from(f64::NEG_INFINITY),
    ]);
    assert_eq!(
        format!("{}", list),
        r#"[false, "abc\"你\"好'啊👌", to_float("NEG_INF")]"#
    );
}
