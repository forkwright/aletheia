//! Runtime values for the krites v2 engine.
//!
//! [`Value`] is the universal type for Datalog variables, parameters, and
//! results. It maps to eidos types at the API boundary (facts, entities,
//! relationships use typed fields; the engine uses Value internally).

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

/// A runtime value in the Datalog engine.
///
/// WHY Arc: Values are shared across joins, deduplication, intermediate
/// result sets, and cache entries. A single fact's content string may
/// appear in dozens of join tuples simultaneously. Arc avoids copying
/// heap data on every clone while keeping the enum `Send + Sync`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Value {
    /// Missing or undefined.
    Null,
    /// Boolean.
    Bool(bool),
    /// 64-bit signed integer.
    Int(i64),
    /// 64-bit float.
    Float(f64),
    /// UTF-8 string (Arc: shared across join tuples and cache).
    Str(Arc<str>),
    /// Raw bytes.
    Bytes(Arc<[u8]>),
    /// Ordered list of values.
    List(Arc<[Value]>),
    /// Embedding vector.
    Vector(VectorValue),
    /// Timestamp (jiff).
    Timestamp(jiff::Timestamp),
}

/// Embedding vector variants.
///
/// WHY Arc: Embedding vectors are 384-1536 floats (1.5-6 KB). They're
/// referenced from HNSW index entries, search results, and fact records
/// simultaneously. Arc avoids duplicating the allocation.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum VectorValue {
    /// 32-bit float vector (standard for embeddings).
    F32(Arc<[f32]>),
    /// 64-bit float vector.
    F64(Arc<[f64]>),
}

// ---------------------------------------------------------------------------
// Serde: serialize Arc<T> via the inner T
// ---------------------------------------------------------------------------

mod serde_impl {
    use super::*;
    use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
    use serde::ser::{SerializeMap, Serializer};

    impl Serialize for Value {
        fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
            // WHY: tagged map format for unambiguous round-trip. Each variant
            // serializes as {"type": "...", "value": ...} so deserialization
            // can reconstruct the correct Arc wrapper.
            let mut map = ser.serialize_map(Some(2))?;
            match self {
                Value::Null => {
                    map.serialize_entry("type", "null")?;
                    map.serialize_entry("value", &())?;
                }
                Value::Bool(v) => {
                    map.serialize_entry("type", "bool")?;
                    map.serialize_entry("value", v)?;
                }
                Value::Int(v) => {
                    map.serialize_entry("type", "int")?;
                    map.serialize_entry("value", v)?;
                }
                Value::Float(v) => {
                    map.serialize_entry("type", "float")?;
                    map.serialize_entry("value", v)?;
                }
                Value::Str(v) => {
                    map.serialize_entry("type", "str")?;
                    map.serialize_entry("value", v.as_ref())?;
                }
                Value::Bytes(v) => {
                    map.serialize_entry("type", "bytes")?;
                    map.serialize_entry("value", &base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        v.as_ref(),
                    ))?;
                }
                Value::List(v) => {
                    map.serialize_entry("type", "list")?;
                    map.serialize_entry("value", v.as_ref())?;
                }
                Value::Vector(v) => {
                    match v {
                        VectorValue::F32(data) => {
                            map.serialize_entry("type", "vec_f32")?;
                            map.serialize_entry("value", data.as_ref())?;
                        }
                        VectorValue::F64(data) => {
                            map.serialize_entry("type", "vec_f64")?;
                            map.serialize_entry("value", data.as_ref())?;
                        }
                    }
                }
                Value::Timestamp(v) => {
                    map.serialize_entry("type", "timestamp")?;
                    map.serialize_entry("value", &v.to_string())?;
                }
            }
            map.end()
        }
    }

    impl<'de> Deserialize<'de> for Value {
        fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
            de.deserialize_map(ValueVisitor)
        }
    }

    struct ValueVisitor;

    impl<'de> Visitor<'de> for ValueVisitor {
        type Value = Value;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a tagged Value map with 'type' and 'value' keys")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
            let mut typ: Option<String> = None;
            let mut raw: Option<serde_json::Value> = None;

            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "type" => typ = Some(map.next_value()?),
                    "value" => raw = Some(map.next_value()?),
                    _ => { let _ = map.next_value::<serde_json::Value>()?; }
                }
            }

            let typ = typ.ok_or_else(|| de::Error::missing_field("type"))?;
            let raw = raw.ok_or_else(|| de::Error::missing_field("value"))?;

            match typ.as_str() {
                "null" => Ok(Value::Null),
                "bool" => serde_json::from_value(raw).map(Value::Bool).map_err(de::Error::custom),
                "int" => serde_json::from_value(raw).map(Value::Int).map_err(de::Error::custom),
                "float" => serde_json::from_value(raw).map(Value::Float).map_err(de::Error::custom),
                "str" => {
                    let s: String = serde_json::from_value(raw).map_err(de::Error::custom)?;
                    Ok(Value::Str(Arc::from(s.as_str())))
                }
                "bytes" => {
                    let b64: String = serde_json::from_value(raw).map_err(de::Error::custom)?;
                    let bytes = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        &b64,
                    ).map_err(de::Error::custom)?;
                    Ok(Value::Bytes(Arc::from(bytes.as_slice())))
                }
                "list" => {
                    let items: Vec<Value> = serde_json::from_value(raw).map_err(de::Error::custom)?;
                    Ok(Value::List(Arc::from(items.as_slice())))
                }
                "vec_f32" => {
                    let data: Vec<f32> = serde_json::from_value(raw).map_err(de::Error::custom)?;
                    Ok(Value::Vector(VectorValue::F32(Arc::from(data.as_slice()))))
                }
                "vec_f64" => {
                    let data: Vec<f64> = serde_json::from_value(raw).map_err(de::Error::custom)?;
                    Ok(Value::Vector(VectorValue::F64(Arc::from(data.as_slice()))))
                }
                "timestamp" => {
                    let s: String = serde_json::from_value(raw).map_err(de::Error::custom)?;
                    let ts: jiff::Timestamp = s.parse().map_err(de::Error::custom)?;
                    Ok(Value::Timestamp(ts))
                }
                other => Err(de::Error::unknown_variant(other, &[
                    "null", "bool", "int", "float", "str", "bytes", "list",
                    "vec_f32", "vec_f64", "timestamp",
                ])),
            }
        }
    }

    impl Serialize for VectorValue {
        fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
            match self {
                VectorValue::F32(data) => data.as_ref().serialize(ser),
                VectorValue::F64(data) => data.as_ref().serialize(ser),
            }
        }
    }

    impl<'de> Deserialize<'de> for VectorValue {
        fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
            // WHY: default to F32 since that's the standard embedding dtype.
            let data: Vec<f32> = Vec::deserialize(de)?;
            Ok(VectorValue::F32(Arc::from(data.as_slice())))
        }
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::Int(i64::from(v))
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::Str(Arc::from(v.as_str()))
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::Str(Arc::from(v))
    }
}

impl From<Arc<str>> for Value {
    fn from(v: Arc<str>) -> Self {
        Self::Str(v)
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Self::Bytes(Arc::from(v.as_slice()))
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Self::List(Arc::from(v.as_slice()))
    }
}

impl From<Vec<f32>> for Value {
    fn from(v: Vec<f32>) -> Self {
        Self::Vector(VectorValue::F32(Arc::from(v.as_slice())))
    }
}

impl From<Vec<f64>> for Value {
    fn from(v: Vec<f64>) -> Self {
        Self::Vector(VectorValue::F64(Arc::from(v.as_slice())))
    }
}

impl From<jiff::Timestamp> for Value {
    fn from(v: jiff::Timestamp) -> Self {
        Self::Timestamp(v)
    }
}

// ---------------------------------------------------------------------------
// Type queries
// ---------------------------------------------------------------------------

impl Value {
    /// Returns the type name for display and error messages.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Str(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::List(_) => "list",
            Self::Vector(_) => "vector",
            Self::Timestamp(_) => "timestamp",
        }
    }

    /// Extract as string reference, if this is a `Str` value.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Extract as i64, if this is an `Int` value.
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Extract as f64, if this is a `Float` value.
    #[must_use]
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Extract as bool, if this is a `Bool` value.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns true if this is `Null`.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Coerce to f64 for numeric operations (Int → f64, Float → f64).
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "i64 to f64: precision loss acceptable for numeric coercion"
    )]
    pub fn to_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Str(s) => write!(f, "{s}"),
            Self::Bytes(b) => write!(f, "<{} bytes>", b.len()),
            Self::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Self::Vector(v) => match v {
                VectorValue::F32(data) => write!(f, "<f32 vec, dim={}>", data.len()),
                VectorValue::F64(data) => write!(f, "<f64 vec, dim={}>", data.len()),
            },
            Self::Timestamp(ts) => write!(f, "{ts}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Ordering (total order for join keys and deduplication)
// ---------------------------------------------------------------------------

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        // WHY: total ordering is required for join keys, deduplication, and
        // sorted output. Floats use total_cmp for NaN handling. Cross-type
        // ordering uses discriminant.
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Int(a), Self::Int(b)) => a.cmp(b),
            (Self::Float(a), Self::Float(b)) => a.total_cmp(b),
            // WHY: Int/Float cross-comparison for numeric joins.
            #[expect(
                clippy::cast_precision_loss,
                reason = "i64 to f64: precision loss acceptable for cross-type numeric comparison"
            )]
            {
                (Self::Int(a), Self::Float(b)) => (*a as f64).total_cmp(b),
                (Self::Float(a), Self::Int(b)) => a.total_cmp(&(*b as f64)),
            }
            (Self::Str(a), Self::Str(b)) => a.cmp(b),
            (Self::Bytes(a), Self::Bytes(b)) => a.cmp(b),
            (Self::Timestamp(a), Self::Timestamp(b)) => a.cmp(b),
            (Self::List(a), Self::List(b)) => a.iter().cmp(b.iter()),
            // Cross-type: order by discriminant.
            _ => self.discriminant().cmp(&other.discriminant()),
        }
    }
}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.discriminant().hash(state);
        match self {
            Self::Null => {}
            Self::Bool(b) => b.hash(state),
            Self::Int(n) => n.hash(state),
            Self::Float(f) => f.to_bits().hash(state),
            Self::Str(s) => s.hash(state),
            Self::Bytes(b) => b.hash(state),
            Self::Timestamp(ts) => ts.as_nanosecond().hash(state),
            Self::List(items) => {
                items.len().hash(state);
                for item in items.iter() {
                    item.hash(state);
                }
            }
            Self::Vector(v) => match v {
                VectorValue::F32(data) => {
                    data.len().hash(state);
                    for f in data.iter() {
                        f.to_bits().hash(state);
                    }
                }
                VectorValue::F64(data) => {
                    data.len().hash(state);
                    for f in data.iter() {
                        f.to_bits().hash(state);
                    }
                }
            },
        }
    }
}

impl Value {
    /// Numeric discriminant for cross-type ordering.
    fn discriminant(&self) -> u8 {
        match self {
            Self::Null => 0,
            Self::Bool(_) => 1,
            Self::Int(_) => 2,
            Self::Float(_) => 3,
            Self::Str(_) => 4,
            Self::Bytes(_) => 5,
            Self::List(_) => 6,
            Self::Vector(_) => 7,
            Self::Timestamp(_) => 8,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn type_conversions() {
        assert_eq!(Value::from(42_i64).as_int(), Some(42));
        assert_eq!(Value::from(3.14).as_float(), Some(3.14));
        assert_eq!(Value::from("hello").as_str(), Some("hello"));
        assert_eq!(Value::from(true).as_bool(), Some(true));
        assert!(Value::Null.is_null());
    }

    #[test]
    fn ordering_same_type() {
        assert!(Value::from(1_i64) < Value::from(2_i64));
        assert!(Value::from("abc") < Value::from("def"));
        assert_eq!(Value::Null, Value::Null);
    }

    #[test]
    fn ordering_cross_type() {
        // Int and Float are comparable.
        assert_eq!(
            Value::from(1_i64).cmp(&Value::from(1.0)),
            Ordering::Equal
        );
        assert!(Value::from(1_i64) < Value::from(2.0));
    }

    #[test]
    fn ordering_different_types() {
        // Different type discriminants order by discriminant.
        assert!(Value::Null < Value::from(false));
        assert!(Value::from(false) < Value::from(0_i64));
        // WHY: Int and Float cross-compare numerically (for join correctness).
        assert_eq!(Value::from(0_i64).cmp(&Value::from(0.0)), Ordering::Equal);
        assert!(Value::from(1_i64) > Value::from(0.5));
        // Non-numeric types order by discriminant.
        assert!(Value::from(0.0) < Value::from(""));
        assert!(Value::from("") < Value::Bytes(Arc::from(b"".as_slice())));
    }

    #[test]
    fn display() {
        assert_eq!(Value::Null.to_string(), "null");
        assert_eq!(Value::from(42_i64).to_string(), "42");
        assert_eq!(Value::from("hello").to_string(), "hello");
    }

    #[test]
    fn hash_consistency() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Value::from(42_i64));
        set.insert(Value::from(42_i64));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn type_name() {
        assert_eq!(Value::Null.type_name(), "null");
        assert_eq!(Value::from(true).type_name(), "bool");
        assert_eq!(Value::from(1_i64).type_name(), "int");
        assert_eq!(Value::from(1.0).type_name(), "float");
        assert_eq!(Value::from("x").type_name(), "str");
    }

    #[test]
    fn vector_from_f32() {
        let v = Value::from(vec![1.0_f32, 2.0, 3.0]);
        assert_eq!(v.type_name(), "vector");
    }

    #[test]
    fn to_f64_coercion() {
        assert_eq!(Value::from(42_i64).to_f64(), Some(42.0));
        assert_eq!(Value::from(3.14).to_f64(), Some(3.14));
        assert_eq!(Value::from("x").to_f64(), None);
    }

    #[test]
    fn serde_roundtrip() {
        let values = vec![
            Value::Null,
            Value::from(true),
            Value::from(42_i64),
            Value::from(3.14),
            Value::from("hello"),
        ];
        for v in values {
            let json = serde_json::to_string(&v).unwrap();
            let deserialized: Value = serde_json::from_str(&json).unwrap();
            assert_eq!(v, deserialized);
        }
    }
}
