//! UUID, timestamp, validity, and random number functions.
#![expect(
    clippy::as_conversions,
    reason = "temporal functions require i64/f64 casts for Unix timestamps"
)]
#![expect(
    clippy::unnecessary_wraps,
    reason = "temporal functions return Result for API consistency with other builtins"
)]
#![expect(
    clippy::single_match_else,
    reason = "timezone branch reads better as if-let for the happy path"
)]
#![expect(
    clippy::cloned_instead_of_copied,
    reason = "DataValue is not Copy — .cloned() is correct"
)]

use std::cmp::Reverse;
use std::time::{SystemTime, UNIX_EPOCH};

use compact_str::CompactString;
use itertools::Itertools;
#[cfg(target_arch = "wasm32")]
use js_sys::Date;
use rand::RngExt;
use rand::seq::IndexedRandom;

use super::arg;
use crate::data::error::*;
type Result<T> = DataResult<T>;
use crate::data::value::{DataValue, UuidWrapper, Validity, ValidityTs};

pub(crate) fn current_validity() -> ValidityTs {
    #[cfg(not(target_arch = "wasm32"))]
    let ts_micros = {
        let now = SystemTime::now();
        // INVARIANT: Unix microseconds fit in i64 until year ~294247 AD;
        // saturating handles the theoretical overflow gracefully.
        i64::try_from(
            now.duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros(),
        )
        .unwrap_or(i64::MAX)
    };
    #[cfg(target_arch = "wasm32")]
    #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
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
            .unwrap_or_default()
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
            // INVARIANT: out-of-range floats (NaN, infinities, > i64::MAX) saturate
            // to i64::MAX / i64::MIN per Rust's `as` semantics. The downstream
            // `Timestamp::from_millisecond` will then surface a `BadTime` error.
            // The alternative (returning an error here) would change behavior for
            // all callers of `format_timestamp`, so we preserve the cast and let
            // the timestamp constructor do the validation.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "preserves saturating behavior; downstream validates"
            )]
            let m = (f * 1000.) as i64;
            m
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
    // INVARIANT: Unix seconds can exceed 2^53 only past year ~285 million AD.
    // Sub-second nanoseconds are bounded to 0..=999_999_999 by jiff's type.
    #[expect(
        clippy::cast_precision_loss,
        reason = "i64 seconds fit f64 until year ~285 million AD"
    )]
    let seconds_f = ts.as_second() as f64;
    Ok(DataValue::from(
        seconds_f + f64::from(ts.subsec_nanosecond()) / 1e9,
    ))
}

pub(crate) fn op_rand_uuid_v1(_args: &[DataValue]) -> Result<DataValue> {
    // UUID epoch offset: 1582-10-15 → 1970-01-01 in 100-nanosecond intervals.
    const UUID_UNIX_OFFSET: u64 = 122_192_928_000_000_000;
    const INTERVALS_PER_SEC: u64 = 10_000_000;

    let mut rng = rand::rng();
    let clock_seq: u16 = rng.random();

    #[cfg(target_arch = "wasm32")]
    let ts_100ns = {
        let since_epoch_ms: f64 = Date::now();
        let since_epoch_secs = since_epoch_ms.floor() / 1000.;
        let secs = since_epoch_secs as u64;
        let nanos = ((since_epoch_ms / 1000. - since_epoch_secs) * 1.0e9) as u64;
        secs * INTERVALS_PER_SEC + nanos / 100 + UUID_UNIX_OFFSET
    };
    #[cfg(not(target_arch = "wasm32"))]
    let ts_100ns = {
        let now = SystemTime::now();
        let since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        let secs = since_epoch.as_secs();
        let nanos = u64::from(since_epoch.subsec_nanos());
        secs * INTERVALS_PER_SEC + nanos / 100 + UUID_UNIX_OFFSET
    };

    let mut rand_vals = [0u8; 6];
    rng.fill(&mut rand_vals);
    let id = koina::uuid::Uuid::new_v1(ts_100ns, clock_seq, &rand_vals);
    Ok(DataValue::uuid(id))
}

pub(crate) fn op_rand_uuid_v4(_args: &[DataValue]) -> Result<DataValue> {
    let id = koina::uuid::Uuid::new_v4();
    Ok(DataValue::uuid(id))
}

pub(crate) fn op_uuid_timestamp(args: &[DataValue]) -> Result<DataValue> {
    Ok(match arg(args, 0)? {
        DataValue::Uuid(UuidWrapper(id)) => match id.get_timestamp() {
            None => DataValue::Null,
            Some(t) => {
                let (s, subs) = t.to_unix();
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "u64 to f64: precision loss acceptable for Unix seconds"
                )]
                let s = (s as f64) + (f64::from(subs) / 10_000_000.);
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
