//! Tests for memory-comparable encoding.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions: index into known-size encoding buffers"
)]
#![expect(
    clippy::unreadable_literal,
    reason = "test: numeric literals mirror tested bit patterns verbatim"
)]
use koina::uuid::Uuid;

use crate::data::memcmp::{MemCmpEncoder, decode_bytes};
use crate::data::value::{DataValue, Num, UuidWrapper};

#[test]
fn encode_decode_num() {
    use rand::prelude::*;

    let n = i64::MAX;
    let mut collected = vec![];

    let mut test_num = |n: Num| {
        let mut encoder = vec![];
        encoder.encode_num(n);
        let (decoded, rest) = Num::decode_from_key(&encoder);
        assert_eq!(decoded, n);
        assert!(rest.is_empty());
        collected.push(encoder);
    };
    for i in 0..54 {
        for j in 0..1000 {
            let vb = (n >> i) - j;
            for v in [vb, -vb - 1] {
                test_num(Num::Int(v));
            }
        }
    }
    test_num(Num::Float(f64::INFINITY));
    test_num(Num::Float(f64::NEG_INFINITY));
    test_num(Num::Float(f64::NAN));
    for _ in 0..100000 {
        let f = (rand::rng().random::<f64>() - 0.5) * 2.0;
        test_num(Num::Float(f));
        test_num(Num::Float(1. / f));
    }
    let mut collected_copy = collected.clone();
    collected.sort();
    collected_copy.sort_by_key(|c| Num::decode_from_key(c).0);
    assert_eq!(collected, collected_copy);
}

#[test]
fn encode_decode_uuid_roundtrips_correctly() {
    let uuid = DataValue::Uuid(UuidWrapper(
        Uuid::parse_str("dd85b19a-5fde-11ed-a88e-1774a7698039").expect("test assertion"),
    ));
    let mut encoder = vec![];
    encoder.encode_datavalue(&uuid);
    let (decoded, remaining) = DataValue::decode_from_key(&encoder);
    assert_eq!(decoded, uuid);
    assert!(remaining.is_empty());
}

#[test]
fn encode_decode_bytes() {
    let target = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit...";
    for i in 0..target.len() {
        let bs = &target[i..];
        let mut encoder: Vec<u8> = vec![];
        encoder.encode_bytes(bs);
        let (decoded, remaining) = decode_bytes(&encoder);
        assert!(remaining.is_empty());
        assert_eq!(bs, decoded);

        let mut encoder: Vec<u8> = vec![];
        encoder.encode_bytes(target);
        encoder.encode_bytes(bs);
        encoder.encode_bytes(bs);
        encoder.encode_bytes(target);

        let (decoded, remaining) = decode_bytes(&encoder);
        assert_eq!(&target[..], decoded);

        let (decoded, remaining) = decode_bytes(remaining);
        assert_eq!(bs, decoded);

        let (decoded, remaining) = decode_bytes(remaining);
        assert_eq!(bs, decoded);

        let (decoded, remaining) = decode_bytes(remaining);
        assert_eq!(&target[..], decoded);
        assert!(remaining.is_empty());
    }
}

#[test]
fn specific_encode() {
    let mut encoder = vec![];
    encoder.encode_datavalue(&DataValue::from(2095));
    encoder.encode_datavalue(&DataValue::from("MSS"));
    let (a, remaining) = DataValue::decode_from_key(&encoder);
    let (b, remaining) = DataValue::decode_from_key(remaining);
    assert!(remaining.is_empty());
    assert_eq!(a, DataValue::from(2095));
    assert_eq!(b, DataValue::from("MSS"));
}

/// Roundtrip well-formed UTF-8 strings through `encode_datavalue` /
/// `decode_from_key`. Mixes ASCII, multi-byte codepoints, empties, and a
/// length that straddles the `decode_bytes` continuation boundary — the
/// decode path that was previously `unsafe { String::from_utf8_unchecked }`.
///
/// Miri-safe: no mmap, no FFI, no OS resources.
#[cfg_attr(miri, test)]
#[test]
fn str_decode_roundtrips_across_utf8_shapes() {
    let cases: &[&str] = &[
        "",
        "ascii only",
        "two-byte \u{00e9}\u{00f1}",                           // é ñ
        "three-byte \u{3053}\u{3093}\u{306b}\u{3061}\u{306f}", // こんにちは
        "four-byte emoji \u{1f98a}\u{1f680}",                  // 🦊 🚀
        "mixed \u{00e9} ascii \u{1f600} tail",                 // é + 😀
    ];
    for s in cases {
        let mut enc: Vec<u8> = vec![];
        enc.encode_datavalue(&DataValue::from(*s));
        let (decoded, rest) = DataValue::decode_from_key(&enc);
        assert!(rest.is_empty(), "trailing bytes after str decode: {s:?}");
        assert_eq!(
            decoded,
            DataValue::from(*s),
            "str roundtrip mismatch for {s:?}"
        );
    }
}

/// Corrupt-payload simulation: construct a key whose `encode_bytes` framing
/// is well-formed but whose *payload* bytes are deliberately non-UTF-8 (a
/// bare 0xFF continuation byte and a lone 0xC0 start byte). This is the
/// exact invariant boundary that the former `unsafe { String::from_utf8_unchecked }`
/// relied on — "payload bytes are valid UTF-8 because we wrote them from a
/// `&str`". A bit-rot or tamper at the bytes layer would have triggered UB.
/// After the audit, decode uses `String::from_utf8_lossy`, so invalid bytes
/// become U+FFFD and the function remains total/safe.
///
/// Miri-safe: no mmap, no FFI, no OS resources.
#[cfg_attr(miri, test)]
#[test]
fn str_decode_tolerates_corrupt_utf8_without_ub() {
    use crate::data::memcmp::MemCmpEncoder;

    // 0xFF and 0xC0 are both invalid as UTF-8 lead bytes.
    let invalid_utf8: &[u8] = &[0x68, 0xFF, 0xC0, 0x69]; // "h?\u{?}i"
    // Build a key whose framing parses (STR_TAG + encode_bytes) but whose
    // contents are not valid UTF-8.
    let mut enc: Vec<u8> = vec![];
    // 0x06 = STR_TAG — replicate the encoder's on-wire layout.
    enc.push(0x06);
    enc.encode_bytes(invalid_utf8);

    let (decoded, rest) = DataValue::decode_from_key(&enc);
    assert!(rest.is_empty(), "trailing bytes after corrupt-str decode");

    // Must decode to a Str, not panic. Payload must contain the replacement
    // character where bytes were invalid, and preserve valid ASCII elsewhere.
    match decoded {
        DataValue::Str(s) => {
            // Both ASCII ends survive; replacement char occupies the invalid
            // region (the exact count of U+FFFD depends on lossy grouping).
            assert!(s.starts_with('h'), "valid leading ASCII preserved: {s:?}");
            assert!(s.ends_with('i'), "valid trailing ASCII preserved: {s:?}");
            assert!(
                s.contains('\u{FFFD}'),
                "invalid UTF-8 replaced with U+FFFD: {s:?}"
            );
        }
        other => panic!("expected DataValue::Str, got {other:?}"),
    }
}

#[test]
fn encode_decode_datavalues() {
    let mut dv = vec![
        DataValue::Null,
        DataValue::from(false),
        DataValue::from(true),
        DataValue::from(1),
        DataValue::from(1.0),
        DataValue::from(i64::MAX),
        DataValue::from(i64::MAX - 1),
        DataValue::from(i64::MAX - 2),
        DataValue::from(i64::MIN),
        DataValue::from(i64::MIN + 1),
        DataValue::from(i64::MIN + 2),
        DataValue::from(f64::INFINITY),
        DataValue::from(f64::NEG_INFINITY),
        DataValue::List(vec![]),
    ];
    dv.push(DataValue::List(dv.clone()));
    dv.push(DataValue::List(dv.clone()));
    let mut encoded = vec![];
    let v = DataValue::List(dv);
    encoded.encode_datavalue(&v);
    let (decoded, remaining) = DataValue::decode_from_key(&encoded);
    assert!(remaining.is_empty());
    assert_eq!(decoded, v);
}
