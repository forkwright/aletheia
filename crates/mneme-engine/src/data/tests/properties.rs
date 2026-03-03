use proptest::prelude::*;

use crate::data::memcmp::MemCmpEncoder;
use crate::data::value::{DataValue, Num};

fn scalar_strategy() -> impl Strategy<Value = DataValue> {
    prop_oneof![
        Just(DataValue::Null),
        any::<bool>().prop_map(DataValue::Bool),
        any::<i64>().prop_map(|n| DataValue::Num(Num::Int(n))),
        "[a-zA-Z0-9 ]{0,50}".prop_map(|s: String| DataValue::Str(s.into())),
        proptest::collection::vec(any::<u8>(), 0..50).prop_map(DataValue::Bytes),
    ]
}

proptest! {
    #[test]
    fn null_roundtrips(_x in Just(DataValue::Null)) {
        let mut enc = vec![];
        enc.encode_datavalue(&DataValue::Null);
        let (decoded, rest) = DataValue::decode_from_key(&enc);
        prop_assert!(rest.is_empty());
        prop_assert_eq!(decoded, DataValue::Null);
    }

    #[test]
    fn bool_roundtrips(b in any::<bool>()) {
        let v = DataValue::Bool(b);
        let mut enc = vec![];
        enc.encode_datavalue(&v);
        let (decoded, rest) = DataValue::decode_from_key(&enc);
        prop_assert!(rest.is_empty());
        prop_assert_eq!(decoded, v);
    }

    #[test]
    fn int_roundtrips(n in any::<i64>()) {
        let v = DataValue::Num(Num::Int(n));
        let mut enc = vec![];
        enc.encode_datavalue(&v);
        let (decoded, rest) = DataValue::decode_from_key(&enc);
        prop_assert!(rest.is_empty());
        prop_assert_eq!(decoded, v);
    }

    #[test]
    fn string_roundtrips(s in "[a-zA-Z0-9 ]{0,50}") {
        let v = DataValue::Str(s.into());
        let mut enc = vec![];
        enc.encode_datavalue(&v);
        let (decoded, rest) = DataValue::decode_from_key(&enc);
        prop_assert!(rest.is_empty());
        prop_assert_eq!(decoded, v);
    }

    #[test]
    fn bytes_roundtrips(bs in proptest::collection::vec(any::<u8>(), 0..50)) {
        let v = DataValue::Bytes(bs);
        let mut enc = vec![];
        enc.encode_datavalue(&v);
        let (decoded, rest) = DataValue::decode_from_key(&enc);
        prop_assert!(rest.is_empty());
        prop_assert_eq!(decoded, v);
    }

    #[test]
    fn int_encoding_preserves_order(a in any::<i64>(), b in any::<i64>()) {
        let mut enc_a = vec![];
        enc_a.encode_datavalue(&DataValue::Num(Num::Int(a)));
        let mut enc_b = vec![];
        enc_b.encode_datavalue(&DataValue::Num(Num::Int(b)));
        prop_assert_eq!(enc_a.cmp(&enc_b), a.cmp(&b));
    }

    #[test]
    fn list_of_scalars_roundtrips(items in proptest::collection::vec(scalar_strategy(), 0..5)) {
        let v = DataValue::List(items);
        let mut enc = vec![];
        enc.encode_datavalue(&v);
        let (decoded, rest) = DataValue::decode_from_key(&enc);
        prop_assert!(rest.is_empty());
        prop_assert_eq!(decoded, v);
    }

    #[test]
    fn concatenated_values_decode_correctly(a in scalar_strategy(), b in scalar_strategy()) {
        let mut enc = vec![];
        enc.encode_datavalue(&a);
        enc.encode_datavalue(&b);
        let (decoded_a, rest) = DataValue::decode_from_key(&enc);
        let (decoded_b, remaining) = DataValue::decode_from_key(rest);
        prop_assert!(remaining.is_empty());
        prop_assert_eq!(decoded_a, a);
        prop_assert_eq!(decoded_b, b);
    }
}
