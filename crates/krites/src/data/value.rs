//! Core value type for the Datalog engine.
#![expect(
    clippy::as_conversions,
    reason = "DataValue numeric conversions require i64/f64/pointer casts"
)]
#![expect(
    clippy::semicolon_if_nothing_returned,
    reason = "hash/display impls — semicolon not needed before closing brace"
)]
#![expect(
    clippy::explicit_iter_loop,
    reason = "explicit .iter() in DataValue collection processing"
)]
#![expect(
    clippy::needless_continue,
    reason = "explicit continue in sort comparison arms aids readability"
)]
#![expect(
    clippy::match_same_arms,
    reason = "Ord comparison arms are explicit for correctness auditing"
)]
#![expect(
    clippy::float_cmp,
    reason = "exact f64 equality check for hash consistency — not accumulated arithmetic"
)]
#![expect(
    clippy::doc_markdown,
    reason = "JsonValue and DataValues are type names in doc comments"
)]

use std::cmp::{Ordering, Reverse};
use std::collections::BTreeSet;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Deref;

use compact_str::CompactString;
use koina::base64;
use koina::uuid::Uuid;
use ndarray::Array1;
use ordered_float::OrderedFloat;
use regex::Regex;
use serde::de::{SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::digest::FixedOutput;
use sha2::{Digest, Sha256};

use crate::data::json::JsonValue;
use crate::data::relation::VecElementType;

/// Newtype wrapper around [`Uuid`] providing custom `Ord` for memcmp-compatible key ordering.
///
/// UUID fields are compared in (time_hi, time_mid, time_low, rest) order so that
/// v1 UUIDs sort chronologically in the storage layer.
#[derive(Clone, Debug, Hash, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct UuidWrapper(pub Uuid);

impl PartialOrd<Self> for UuidWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UuidWrapper {
    fn cmp(&self, other: &Self) -> Ordering {
        let (s_l, s_m, s_h, s_rest) = self.0.as_fields();
        let (o_l, o_m, o_h, o_rest) = other.0.as_fields();
        s_h.cmp(&o_h)
            .then_with(|| s_m.cmp(&o_m))
            .then_with(|| s_l.cmp(&o_l))
            .then_with(|| s_rest.cmp(o_rest))
    }
}

/// Compiled regex carried as a transient value inside the engine.
///
/// `RegexWrapper` is **not** serializable: it exists only during expression
/// evaluation (e.g., `regex_matches`). `Hash`, `Eq`, and `Ord` compare the
/// source pattern string, not compiled state.
#[derive(Clone)]
pub struct RegexWrapper(pub Regex);

impl Debug for RegexWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Regex({:?})", self.0.as_str())
    }
}

impl Hash for RegexWrapper {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_str().hash(state)
    }
}

impl Serialize for RegexWrapper {
    fn serialize<S>(&self, _serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom(
            "RegexWrapper is transient and cannot be serialized",
        ))
    }
}

impl<'de> Deserialize<'de> for RegexWrapper {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Err(serde::de::Error::custom(
            "RegexWrapper is transient and cannot be deserialized",
        ))
    }
}

impl PartialEq for RegexWrapper {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for RegexWrapper {}

impl Ord for RegexWrapper {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.as_str().cmp(other.0.as_str())
    }
}

impl PartialOrd for RegexWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Microsecond timestamp for time-travel validity, sorted **descending**.
///
/// Wraps `Reverse<i64>` so that the natural `Ord` puts newer timestamps first,
/// which is required by the key-scan logic in `check_key_for_validity`.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, serde::Deserialize, serde::Serialize, Hash, Debug,
)]
pub struct ValidityTs(pub Reverse<i64>);

/// Validity marker attached to stored tuples for time-travel queries.
///
/// Each stored fact carries a `(timestamp, is_assert)` pair. Assertions add the
/// fact at a point in time; retractions remove it. Both fields sort descending so
/// that the storage scan finds the most recent state first.
#[derive(
    Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Deserialize, serde::Serialize, Hash,
)]
pub struct Validity {
    /// Microsecond timestamp, sorted descending (newest first).
    pub timestamp: ValidityTs,
    /// `true` = assertion, `false` = retraction; sorted descending.
    pub is_assert: Reverse<bool>,
}

impl From<(i64, bool)> for Validity {
    fn from(value: (i64, bool)) -> Self {
        Self {
            timestamp: ValidityTs(Reverse(value.0)),
            is_assert: Reverse(value.1),
        }
    }
}

/// The core value type for every datum in the Datalog engine.
///
/// `DataValue` appears in tuples, expressions, function arguments, and storage
/// keys. Variant ordering defines the memcmp sort order used by the storage
/// layer (Null < Bool < Num < Str < ... < Bot).
///
/// # Internal-only variants
///
/// `Regex`, `Set`, `Validity`, and `Bot` are engine-internal: they never appear
/// in user-facing query results. `Bot` is the bottom sentinel used as the
/// upper-bound key in range scans.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize, Hash)]
#[non_exhaustive]
pub enum DataValue {
    /// The null (absent) value. Sorts before all other variants.
    Null,
    /// Boolean truth value.
    Bool(bool),
    /// Numeric value — integer or float (see [`Num`]).
    Num(Num),
    /// UTF-8 string, stored inline via [`CompactString`].
    Str(CompactString),
    /// Raw byte sequence, serialized via `serde_bytes`.
    #[serde(with = "serde_bytes")]
    Bytes(Vec<u8>),
    /// UUID value with chronological sort order (see [`UuidWrapper`]).
    Uuid(UuidWrapper),
    /// Compiled regex — transient, not serializable. Engine-internal.
    Regex(RegexWrapper),
    /// Ordered sequence of values.
    List(Vec<DataValue>),
    /// Deduplicated ordered set. Engine-internal; coerced to `List` at output.
    Set(BTreeSet<DataValue>),
    /// Typed floating-point vector for proximity search (HNSW).
    Vec(Vector),
    /// Arbitrary JSON value (objects, arrays, etc.).
    Json(JsonData),
    /// Timestamp + assertion flag for time-travel queries. Engine-internal.
    Validity(Validity),
    /// Bottom sentinel — sorts after everything. Used as upper key bound. Engine-internal.
    Bot,
}

/// Wrapper around [`JsonValue`] that provides `Ord` and `Hash` (via string representation).
///
/// JSON objects and arrays are stored as opaque blobs in the Datalog engine.
/// Ordering and hashing use the JSON serialized form, which is sufficient for
/// key deduplication but **not** semantically meaningful (different key orders
/// in objects compare differently).
#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct JsonData(pub JsonValue);

impl Debug for JsonData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "JsonData({})", self.0)
    }
}

impl PartialOrd<Self> for JsonData {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JsonData {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.to_string().cmp(&other.0.to_string())
    }
}

impl Deref for JsonData {
    type Target = JsonValue;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Hash for JsonData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_string().hash(state)
    }
}

/// Typed dense vector of floating-point numbers, used for HNSW proximity search.
///
/// Supports both single- and double-precision representations. Distance
/// functions (`l2_dist`, `ip_dist`, `cos_dist`) operate on vectors of the
/// same element type.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Vector {
    /// Single-precision (32-bit) float array.
    F32(Array1<f32>),
    /// Double-precision (64-bit) float array.
    F64(Array1<f64>),
}

struct VecBytes<'a>(&'a [u8]);

impl serde::Serialize for VecBytes<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(self.0)
    }
}

impl serde::Serialize for Vector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_tuple(2)?;
        match self {
            Vector::F32(a) => {
                state.serialize_element(&0u8)?;
                let arr = a.as_slice().unwrap_or(&[]);
                let bytes: &[u8] = bytemuck::cast_slice(arr);
                state.serialize_element(&VecBytes(bytes))?;
            }
            Vector::F64(a) => {
                state.serialize_element(&1u8)?;
                let arr = a.as_slice().unwrap_or(&[]);
                let bytes: &[u8] = bytemuck::cast_slice(arr);
                state.serialize_element(&VecBytes(bytes))?;
            }
        }
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for Vector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(2, VectorVisitor)
    }
}

struct VectorVisitor;

impl<'de> Visitor<'de> for VectorVisitor {
    type Value = Vector;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("vector representation")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let tag: u8 = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
        let bytes: &[u8] = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
        match tag {
            0u8 => {
                // WHY: bytemuck::try_cast_slice requires f32 alignment (4 bytes), but
                // msgpack binary payloads are not guaranteed to be aligned within the
                // stream buffer. Fall back to per-element copy on alignment failure.
                let floats = cast_bytes_to_f32_vec(bytes)
                    .map_err(|e| serde::de::Error::custom(format!("f32 cast: {e}")))?;
                Ok(Vector::F32(Array1::from(floats)))
            }
            1u8 => {
                let floats = cast_bytes_to_f64_vec(bytes)
                    .map_err(|e| serde::de::Error::custom(format!("f64 cast: {e}")))?;
                Ok(Vector::F64(Array1::from(floats)))
            }
            _ => Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Unsigned(u64::from(tag)),
                &self,
            )),
        }
    }
}

/// Decode raw bytes into a `Vec<f32>`, tolerating unaligned input.
///
/// Tries a zero-copy `bytemuck::try_cast_slice` first. If that fails due to
/// alignment (common when bytes are borrowed from a msgpack stream), falls back
/// to copying each 4-byte chunk via `f32::from_le_bytes`.
fn cast_bytes_to_f32_vec(bytes: &[u8]) -> std::result::Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err(format!(
            "byte length {} is not a multiple of 4",
            bytes.len()
        ));
    }
    // Fast path: aligned data can be zero-copy cast.
    if let Ok(floats) = bytemuck::try_cast_slice(bytes) {
        return Ok(floats.to_vec());
    }
    // Slow path: copy 4 bytes at a time to satisfy alignment.
    Ok(bytes
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| {
            let arr: [u8; 4] = chunk
                .try_into()
                .unwrap_or_else(|_| unreachable!("chunks_exact guarantees 4 bytes"));
            f32::from_le_bytes(arr)
        })
        .collect())
}

/// Decode raw bytes into a `Vec<f64>`, tolerating unaligned input.
///
/// Same strategy as [`cast_bytes_to_f32_vec`] but for 8-byte doubles.
fn cast_bytes_to_f64_vec(bytes: &[u8]) -> std::result::Result<Vec<f64>, String> {
    if !bytes.len().is_multiple_of(std::mem::size_of::<f64>()) {
        return Err(format!(
            "byte length {} is not a multiple of 8",
            bytes.len()
        ));
    }
    if let Ok(floats) = bytemuck::try_cast_slice(bytes) {
        return Ok(floats.to_vec());
    }
    Ok(bytes
        .chunks_exact(std::mem::size_of::<f64>())
        .map(|chunk| {
            let arr: [u8; 8] = chunk
                .try_into()
                .unwrap_or_else(|_| unreachable!("chunks_exact guarantees 8 bytes"));
            f64::from_le_bytes(arr)
        })
        .collect())
}

impl Vector {
    /// Get the length of the vector
    pub fn len(&self) -> usize {
        match self {
            Vector::F32(v) => v.len(),
            Vector::F64(v) => v.len(),
        }
    }
    /// Check if the vector is empty
    pub fn is_empty(&self) -> bool {
        match self {
            Vector::F32(v) => v.is_empty(),
            Vector::F64(v) => v.is_empty(),
        }
    }
    pub(crate) fn el_type(&self) -> VecElementType {
        match self {
            Vector::F32(_) => VecElementType::F32,
            Vector::F64(_) => VecElementType::F64,
        }
    }
    pub(crate) fn get_hash(&self) -> impl AsRef<[u8]> {
        let mut hasher = Sha256::new();
        match self {
            Vector::F32(v) => {
                for e in v.iter() {
                    hasher.update(e.to_le_bytes());
                }
            }
            Vector::F64(v) => {
                for e in v.iter() {
                    hasher.update(e.to_le_bytes());
                }
            }
        }
        hasher.finalize_fixed()
    }
}

impl PartialEq<Self> for Vector {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Vector::F32(l), Vector::F32(r)) => {
                if l.len() != r.len() {
                    return false;
                }
                for (le, re) in l.iter().zip(r) {
                    if !OrderedFloat(*le).eq(&OrderedFloat(*re)) {
                        return false;
                    }
                }
                true
            }
            (Vector::F64(l), Vector::F64(r)) => {
                if l.len() != r.len() {
                    return false;
                }
                for (le, re) in l.iter().zip(r) {
                    if !OrderedFloat(*le).eq(&OrderedFloat(*re)) {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }
}

impl Eq for Vector {}

impl PartialOrd for Vector {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Vector {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Vector::F32(l), Vector::F32(r)) => {
                match l.len().cmp(&r.len()) {
                    Ordering::Equal => (),
                    o => return o,
                }
                for (le, re) in l.iter().zip(r) {
                    match OrderedFloat(*le).cmp(&OrderedFloat(*re)) {
                        Ordering::Equal => continue,
                        o => return o,
                    }
                }
                Ordering::Equal
            }
            (Vector::F32(_), Vector::F64(_)) => Ordering::Less,
            (Vector::F64(l), Vector::F64(r)) => {
                match l.len().cmp(&r.len()) {
                    Ordering::Equal => (),
                    o => return o,
                }
                for (le, re) in l.iter().zip(r) {
                    match OrderedFloat(*le).cmp(&OrderedFloat(*re)) {
                        Ordering::Equal => continue,
                        o => return o,
                    }
                }
                Ordering::Equal
            }
            (Vector::F64(_), Vector::F32(_)) => Ordering::Greater,
        }
    }
}

impl Hash for Vector {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Vector::F32(a) => {
                for el in a {
                    OrderedFloat(*el).hash(state)
                }
            }
            Vector::F64(a) => {
                for el in a {
                    OrderedFloat(*el).hash(state)
                }
            }
        }
    }
}

impl From<i64> for DataValue {
    fn from(v: i64) -> Self {
        DataValue::Num(Num::Int(v))
    }
}

impl From<f64> for DataValue {
    fn from(v: f64) -> Self {
        // INVARIANT: NaN must not enter the DataValue graph as a valid
        // numeric node. Downstream Datalog arithmetic treats NaN as a
        // poison value that propagates silently through rules.
        if v.is_nan() {
            return DataValue::Null;
        }
        DataValue::Num(Num::Float(v))
    }
}

impl From<&str> for DataValue {
    fn from(v: &str) -> Self {
        DataValue::Str(CompactString::from(v))
    }
}

impl From<String> for DataValue {
    fn from(v: String) -> Self {
        DataValue::Str(CompactString::from(v))
    }
}

impl From<CompactString> for DataValue {
    fn from(v: CompactString) -> Self {
        DataValue::Str(v)
    }
}

impl From<bool> for DataValue {
    fn from(value: bool) -> Self {
        DataValue::Bool(value)
    }
}

/// Numeric value: either an exact integer or an IEEE 754 double.
///
/// Mixed-type arithmetic promotes `Int` to `Float`. The custom `Ord`
/// implementation sorts `Int(n)` before `Float(n)` when the float
/// representation is identical, preserving type distinction in key ordering.
#[derive(Copy, Clone, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub enum Num {
    /// Exact integer value.
    Int(i64),
    /// IEEE 754 double-precision floating-point value.
    Float(f64),
}

impl Hash for Num {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Num::Int(i) => i.hash(state),
            Num::Float(f) => OrderedFloat(*f).hash(state),
        }
    }
}

impl Num {
    /// Extract the integer value, converting floats that are whole numbers.
    pub fn get_int(&self) -> Option<i64> {
        match self {
            Num::Int(i) => Some(*i),
            Num::Float(f) => {
                if f.round() == *f {
                    // WARNING: preserves historical saturating-cast behavior for
                    // floats outside [i64::MIN, i64::MAX]; tightening this would
                    // ripple across all get_int() callers. Tracked under #2760.
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "preserves pre-existing behavior; out-of-range floats saturate"
                    )]
                    let i = *f as i64;
                    Some(i)
                } else {
                    None
                }
            }
        }
    }
    /// Convert to f64, promoting integers via `as` cast (precision loss above 2^53).
    pub(crate) fn get_float(&self) -> f64 {
        match self {
            #[expect(
                clippy::cast_precision_loss,
                reason = "i64 to f64: precision loss above 2^53 is acceptable for this path"
            )]
            Num::Int(i) => *i as f64,
            Num::Float(f) => *f,
        }
    }
}

impl PartialEq for Num {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Num {}

impl Display for Num {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Num::Int(i) => write!(f, "{i}"),
            Num::Float(n) => {
                if n.is_nan() {
                    write!(f, r#"to_float("NAN")"#)
                } else if n.is_infinite() {
                    if n.is_sign_negative() {
                        write!(f, r#"to_float("NEG_INF")"#)
                    } else {
                        write!(f, r#"to_float("INF")"#)
                    }
                } else {
                    write!(f, "{n}")
                }
            }
        }
    }
}

impl Debug for Num {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Num::Int(i) => write!(f, "{i}"),
            Num::Float(n) => write!(f, "{n}"),
        }
    }
}

impl PartialOrd for Num {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Num {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Num::Int(i), Num::Float(r)) => {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "i64 to f64: precision loss acceptable"
                )]
                let l = *i as f64;
                match l.total_cmp(r) {
                    Ordering::Less => Ordering::Less,
                    Ordering::Equal => Ordering::Less,
                    Ordering::Greater => Ordering::Greater,
                }
            }
            (Num::Float(l), Num::Int(i)) => {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "i64 to f64: precision loss acceptable"
                )]
                let r = *i as f64;
                match l.total_cmp(&r) {
                    Ordering::Less => Ordering::Less,
                    Ordering::Equal => Ordering::Greater,
                    Ordering::Greater => Ordering::Greater,
                }
            }
            (Num::Int(l), Num::Int(r)) => l.cmp(r),
            (Num::Float(l), Num::Float(r)) => l.total_cmp(r),
        }
    }
}

impl Debug for DataValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl Display for DataValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DataValue::Null => f.write_str("null"),
            DataValue::Bool(b) => write!(f, "{b}"),
            DataValue::Num(n) => write!(f, "{n}"),
            DataValue::Str(s) => write!(f, "{s:?}"),
            DataValue::Bytes(b) => {
                let bs = base64::encode(b);
                write!(f, "decode_base64({bs:?})")
            }
            DataValue::Uuid(u) => {
                let us = u.0.to_string();
                write!(f, "to_uuid({us:?})")
            }
            DataValue::Regex(rx) => {
                write!(f, "regex({:?})", rx.0.as_str())
            }
            DataValue::List(ls) => f.debug_list().entries(ls).finish(),
            DataValue::Set(s) => f.debug_list().entries(s).finish(),
            DataValue::Bot => write!(f, "null"),
            DataValue::Validity(v) => f
                .debug_struct("Validity")
                .field("timestamp", &v.timestamp.0)
                .field("retracted", &v.is_assert)
                .finish(),
            DataValue::Vec(a) => match a {
                Vector::F32(a) => {
                    write!(f, "vec({:?})", a.to_vec())
                }
                Vector::F64(a) => {
                    write!(f, "vec({:?}, \"F64\")", a.to_vec())
                }
            },
            DataValue::Json(j) => {
                if j.is_object() {
                    write!(f, "{}", j.0)
                } else {
                    write!(f, "json({})", j.0)
                }
            }
        }
    }
}

impl DataValue {
    /// Returns a slice of bytes if this one is a Bytes
    pub fn get_bytes(&self) -> Option<&[u8]> {
        match self {
            DataValue::Bytes(b) => Some(b),
            _ => None,
        }
    }
    /// Returns a slice of DataValues if this one is a List
    pub fn get_slice(&self) -> Option<&[DataValue]> {
        match self {
            DataValue::List(l) => Some(l),
            _ => None,
        }
    }
    /// Returns the raw str if this one is a Str
    pub fn get_str(&self) -> Option<&str> {
        match self {
            DataValue::Str(s) => Some(s),
            _ => None,
        }
    }
    /// Returns int if this one is an int
    pub fn get_int(&self) -> Option<i64> {
        match self {
            DataValue::Num(n) => n.get_int(),
            _ => None,
        }
    }
    /// Returns the integer value if non-negative, converting whole floats.
    pub(crate) fn get_non_neg_int(&self) -> Option<u64> {
        match self {
            DataValue::Num(n) => n.get_int().and_then(|i| u64::try_from(i).ok()),
            _ => None,
        }
    }
    /// Returns float if this one is.
    pub fn get_float(&self) -> Option<f64> {
        match self {
            DataValue::Num(n) => Some(n.get_float()),
            _ => None,
        }
    }
    /// Returns bool if this one is.
    pub fn get_bool(&self) -> Option<bool> {
        match self {
            DataValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    /// Construct a `DataValue::Uuid` from a raw [`Uuid`].
    pub(crate) fn uuid(uuid: Uuid) -> Self {
        Self::Uuid(UuidWrapper(uuid))
    }
    /// Extract UUID, also parsing from `Str` if the string is a valid UUID.
    pub(crate) fn get_uuid(&self) -> Option<Uuid> {
        match self {
            DataValue::Uuid(UuidWrapper(uuid)) => Some(*uuid),
            DataValue::Str(s) => Uuid::parse_str(s).ok(), // WHY: parse failure means not a valid UUID; None is correct
            _ => None,
        }
    }
}

/// Largest valid Unicode code point — used as the suffix for prefix-scan upper bounds.
pub(crate) const LARGEST_UTF_CHAR: char = '\u{10ffff}';
