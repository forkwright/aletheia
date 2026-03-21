//! Memory-comparable encoding for composite keys.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::io::Write;
use std::str::FromStr;

use regex::Regex;

use crate::data::value::{
    DataValue, JsonData, Num, RegexWrapper, UuidWrapper, Validity, ValidityTs, Vector,
};

const INIT_TAG: u8 = 0x00;
const NULL_TAG: u8 = 0x01;
const FALSE_TAG: u8 = 0x02;
const TRUE_TAG: u8 = 0x03;
const VEC_TAG: u8 = 0x04;
const NUM_TAG: u8 = 0x05;
const STR_TAG: u8 = 0x06;
const BYTES_TAG: u8 = 0x07;
const UUID_TAG: u8 = 0x08;
const REGEX_TAG: u8 = 0x09;
const LIST_TAG: u8 = 0x0A;
const SET_TAG: u8 = 0x0B;
const VLD_TAG: u8 = 0x0C;
const JSON_TAG: u8 = 0x0D;
const BOT_TAG: u8 = 0xFF;

const VEC_F32: u8 = 0x01;
const VEC_F64: u8 = 0x02;

const IS_FLOAT: u8 = 0b00010000;
const IS_APPROX_INT: u8 = 0b00000100;
const IS_EXACT_INT: u8 = 0b00000000;
const EXACT_INT_BOUND: i64 = 0x20_0000_0000_0000;

pub(crate) trait MemCmpEncoder: Write {
    fn encode_datavalue(&mut self, v: &DataValue) {
        match v {
            DataValue::Null => self
                .write_all(&[NULL_TAG])
                .expect("Vec<u8> write is infallible"),
            DataValue::Bool(false) => self
                .write_all(&[FALSE_TAG])
                .expect("Vec<u8> write is infallible"),
            DataValue::Bool(true) => self
                .write_all(&[TRUE_TAG])
                .expect("Vec<u8> write is infallible"),
            DataValue::Vec(arr) => {
                self.write_all(&[VEC_TAG])
                    .expect("Vec<u8> write is infallible");
                match arr {
                    Vector::F32(a) => {
                        self.write_all(&[VEC_F32])
                            .expect("Vec<u8> write is infallible");
                        let l = a.len();
                        self.write_all(&(l as u64).to_be_bytes())
                            .expect("Vec<u8> write is infallible");
                        for el in a {
                            self.write_all(&el.to_be_bytes())
                                .expect("Vec<u8> write is infallible");
                        }
                    }
                    Vector::F64(a) => {
                        self.write_all(&[VEC_F64])
                            .expect("Vec<u8> write is infallible");
                        let l = a.len();
                        self.write_all(&(l as u64).to_be_bytes())
                            .expect("Vec<u8> write is infallible");
                        for el in a {
                            self.write_all(&el.to_be_bytes())
                                .expect("Vec<u8> write is infallible");
                        }
                    }
                }
            }
            DataValue::Num(n) => {
                self.write_all(&[NUM_TAG])
                    .expect("Vec<u8> write is infallible");
                self.encode_num(*n);
            }
            DataValue::Str(s) => {
                self.write_all(&[STR_TAG])
                    .expect("Vec<u8> write is infallible");
                self.encode_bytes(s.as_bytes());
            }
            DataValue::Json(j) => {
                self.write_all(&[JSON_TAG])
                    .expect("Vec<u8> write is infallible");
                let s = j.0.to_string();
                self.encode_bytes(s.as_bytes());
            }
            DataValue::Bytes(b) => {
                self.write_all(&[BYTES_TAG])
                    .expect("Vec<u8> write is infallible");
                self.encode_bytes(b)
            }
            DataValue::Uuid(u) => {
                self.write_all(&[UUID_TAG])
                    .expect("Vec<u8> write is infallible");
                let (s_l, s_m, s_h, s_rest) = u.0.as_fields();
                self.write_all(&s_h.to_be_bytes())
                    .expect("Vec<u8> write is infallible");
                self.write_all(&s_m.to_be_bytes())
                    .expect("Vec<u8> write is infallible");
                self.write_all(&s_l.to_be_bytes())
                    .expect("Vec<u8> write is infallible");
                self.write_all(s_rest.as_ref())
                    .expect("Vec<u8> write is infallible");
            }
            DataValue::Regex(rx) => {
                self.write_all(&[REGEX_TAG])
                    .expect("Vec<u8> write is infallible");
                let s = rx.0.as_str().as_bytes();
                self.encode_bytes(s)
            }
            DataValue::List(l) => {
                self.write_all(&[LIST_TAG])
                    .expect("Vec<u8> write is infallible");
                for el in l {
                    self.encode_datavalue(el);
                }
                self.write_all(&[INIT_TAG])
                    .expect("Vec<u8> write is infallible")
            }
            DataValue::Set(s) => {
                self.write_all(&[SET_TAG])
                    .expect("Vec<u8> write is infallible");
                for el in s {
                    self.encode_datavalue(el);
                }
                self.write_all(&[INIT_TAG])
                    .expect("Vec<u8> write is infallible")
            }
            DataValue::Validity(vld) => {
                let ts = vld.timestamp.0.0;
                let ts_u64 = order_encode_i64(ts);
                let ts_flipped = !ts_u64;
                self.write_all(&[VLD_TAG])
                    .expect("Vec<u8> write is infallible");
                self.write_all(&ts_flipped.to_be_bytes())
                    .expect("Vec<u8> write is infallible");
                self.write_all(&[!vld.is_assert.0 as u8])
                    .expect("Vec<u8> write is infallible");
            }
            DataValue::Bot => self
                .write_all(&[BOT_TAG])
                .expect("Vec<u8> write is infallible"),
        }
    }
    fn encode_num(&mut self, v: Num) {
        let f = v.get_float();
        let u = order_encode_f64(f);
        self.write_all(&u.to_be_bytes())
            .expect("Vec<u8> write is infallible");
        match v {
            Num::Int(i) => {
                if i > -EXACT_INT_BOUND && i < EXACT_INT_BOUND {
                    self.write_all(&[IS_EXACT_INT])
                        .expect("Vec<u8> write is infallible");
                } else {
                    self.write_all(&[IS_APPROX_INT])
                        .expect("Vec<u8> write is infallible");
                    let en = order_encode_i64(i);
                    self.write_all(&en.to_be_bytes())
                        .expect("Vec<u8> write is infallible");
                }
            }
            Num::Float(_) => {
                self.write_all(&[IS_FLOAT])
                    .expect("Vec<u8> write is infallible");
            }
        }
    }

    fn encode_bytes(&mut self, key: &[u8]) {
        let len = key.len();
        let mut index = 0;
        while index <= len {
            let remain = len - index;
            let mut pad: usize = 0;
            if remain > ENC_GROUP_SIZE {
                self.write_all(&key[index..index + ENC_GROUP_SIZE])
                    .expect("Vec<u8> write is infallible");
            } else {
                pad = ENC_GROUP_SIZE - remain;
                self.write_all(&key[index..])
                    .expect("Vec<u8> write is infallible");
                self.write_all(&ENC_ASC_PADDING[..pad])
                    .expect("Vec<u8> write is infallible");
            }
            self.write_all(&[ENC_MARKER - (pad as u8)])
                .expect("Vec<u8> write is infallible");
            index += ENC_GROUP_SIZE;
        }
    }
}

pub fn decode_bytes(data: &[u8]) -> (Vec<u8>, &[u8]) {
    let mut key = Vec::with_capacity(data.len() / (ENC_GROUP_SIZE + 1) * ENC_GROUP_SIZE);
    let mut offset = 0;
    let chunk_len = ENC_GROUP_SIZE + 1;
    loop {
        let next_offset = offset + chunk_len;
        debug_assert!(next_offset <= data.len());
        let chunk = &data[offset..next_offset];
        offset = next_offset;

        let (&marker, bytes) = chunk
            .split_last()
            .expect("chunk is ENC_GROUP_SIZE+1 bytes, always non-empty");
        let pad_size = (ENC_MARKER - marker) as usize;

        if pad_size == 0 {
            key.write_all(bytes).expect("Vec<u8> write is infallible");
            continue;
        }
        debug_assert!(pad_size <= ENC_GROUP_SIZE);

        let (bytes, padding) = bytes.split_at(ENC_GROUP_SIZE - pad_size);
        key.write_all(bytes).expect("Vec<u8> write is infallible");

        debug_assert!(!padding.iter().any(|x| *x != 0));

        return (key, &data[offset..]);
    }
}

const SIGN_MARK: u64 = 0x8000000000000000;

fn order_encode_i64(v: i64) -> u64 {
    v as u64 ^ SIGN_MARK
}

fn order_decode_i64(u: u64) -> i64 {
    (u ^ SIGN_MARK) as i64
}

fn order_encode_f64(v: f64) -> u64 {
    let u = v.to_bits();
    if v.is_sign_positive() {
        u | SIGN_MARK
    } else {
        !u
    }
}

fn order_decode_f64(u: u64) -> f64 {
    let u = if u & SIGN_MARK > 0 {
        u & (!SIGN_MARK)
    } else {
        !u
    };
    f64::from_bits(u)
}

const ENC_GROUP_SIZE: usize = 8;
const ENC_MARKER: u8 = b'\xff';
const ENC_ASC_PADDING: [u8; ENC_GROUP_SIZE] = [0; ENC_GROUP_SIZE];

impl Num {
    pub(crate) fn decode_from_key(bs: &[u8]) -> (Self, &[u8]) {
        let (float_part, remaining) = bs.split_at(8);
        let fu = u64::from_be_bytes(
            float_part
                .try_into()
                .expect("split_at(8) yields exactly 8 bytes"),
        );
        let f = order_decode_f64(fu);
        let (tag, remaining) = remaining
            .split_first()
            .expect("encoded key always has a tag byte after the float part");
        match *tag {
            IS_FLOAT => (Num::Float(f), remaining),
            IS_EXACT_INT => (Num::Int(f as i64), remaining),
            IS_APPROX_INT => {
                let (int_part, remaining) = remaining.split_at(8);
                let iu = u64::from_be_bytes(
                    int_part
                        .try_into()
                        .expect("split_at(8) yields exactly 8 bytes"),
                );
                let i = order_decode_i64(iu);
                (Num::Int(i), remaining)
            }
            _ => unreachable!(),
        }
    }
}

impl DataValue {
    pub(crate) fn decode_from_key(bs: &[u8]) -> (Self, &[u8]) {
        let (tag, remaining) = bs
            .split_first()
            .expect("encoded key always starts with a tag byte");
        match *tag {
            NULL_TAG => (DataValue::Null, remaining),
            FALSE_TAG => (DataValue::from(false), remaining),
            TRUE_TAG => (DataValue::from(true), remaining),
            NUM_TAG => {
                let (n, remaining) = Num::decode_from_key(remaining);
                (DataValue::Num(n), remaining)
            }
            STR_TAG => {
                let (bytes, remaining) = decode_bytes(remaining);
                // SAFETY: These bytes were produced by `encode_datavalue` for a
                // `DataValue::Str`, which called `s.as_bytes()` on a valid Rust `&str`.
                // UTF-8 validity is therefore guaranteed by the encoding invariant.
                let s = unsafe { String::from_utf8_unchecked(bytes) };
                (DataValue::Str(s.into()), remaining)
            }
            JSON_TAG => {
                let (bytes, remaining) = decode_bytes(remaining);
                (
                    DataValue::Json(JsonData(
                        serde_json::from_slice(&bytes)
                            .expect("bytes were encoded as JSON by encode_datavalue"),
                    )),
                    remaining,
                )
            }
            BYTES_TAG => {
                let (bytes, remaining) = decode_bytes(remaining);
                (DataValue::Bytes(bytes), remaining)
            }
            UUID_TAG => {
                let (uuid_data, remaining) = remaining.split_at(16);
                let s_h = u16::from_be_bytes(
                    uuid_data[0..2]
                        .try_into()
                        .expect("slice [0..2] is exactly 2 bytes"),
                );
                let s_m = u16::from_be_bytes(
                    uuid_data[2..4]
                        .try_into()
                        .expect("slice [2..4] is exactly 2 bytes"),
                );
                let s_l = u32::from_be_bytes(
                    uuid_data[4..8]
                        .try_into()
                        .expect("slice [4..8] is exactly 4 bytes"),
                );
                let mut s_rest = [0u8; 8];
                s_rest.copy_from_slice(&uuid_data[8..]);
                let uuid = uuid::Uuid::from_fields(s_l, s_m, s_h, &s_rest);
                (DataValue::Uuid(UuidWrapper(uuid)), remaining)
            }
            REGEX_TAG => {
                let (bytes, remaining) = decode_bytes(remaining);
                // SAFETY: These bytes were produced by `encode_datavalue` for a
                // `DataValue::Regex`, which serialised the regex source string via
                // `s.as_bytes()`. The original source is a valid Rust `&str`, so
                // UTF-8 validity is guaranteed by the encoding invariant.
                let s = unsafe { String::from_utf8_unchecked(bytes) };
                (
                    DataValue::Regex(RegexWrapper(
                        Regex::from_str(&s)
                            .expect("regex source was serialized from a valid Regex"),
                    )),
                    remaining,
                )
            }
            LIST_TAG => {
                let mut collected = vec![];
                let mut remaining = remaining;
                while remaining[0] != INIT_TAG {
                    let (val, next_chunk) = DataValue::decode_from_key(remaining);
                    remaining = next_chunk;
                    collected.push(val);
                }
                (DataValue::List(collected), &remaining[1..])
            }
            SET_TAG => {
                let mut collected = BTreeSet::default();
                let mut remaining = remaining;
                while remaining[0] != INIT_TAG {
                    let (val, next_chunk) = DataValue::decode_from_key(remaining);
                    remaining = next_chunk;
                    collected.insert(val);
                }
                (DataValue::Set(collected), &remaining[1..])
            }
            VLD_TAG => {
                let (ts_flipped_bytes, rest) = remaining.split_at(8);
                let ts_flipped = u64::from_be_bytes(
                    ts_flipped_bytes
                        .try_into()
                        .expect("split_at(8) yields exactly 8 bytes"),
                );
                let ts_u64 = !ts_flipped;
                let ts = order_decode_i64(ts_u64);
                let (is_assert_byte, rest) = rest
                    .split_first()
                    .expect("encoded key always has an is_assert byte after timestamp");
                let is_assert = *is_assert_byte == 0;
                (
                    DataValue::Validity(Validity {
                        timestamp: ValidityTs(Reverse(ts)),
                        is_assert: Reverse(is_assert),
                    }),
                    rest,
                )
            }
            BOT_TAG => (DataValue::Bot, remaining),
            VEC_TAG => {
                let (t_tag, remaining) = remaining
                    .split_first()
                    .expect("encoded key always has a vector type tag after VEC_TAG");
                let (len_bytes, mut rest) = remaining.split_at(8);
                let len = u64::from_be_bytes(
                    len_bytes
                        .try_into()
                        .expect("split_at(8) yields exactly 8 bytes"),
                ) as usize;
                match *t_tag {
                    VEC_F32 => {
                        let mut res_arr = ndarray::Array1::zeros(len);
                        for mut row in res_arr.axis_iter_mut(ndarray::Axis(0)) {
                            let (f_bytes, next_chunk) = rest.split_at(4);
                            rest = next_chunk;
                            let f = f32::from_be_bytes(
                                f_bytes
                                    .try_into()
                                    .expect("split_at(4) yields exactly 4 bytes"),
                            );
                            row.fill(f);
                        }
                        (DataValue::Vec(Vector::F32(res_arr)), rest)
                    }
                    VEC_F64 => {
                        let mut res_arr = ndarray::Array1::zeros(len);
                        for mut row in res_arr.axis_iter_mut(ndarray::Axis(0)) {
                            let (f_bytes, next_chunk) = rest.split_at(8);
                            rest = next_chunk;
                            let f = f64::from_be_bytes(
                                f_bytes
                                    .try_into()
                                    .expect("split_at(8) yields exactly 8 bytes"),
                            );
                            row.fill(f);
                        }
                        (DataValue::Vec(Vector::F64(res_arr)), rest)
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!("{:?}", bs),
        }
    }
}

impl<T: Write> MemCmpEncoder for T {}
