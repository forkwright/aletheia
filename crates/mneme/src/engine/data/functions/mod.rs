//! Built-in scalar functions.

use super::error::*;
type Result<T> = DataResult<T>;

use crate::engine::data::expr::Op;
use crate::engine::data::value::DataValue;

mod aggregate;
mod bits;
mod math;
mod string;
mod temporal;
mod trig;
mod utility;
mod vector;

/// Bounds-checked argument access for engine built-in functions.
///
/// The engine validates arity before calling functions (see `min_arity` in
/// `Op`), so out-of-bounds access indicates an internal engine bug rather than
/// user error. Returns a typed error instead of panicking.
pub(crate) fn arg(args: &[DataValue], idx: usize) -> Result<&DataValue> {
    args.get(idx).ok_or_else(|| {
        TypeMismatchSnafu {
            op: "builtin",
            expected: format!("at least {} argument(s)", idx + 1),
        }
        .build()
    })
}

pub(crate) use aggregate::*;
pub(crate) use bits::*;
pub(crate) use math::*;
pub(crate) use string::*;
pub(crate) use temporal::*;
pub(crate) use trig::*;
pub(crate) use utility::*;
pub(crate) use vector::*;

macro_rules! define_op {
    ($name:ident, $lower:ident, $min_arity:expr, $vararg:expr) => {
        pub(crate) const $name: Op = Op {
            name: stringify!($name),
            min_arity: $min_arity,
            vararg: $vararg,
            inner: $lower,
        };
    };
}

// Re-export items used externally.
pub(crate) use temporal::{MAX_VALIDITY_TS, TERMINAL_VALIDITY, current_validity, str2vld};

// ── aggregate / list / set / range ───────────────────────────────────────────
define_op!(OP_LIST, op_list, 0, true);
define_op!(OP_CONCAT, op_concat, 1, true);
define_op!(OP_APPEND, op_append, 2, false);
define_op!(OP_PREPEND, op_prepend, 2, false);
define_op!(OP_UNION, op_union, 1, true);
define_op!(OP_DIFFERENCE, op_difference, 2, true);
define_op!(OP_INTERSECTION, op_intersection, 1, true);
define_op!(OP_LENGTH, op_length, 1, false);
define_op!(OP_FIRST, op_first, 1, false);
define_op!(OP_LAST, op_last, 1, false);
define_op!(OP_SORTED, op_sorted, 1, false);
define_op!(OP_REVERSE, op_reverse, 1, false);
define_op!(OP_CHUNKS, op_chunks, 2, false);
define_op!(OP_CHUNKS_EXACT, op_chunks_exact, 2, false);
define_op!(OP_WINDOWS, op_windows, 2, false);
define_op!(OP_GET, op_get, 2, true);
define_op!(OP_MAYBE_GET, op_maybe_get, 2, false);
define_op!(OP_SLICE, op_slice, 3, false);
define_op!(OP_INT_RANGE, op_int_range, 1, true);
define_op!(OP_ASSERT, op_assert, 1, true);

// ── bits ─────────────────────────────────────────────────────────────────────
define_op!(OP_AND, op_and, 0, true);
define_op!(OP_OR, op_or, 0, true);
define_op!(OP_NEGATE, op_negate, 1, false);
define_op!(OP_BIT_AND, op_bit_and, 2, false);
define_op!(OP_BIT_OR, op_bit_or, 2, false);
define_op!(OP_BIT_XOR, op_bit_xor, 2, false);
define_op!(OP_BIT_NOT, op_bit_not, 1, false);
define_op!(OP_UNPACK_BITS, op_unpack_bits, 1, false);
define_op!(OP_PACK_BITS, op_pack_bits, 1, false);

// ── math ─────────────────────────────────────────────────────────────────────
define_op!(OP_EQ, op_eq, 2, false);
define_op!(OP_NEQ, op_neq, 2, false);
define_op!(OP_GT, op_gt, 2, false);
define_op!(OP_GE, op_ge, 2, false);
define_op!(OP_LT, op_lt, 2, false);
define_op!(OP_LE, op_le, 2, false);
define_op!(OP_ADD, op_add, 0, true);
define_op!(OP_MAX, op_max, 1, true);
define_op!(OP_MIN, op_min, 1, true);
define_op!(OP_SUB, op_sub, 2, false);
define_op!(OP_MUL, op_mul, 0, true);
define_op!(OP_DIV, op_div, 2, false);
define_op!(OP_MINUS, op_minus, 1, false);
define_op!(OP_POW, op_pow, 2, false);
define_op!(OP_MOD, op_mod, 2, false);
define_op!(OP_ABS, op_abs, 1, false);
define_op!(OP_SIGNUM, op_signum, 1, false);
define_op!(OP_FLOOR, op_floor, 1, false);
define_op!(OP_CEIL, op_ceil, 1, false);
define_op!(OP_ROUND, op_round, 1, false);
define_op!(OP_SQRT, op_sqrt, 1, false);
define_op!(OP_EXP, op_exp, 1, false);
define_op!(OP_EXP2, op_exp2, 1, false);
define_op!(OP_LN, op_ln, 1, false);
define_op!(OP_LOG2, op_log2, 1, false);
define_op!(OP_LOG10, op_log10, 1, false);
define_op!(OP_IS_IN, op_is_in, 2, false);
define_op!(OP_COALESCE, op_coalesce, 0, true);

// ── string ───────────────────────────────────────────────────────────────────
define_op!(OP_STR_INCLUDES, op_str_includes, 2, false);
define_op!(OP_LOWERCASE, op_lowercase, 1, false);
define_op!(OP_UPPERCASE, op_uppercase, 1, false);
define_op!(OP_TRIM, op_trim, 1, false);
define_op!(OP_TRIM_START, op_trim_start, 1, false);
define_op!(OP_TRIM_END, op_trim_end, 1, false);
define_op!(OP_STARTS_WITH, op_starts_with, 2, false);
define_op!(OP_ENDS_WITH, op_ends_with, 2, false);
define_op!(OP_REGEX, op_regex, 1, false);
define_op!(OP_REGEX_MATCHES, op_regex_matches, 2, false);
define_op!(OP_REGEX_REPLACE, op_regex_replace, 3, false);
define_op!(OP_REGEX_REPLACE_ALL, op_regex_replace_all, 3, false);
define_op!(OP_REGEX_EXTRACT, op_regex_extract, 2, false);
define_op!(OP_REGEX_EXTRACT_FIRST, op_regex_extract_first, 2, false);
define_op!(OP_UNICODE_NORMALIZE, op_unicode_normalize, 2, false);
define_op!(OP_ENCODE_BASE64, op_encode_base64, 1, false);
define_op!(OP_DECODE_BASE64, op_decode_base64, 1, false);
define_op!(OP_CHARS, op_chars, 1, false);
define_op!(OP_SLICE_STRING, op_slice_string, 3, false);
define_op!(OP_FROM_SUBSTRINGS, op_from_substrings, 1, false);

// ── temporal / UUID / random ─────────────────────────────────────────────────
define_op!(OP_NOW, op_now, 0, false);
define_op!(OP_FORMAT_TIMESTAMP, op_format_timestamp, 1, true);
define_op!(OP_PARSE_TIMESTAMP, op_parse_timestamp, 1, false);
define_op!(OP_RAND_UUID_V1, op_rand_uuid_v1, 0, false);
define_op!(OP_RAND_UUID_V4, op_rand_uuid_v4, 0, false);
define_op!(OP_UUID_TIMESTAMP, op_uuid_timestamp, 1, false);
define_op!(OP_RAND_FLOAT, op_rand_float, 0, false);
define_op!(OP_RAND_BERNOULLI, op_rand_bernoulli, 1, false);
define_op!(OP_RAND_INT, op_rand_int, 2, false);
define_op!(OP_RAND_CHOOSE, op_rand_choose, 1, false);
define_op!(OP_VALIDITY, op_validity, 1, true);

// ── trig ─────────────────────────────────────────────────────────────────────
define_op!(OP_SIN, op_sin, 1, false);
define_op!(OP_COS, op_cos, 1, false);
define_op!(OP_TAN, op_tan, 1, false);
define_op!(OP_ASIN, op_asin, 1, false);
define_op!(OP_ACOS, op_acos, 1, false);
define_op!(OP_ATAN, op_atan, 1, false);
define_op!(OP_ATAN2, op_atan2, 2, false);
define_op!(OP_SINH, op_sinh, 1, false);
define_op!(OP_COSH, op_cosh, 1, false);
define_op!(OP_TANH, op_tanh, 1, false);
define_op!(OP_ASINH, op_asinh, 1, false);
define_op!(OP_ACOSH, op_acosh, 1, false);
define_op!(OP_ATANH, op_atanh, 1, false);
define_op!(OP_HAVERSINE, op_haversine, 4, false);
define_op!(OP_HAVERSINE_DEG_INPUT, op_haversine_deg_input, 4, false);
define_op!(OP_DEG_TO_RAD, op_deg_to_rad, 1, false);
define_op!(OP_RAD_TO_DEG, op_rad_to_deg, 1, false);

// ── utility / type-checking / conversion / JSON ──────────────────────────────
define_op!(OP_IS_NULL, op_is_null, 1, false);
define_op!(OP_IS_INT, op_is_int, 1, false);
define_op!(OP_IS_FLOAT, op_is_float, 1, false);
define_op!(OP_IS_NUM, op_is_num, 1, false);
define_op!(OP_IS_FINITE, op_is_finite, 1, false);
define_op!(OP_IS_INFINITE, op_is_infinite, 1, false);
define_op!(OP_IS_NAN, op_is_nan, 1, false);
define_op!(OP_IS_STRING, op_is_string, 1, false);
define_op!(OP_IS_LIST, op_is_list, 1, false);
define_op!(OP_IS_VEC, op_is_vec, 1, false);
define_op!(OP_IS_BYTES, op_is_bytes, 1, false);
define_op!(OP_IS_UUID, op_is_uuid, 1, false);
define_op!(OP_IS_JSON, op_is_json, 1, false);
define_op!(OP_TO_BOOL, op_to_bool, 1, false);
define_op!(OP_TO_UNITY, op_to_unity, 1, false);
define_op!(OP_TO_INT, op_to_int, 1, false);
define_op!(OP_TO_FLOAT, op_to_float, 1, false);
define_op!(OP_TO_STRING, op_to_string, 1, false);
define_op!(OP_TO_UUID, op_to_uuid, 1, false);
define_op!(OP_JSON_TO_SCALAR, op_json_to_scalar, 1, false);
define_op!(OP_JSON, op_json, 1, false);
define_op!(OP_JSON_OBJECT, op_json_object, 0, true);
define_op!(OP_PARSE_JSON, op_parse_json, 1, false);
define_op!(OP_DUMP_JSON, op_dump_json, 1, false);
define_op!(OP_SET_JSON_PATH, op_set_json_path, 3, false);
define_op!(OP_REMOVE_JSON_PATH, op_remove_json_path, 2, false);

// ── vector ───────────────────────────────────────────────────────────────────
define_op!(OP_VEC, op_vec, 1, true);
define_op!(OP_RAND_VEC, op_rand_vec, 1, true);
define_op!(OP_L2_NORMALIZE, op_l2_normalize, 1, false);
define_op!(OP_L2_DIST, op_l2_dist, 2, false);
define_op!(OP_IP_DIST, op_ip_dist, 2, false);
define_op!(OP_COS_DIST, op_cos_dist, 2, false);

// ── misc (t2s stub) ───────────────────────────────────────────────────────────
define_op!(OP_T2S, op_t2s, 1, false);
fn op_t2s(args: &[DataValue]) -> Result<DataValue> {
    // fast2s crate removed; pass through unchanged
    Ok(arg(args, 0)?.clone())
}
