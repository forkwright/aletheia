//! UUID, timestamp, validity, and random number functions.
#![expect(
    clippy::expect_used,
    reason = "engine invariant — internal CozoDB algorithm correctness guarantee"
)]
#![expect(
    clippy::as_conversions,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::cmp::Reverse;
use std::time::{SystemTime, UNIX_EPOCH};

use compact_str::CompactString;
use itertools::Itertools;
#[cfg(target_arch = "wasm32")]
use js_sys::Date;
use rand::prelude::*;
use uuid::v1::Timestamp;

use super::arg;
use crate::engine::data::error::*;
type Result<T> = DataResult<T>;
use crate::engine::data::value::{DataValue, UuidWrapper, Validity, ValidityTs};

pub(crate) fn current_validity() -> ValidityTs {
    #[cfg(not(target_arch = "wasm32"))]
    let ts_micros = {
        let now = SystemTime::now();
        now.duration_since(UNIX_EPOCH)
            .expect("SystemTime::now() is always after UNIX_EPOCH")
            .as_micros() as i64
    };
    #[cfg(target_arch = "wasm32")]
    let ts_micros = { (Date::now() * 1000.) as i64 };

    ValidityTs(Reverse(ts_micros))
}

pub(crate) const MAX_VALIDITY_TS: ValidityTs = ValidityTs(Reverse(i64::MAX));
pub(crate) const TERMINAL_VALIDITY: Validity = Validity {
    timestamp: ValidityTs(Reverse(i64::MIN)),
    is_assert: Reverse(false),
};

pub(crate) fn str2vld(s: &str) -> Result<ValidityTs> {
    let ts: jiff::Timestamp = s.parse().map_err(|e| {
        BadTimeSnafu {
            message: format!("bad datetime: {s}: {e}"),
        }
        .build()
    })?;
    Ok(ValidityTs(Reverse(ts.as_microsecond())))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn op_now(_args: &[DataValue]) -> Result<DataValue> {
    let d: f64 = Date::now() / 1000.;
    Ok(DataValue::from(d))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn op_now(_args: &[DataValue]) -> Result<DataValue> {
    let now = SystemTime::now();
    Ok(DataValue::from(
        now.duration_since(UNIX_EPOCH)
            .expect("SystemTime::now() is always after UNIX_EPOCH")
            .as_secs_f64(),
    ))
}

pub(crate) fn op_format_timestamp(args: &[DataValue]) -> Result<DataValue> {
    let millis = match arg(args, 0)? {
        DataValue::Validity(vld) => vld.timestamp.0.0 / 1000,
        v => {
            let f = v.get_float().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "format_timestamp",
                    expected: "a number",
                }
                .build()
            })?;
            (f * 1000.) as i64
        }
    };
    let raw_arg = arg(args, 0)?;
    let ts = jiff::Timestamp::from_millisecond(millis).map_err(|e| {
        BadTimeSnafu {
            message: format!("bad time: {raw_arg}: {e}"),
        }
        .build()
    })?;
    match args.get(1) {
        Some(tz_v) => {
            let tz_s = tz_v.get_str().ok_or_else(|| {
                TypeMismatchSnafu {
                    op: "format_timestamp",
                    expected: "a string for timezone specification",
                }
                .build()
            })?;
            let tz = jiff::tz::TimeZone::get(tz_s).map_err(|e| {
                BadTimeSnafu {
                    message: format!("bad timezone specification: {tz_s}: {e}"),
                }
                .build()
            })?;
            let zoned = ts.to_zoned(tz);
            let s = CompactString::from(zoned.to_string());
            Ok(DataValue::Str(s))
        }
        None => {
            let s = CompactString::from(ts.to_string());
            Ok(DataValue::Str(s))
        }
    }
}

pub(crate) fn op_parse_timestamp(args: &[DataValue]) -> Result<DataValue> {
    let s = arg(args, 0)?.get_str().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "parse_timestamp",
            expected: "a string",
        }
        .build()
    })?;
    let ts: jiff::Timestamp = s.parse().map_err(|e| {
        BadTimeSnafu {
            message: format!("bad datetime: {s}: {e}"),
        }
        .build()
    })?;
    Ok(DataValue::from(
        ts.as_second() as f64 + ts.subsec_nanosecond() as f64 / 1e9,
    ))
}

pub(crate) fn op_rand_uuid_v1(_args: &[DataValue]) -> Result<DataValue> {
    let mut rng = rand::rng();
    let uuid_ctx = uuid::v1::Context::new(rng.random());
    #[cfg(target_arch = "wasm32")]
    let ts = {
        let since_epoch: f64 = Date::now();
        let seconds = since_epoch.floor();
        let fractional = (since_epoch - seconds) * 1.0e9;
        Timestamp::from_unix(uuid_ctx, seconds as u64, fractional as u32)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let ts = {
        let now = SystemTime::now();
        let since_epoch = now
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime::now() is always after UNIX_EPOCH");
        Timestamp::from_unix(uuid_ctx, since_epoch.as_secs(), since_epoch.subsec_nanos())
    };
    let mut rand_vals = [0u8; 6];
    rng.fill(&mut rand_vals);
    let id = uuid::Uuid::new_v1(ts, &rand_vals);
    Ok(DataValue::uuid(id))
}

pub(crate) fn op_rand_uuid_v4(_args: &[DataValue]) -> Result<DataValue> {
    let id = uuid::Uuid::new_v4();
    Ok(DataValue::uuid(id))
}

pub(crate) fn op_uuid_timestamp(args: &[DataValue]) -> Result<DataValue> {
    Ok(match arg(args, 0)? {
        DataValue::Uuid(UuidWrapper(id)) => match id.get_timestamp() {
            None => DataValue::Null,
            Some(t) => {
                let (s, subs) = t.to_unix();
                let s = (s as f64) + (subs as f64 / 10_000_000.);
                s.into()
            }
        },
        _ => {
            return TypeMismatchSnafu {
                op: "uuid_timestamp",
                expected: "a UUID",
            }
            .fail();
        }
    })
}

pub(crate) fn op_rand_float(_args: &[DataValue]) -> Result<DataValue> {
    Ok(rand::rng().random::<f64>().into())
}

pub(crate) fn op_rand_bernoulli(args: &[DataValue]) -> Result<DataValue> {
    let prob = match arg(args, 0)? {
        DataValue::Num(n) => {
            let f = n.get_float();
            snafu::ensure!(
                (0. ..=1.).contains(&f),
                InvalidValueSnafu {
                    message: "'rand_bernoulli' requires number between 0. and 1."
                }
            );
            f
        }
        _ => {
            return TypeMismatchSnafu {
                op: "rand_bernoulli",
                expected: "number between 0. and 1.",
            }
            .fail();
        }
    };
    Ok(DataValue::from(rand::rng().random_bool(prob)))
}

pub(crate) fn op_rand_int(args: &[DataValue]) -> Result<DataValue> {
    let lower = &arg(args, 0)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "rand_int",
            expected: "integers",
        }
        .build()
    })?;
    let upper = &arg(args, 1)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "rand_int",
            expected: "integers",
        }
        .build()
    })?;
    Ok(rand::rng().random_range(*lower..=*upper).into())
}

pub(crate) fn op_rand_choose(args: &[DataValue]) -> Result<DataValue> {
    match arg(args, 0)? {
        DataValue::List(l) => Ok(l
            .choose(&mut rand::rng())
            .cloned()
            .unwrap_or(DataValue::Null)),
        DataValue::Set(l) => Ok(l
            .iter()
            .collect_vec()
            .choose(&mut rand::rng())
            .cloned()
            .cloned()
            .unwrap_or(DataValue::Null)),
        _ => TypeMismatchSnafu {
            op: "rand_choice",
            expected: "lists",
        }
        .fail(),
    }
}

pub(crate) fn op_validity(args: &[DataValue]) -> Result<DataValue> {
    let ts = arg(args, 0)?.get_int().ok_or_else(|| {
        TypeMismatchSnafu {
            op: "validity",
            expected: "an integer",
        }
        .build()
    })?;
    let is_assert = if args.len() == 1 {
        true
    } else {
        arg(args, 1)?.get_bool().ok_or_else(|| {
            TypeMismatchSnafu {
                op: "validity",
                expected: "a boolean as second argument",
            }
            .build()
        })?
    };
    Ok(DataValue::Validity(Validity {
        timestamp: ValidityTs(Reverse(ts)),
        is_assert: Reverse(is_assert),
    }))
}
