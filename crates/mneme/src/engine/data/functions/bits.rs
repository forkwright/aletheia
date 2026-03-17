//! Bitwise and boolean operations.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
use std::ops::Div;

use itertools::Itertools;

use super::arg;
use crate::engine::data::error::*;
type Result<T> = DataResult<T>;
use crate::engine::data::value::DataValue;

pub(crate) fn op_and(args: &[DataValue]) -> Result<DataValue> {
    for arg in args {
        if !arg.get_bool().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "and",
                expected: "booleans",
            }
            .build()
        })? {
            return Ok(DataValue::from(false));
        }
    }
    Ok(DataValue::from(true))
}

pub(crate) fn op_or(args: &[DataValue]) -> Result<DataValue> {
    for arg in args {
        if arg.get_bool().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "or",
                expected: "booleans",
            }
            .build()
        })? {
            return Ok(DataValue::from(true));
        }
    }
    Ok(DataValue::from(false))
}

pub(crate) fn op_negate(args: &[DataValue]) -> Result<DataValue> {
    if let DataValue::Bool(b) = arg(args, 0)? {
        Ok(DataValue::from(!*b))
    } else {
        TypeMismatchSnafu {
            op: "negate",
            expected: "booleans",
        }
        .fail()
    }
}

pub(crate) fn op_bit_and(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Bytes(left), DataValue::Bytes(right)) => {
            snafu::ensure!(
                left.len() == right.len(),
                ByteLengthMismatchSnafu { op: "bit_and" }
            );
            let mut ret = left.clone();
            for (l, r) in ret.iter_mut().zip(right.iter()) {
                *l &= *r;
            }
            Ok(DataValue::Bytes(ret))
        }
        _ => TypeMismatchSnafu {
            op: "bit_and",
            expected: "bytes",
        }
        .fail(),
    }
}

pub(crate) fn op_bit_or(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Bytes(left), DataValue::Bytes(right)) => {
            snafu::ensure!(
                left.len() == right.len(),
                ByteLengthMismatchSnafu { op: "bit_or" }
            );
            let mut ret = left.clone();
            for (l, r) in ret.iter_mut().zip(right.iter()) {
                *l |= *r;
            }
            Ok(DataValue::Bytes(ret))
        }
        _ => TypeMismatchSnafu {
            op: "bit_or",
            expected: "bytes",
        }
        .fail(),
    }
}

pub(crate) fn op_bit_not(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::Bytes(arg) => {
            let mut ret = arg.clone();
            for l in ret.iter_mut() {
                *l = !*l;
            }
            Ok(DataValue::Bytes(ret))
        }
        _ => TypeMismatchSnafu {
            op: "bit_not",
            expected: "bytes",
        }
        .fail(),
    }
}

pub(crate) fn op_bit_xor(args: &[DataValue]) -> Result<DataValue> {
    match (arg(args, 0)?, arg(args, 1)?) {
        (DataValue::Bytes(left), DataValue::Bytes(right)) => {
            snafu::ensure!(
                left.len() == right.len(),
                ByteLengthMismatchSnafu { op: "bit_xor" }
            );
            let mut ret = left.clone();
            for (l, r) in ret.iter_mut().zip(right.iter()) {
                *l ^= *r;
            }
            Ok(DataValue::Bytes(ret))
        }
        _ => TypeMismatchSnafu {
            op: "bit_xor",
            expected: "bytes",
        }
        .fail(),
    }
}

pub(crate) fn op_unpack_bits(args: &[DataValue]) -> Result<DataValue> {
    if let DataValue::Bytes(bs) = arg(args, 0)? {
        let mut ret = vec![false; bs.len() * 8];
        for (chunk, byte) in bs.iter().enumerate() {
            ret[chunk * 8] = (*byte & 0b10000000) != 0;
            ret[chunk * 8 + 1] = (*byte & 0b01000000) != 0;
            ret[chunk * 8 + 2] = (*byte & 0b00100000) != 0;
            ret[chunk * 8 + 3] = (*byte & 0b00010000) != 0;
            ret[chunk * 8 + 4] = (*byte & 0b00001000) != 0;
            ret[chunk * 8 + 5] = (*byte & 0b00000100) != 0;
            ret[chunk * 8 + 6] = (*byte & 0b00000010) != 0;
            ret[chunk * 8 + 7] = (*byte & 0b00000001) != 0;
        }
        Ok(DataValue::List(
            ret.into_iter().map(DataValue::Bool).collect_vec(),
        ))
    } else {
        TypeMismatchSnafu {
            op: "unpack_bits",
            expected: "bytes",
        }
        .fail()
    }
}

pub(crate) fn op_pack_bits(args: &[DataValue]) -> Result<DataValue> {
    if let DataValue::List(v) = arg(args, 0)? {
        let l = (v.len() as f64 / 8.).ceil() as usize;
        let mut res = vec![0u8; l];
        for (i, b) in v.iter().enumerate() {
            match b {
                DataValue::Bool(b) => {
                    if *b {
                        let chunk = i.div(&8);
                        let idx = i % 8;
                        let target = res
                            .get_mut(chunk)
                            .expect("chunk index bounded by ceil(v.len()/8) == res.len()");
                        match idx {
                            0 => *target |= 0b10000000,
                            1 => *target |= 0b01000000,
                            2 => *target |= 0b00100000,
                            3 => *target |= 0b00010000,
                            4 => *target |= 0b00001000,
                            5 => *target |= 0b00000100,
                            6 => *target |= 0b00000010,
                            7 => *target |= 0b00000001,
                            _ => unreachable!(),
                        }
                    }
                }
                _ => {
                    return TypeMismatchSnafu {
                        op: "pack_bits",
                        expected: "list of booleans",
                    }
                    .fail();
                }
            }
        }
        Ok(DataValue::Bytes(res))
    } else if let DataValue::Set(v) = arg(args, 0)? {
        let l = v.iter().cloned().collect_vec();
        op_pack_bits(&[DataValue::List(l)])
    } else {
        TypeMismatchSnafu {
            op: "pack_bits",
            expected: "list of booleans",
        }
        .fail()
    }
}
