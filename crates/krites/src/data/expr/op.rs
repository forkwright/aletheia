//! Op and ValueRange types.
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::cmp::{max, min};
use std::fmt::{Debug, Formatter};

use serde::de::{Error, Visitor};
use serde::{Deserializer, Serializer};

use crate::data::functions::*;
use crate::data::relation::NullableColType;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;

use super::super::error::*;
use super::expr_impl::Expr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValueRange {
    pub(crate) lower: DataValue,
    pub(crate) upper: DataValue,
}

impl ValueRange {
    pub(crate) fn merge(self, other: Self) -> Self {
        let lower = max(self.lower, other.lower);
        let upper = min(self.upper, other.upper);
        if lower > upper {
            Self::null()
        } else {
            Self { lower, upper }
        }
    }
    fn null() -> Self {
        Self {
            lower: DataValue::Bot,
            upper: DataValue::Bot,
        }
    }
    pub(crate) fn new(lower: DataValue, upper: DataValue) -> Self {
        Self { lower, upper }
    }
    pub(crate) fn lower_bound(val: DataValue) -> Self {
        Self {
            lower: val,
            upper: DataValue::Bot,
        }
    }
    pub(crate) fn upper_bound(val: DataValue) -> Self {
        Self {
            lower: DataValue::Null,
            upper: val,
        }
    }
}

impl Default for ValueRange {
    fn default() -> Self {
        Self {
            lower: DataValue::Null,
            upper: DataValue::Bot,
        }
    }
}

#[derive(Clone)]
pub struct Op {
    pub(crate) name: &'static str,
    pub(crate) min_arity: usize,
    pub(crate) vararg: bool,
    pub(crate) inner: fn(&[DataValue]) -> DataResult<DataValue>,
}

/// Used as `Arc<dyn CustomOp>`
#[expect(
    dead_code,
    reason = "public extension point for custom operations, no built-in implementors yet"
)]
pub trait CustomOp {
    fn name(&self) -> &'static str;
    fn min_arity(&self) -> usize;
    fn vararg(&self) -> bool;
    fn return_type(&self) -> NullableColType;
    fn call(&self, args: &[DataValue]) -> Result<DataValue>;
}

impl serde::Serialize for &'_ Op {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.name)
    }
}

impl<'de> serde::Deserialize<'de> for &'static Op {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(OpVisitor)
    }
}

struct OpVisitor;

impl<'de> Visitor<'de> for OpVisitor {
    type Value = &'static Op;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("name of the op")
    }

    fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
    where
        E: Error,
    {
        let name = v
            .strip_prefix("OP_")
            .ok_or_else(|| E::custom(format!("op name must start with OP_, got: {v}")))?
            .to_ascii_lowercase();
        get_op(&name).ok_or_else(|| E::custom(format!("op not found in serialized data: {v}")))
    }
}

impl PartialEq for Op {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Op {}

impl Debug for Op {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

pub(crate) fn get_op(name: &str) -> Option<&'static Op> {
    Some(match name {
        "coalesce" => &OP_COALESCE,
        "list" => &OP_LIST,
        "json" => &OP_JSON,
        "set_json_path" => &OP_SET_JSON_PATH,
        "remove_json_path" => &OP_REMOVE_JSON_PATH,
        "parse_json" => &OP_PARSE_JSON,
        "dump_json" => &OP_DUMP_JSON,
        "json_object" => &OP_JSON_OBJECT,
        "is_json" => &OP_IS_JSON,
        "json_to_scalar" => &OP_JSON_TO_SCALAR,
        "add" => &OP_ADD,
        "sub" => &OP_SUB,
        "mul" => &OP_MUL,
        "div" => &OP_DIV,
        "minus" => &OP_MINUS,
        "abs" => &OP_ABS,
        "signum" => &OP_SIGNUM,
        "floor" => &OP_FLOOR,
        "ceil" => &OP_CEIL,
        "round" => &OP_ROUND,
        "mod" => &OP_MOD,
        "max" => &OP_MAX,
        "min" => &OP_MIN,
        "pow" => &OP_POW,
        "sqrt" => &OP_SQRT,
        "exp" => &OP_EXP,
        "exp2" => &OP_EXP2,
        "ln" => &OP_LN,
        "log2" => &OP_LOG2,
        "log10" => &OP_LOG10,
        "sin" => &OP_SIN,
        "cos" => &OP_COS,
        "tan" => &OP_TAN,
        "asin" => &OP_ASIN,
        "acos" => &OP_ACOS,
        "atan" => &OP_ATAN,
        "atan2" => &OP_ATAN2,
        "sinh" => &OP_SINH,
        "cosh" => &OP_COSH,
        "tanh" => &OP_TANH,
        "asinh" => &OP_ASINH,
        "acosh" => &OP_ACOSH,
        "atanh" => &OP_ATANH,
        "eq" => &OP_EQ,
        "neq" => &OP_NEQ,
        "gt" => &OP_GT,
        "ge" => &OP_GE,
        "lt" => &OP_LT,
        "le" => &OP_LE,
        "or" => &OP_OR,
        "and" => &OP_AND,
        "negate" => &OP_NEGATE,
        "bit_and" => &OP_BIT_AND,
        "bit_or" => &OP_BIT_OR,
        "bit_not" => &OP_BIT_NOT,
        "bit_xor" => &OP_BIT_XOR,
        "pack_bits" => &OP_PACK_BITS,
        "unpack_bits" => &OP_UNPACK_BITS,
        "concat" => &OP_CONCAT,
        "str_includes" => &OP_STR_INCLUDES,
        "lowercase" => &OP_LOWERCASE,
        "uppercase" => &OP_UPPERCASE,
        "trim" => &OP_TRIM,
        "trim_start" => &OP_TRIM_START,
        "trim_end" => &OP_TRIM_END,
        "starts_with" => &OP_STARTS_WITH,
        "ends_with" => &OP_ENDS_WITH,
        "is_null" => &OP_IS_NULL,
        "is_int" => &OP_IS_INT,
        "is_float" => &OP_IS_FLOAT,
        "is_num" => &OP_IS_NUM,
        "is_string" => &OP_IS_STRING,
        "is_list" => &OP_IS_LIST,
        "is_bytes" => &OP_IS_BYTES,
        "is_in" => &OP_IS_IN,
        "is_finite" => &OP_IS_FINITE,
        "is_infinite" => &OP_IS_INFINITE,
        "is_nan" => &OP_IS_NAN,
        "is_uuid" => &OP_IS_UUID,
        "is_vec" => &OP_IS_VEC,
        "length" => &OP_LENGTH,
        "sorted" => &OP_SORTED,
        "reverse" => &OP_REVERSE,
        "append" => &OP_APPEND,
        "prepend" => &OP_PREPEND,
        "unicode_normalize" => &OP_UNICODE_NORMALIZE,
        "haversine" => &OP_HAVERSINE,
        "haversine_deg_input" => &OP_HAVERSINE_DEG_INPUT,
        "deg_to_rad" => &OP_DEG_TO_RAD,
        "rad_to_deg" => &OP_RAD_TO_DEG,
        "get" => &OP_GET,
        "maybe_get" => &OP_MAYBE_GET,
        "chars" => &OP_CHARS,
        "slice_string" => &OP_SLICE_STRING,
        "from_substrings" => &OP_FROM_SUBSTRINGS,
        "slice" => &OP_SLICE,
        "regex_matches" => &OP_REGEX_MATCHES,
        "regex_replace" => &OP_REGEX_REPLACE,
        "regex_replace_all" => &OP_REGEX_REPLACE_ALL,
        "regex_extract" => &OP_REGEX_EXTRACT,
        "regex_extract_first" => &OP_REGEX_EXTRACT_FIRST,
        "t2s" => &OP_T2S,
        "encode_base64" => &OP_ENCODE_BASE64,
        "decode_base64" => &OP_DECODE_BASE64,
        "first" => &OP_FIRST,
        "last" => &OP_LAST,
        "chunks" => &OP_CHUNKS,
        "chunks_exact" => &OP_CHUNKS_EXACT,
        "windows" => &OP_WINDOWS,
        "to_int" => &OP_TO_INT,
        "to_float" => &OP_TO_FLOAT,
        "to_string" => &OP_TO_STRING,
        "l2_dist" => &OP_L2_DIST,
        "l2_normalize" => &OP_L2_NORMALIZE,
        "ip_dist" => &OP_IP_DIST,
        "cos_dist" => &OP_COS_DIST,
        "int_range" => &OP_INT_RANGE,
        "rand_float" => &OP_RAND_FLOAT,
        "rand_bernoulli" => &OP_RAND_BERNOULLI,
        "rand_int" => &OP_RAND_INT,
        "rand_choose" => &OP_RAND_CHOOSE,
        "assert" => &OP_ASSERT,
        "union" => &OP_UNION,
        "intersection" => &OP_INTERSECTION,
        "difference" => &OP_DIFFERENCE,
        "to_uuid" => &OP_TO_UUID,
        "to_bool" => &OP_TO_BOOL,
        "to_unity" => &OP_TO_UNITY,
        "rand_uuid_v1" => &OP_RAND_UUID_V1,
        "rand_uuid_v4" => &OP_RAND_UUID_V4,
        "uuid_timestamp" => &OP_UUID_TIMESTAMP,
        "validity" => &OP_VALIDITY,
        "now" => &OP_NOW,
        "format_timestamp" => &OP_FORMAT_TIMESTAMP,
        "parse_timestamp" => &OP_PARSE_TIMESTAMP,
        "vec" => &OP_VEC,
        "rand_vec" => &OP_RAND_VEC,
        _ => return None,
    })
}

impl Op {
    pub(crate) fn post_process_args(&self, args: &mut [Expr]) {
        if self.name.starts_with("OP_REGEX_") {
            args[1] = Expr::Apply {
                op: &OP_REGEX,
                args: [args[1].clone()].into(),
                span: args[1].span(),
            }
        }
    }
}
