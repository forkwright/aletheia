//! Tests for core value type.
#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::{BTreeMap, HashMap};
use std::mem::size_of;

use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;

#[test]
fn show_size() {
    println!("DataValue size: {}", size_of::<DataValue>());
    println!("Symbol size: {}", size_of::<Symbol>());
    println!("String size: {}", size_of::<String>());
    println!(
        "HashMap<String,String> size: {}",
        size_of::<HashMap<String, String>>()
    );
    println!(
        "BTreeMap<String,String> size: {}",
        size_of::<BTreeMap<String, String>>()
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
    println!("{}", DataValue::Null);
    println!("{}", DataValue::from(true));
    println!("{}", DataValue::from(-1));
    println!("{}", DataValue::from(-1_121_212_121.331_212_f64));
    println!("{}", DataValue::from(f64::NAN));
    println!("{}", DataValue::from(f64::NEG_INFINITY));
    println!(
        "{}",
        DataValue::List(vec![
            DataValue::from(false),
            DataValue::from(r###"abc"你"好'啊👌"###),
            DataValue::from(f64::NEG_INFINITY),
        ])
    );
}
