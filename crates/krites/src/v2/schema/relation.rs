//! Relation schema: column definitions, types, and validation.

use serde::{Deserialize, Serialize};

use crate::v2::error::{self, Result};
use crate::v2::value::Value;

// ---------------------------------------------------------------------------
// Column types
// ---------------------------------------------------------------------------

/// Data type for a column in a stored relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ColumnType {
    /// UTF-8 string.
    String,
    /// 64-bit signed integer.
    Int,
    /// 64-bit float.
    Float,
    /// Boolean.
    Bool,
    /// Raw bytes.
    Bytes,
    /// Any type (no constraint). Used for flexible schemas.
    Any,
    /// Embedding vector with fixed dimensionality.
    Vector {
        /// Vector element type.
        dtype: VectorDtype,
        /// Expected dimensionality (0 = unconstrained).
        dim: u32,
    },
    /// Timestamp.
    Timestamp,
}

/// Vector element data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum VectorDtype {
    /// 32-bit float (standard for embeddings).
    F32,
    /// 64-bit float.
    F64,
}

impl ColumnType {
    /// Check whether a value is compatible with this column type.
    #[must_use]
    pub fn accepts(&self, value: &Value) -> bool {
        match (self, value) {
            (_, Value::Null) => true, // WHY: null is valid for any optional column.
            (Self::Any, _) => true,
            (Self::String, Value::Str(_)) => true,
            (Self::Int, Value::Int(_)) => true,
            (Self::Float, Value::Float(_)) => true,
            (Self::Float, Value::Int(_)) => true, // WHY: int→float promotion for numeric columns.
            (Self::Bool, Value::Bool(_)) => true,
            (Self::Bytes, Value::Bytes(_)) => true,
            (Self::Timestamp, Value::Timestamp(_)) => true,
            (Self::Vector { .. }, Value::Vector(_)) => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Column definition
// ---------------------------------------------------------------------------

/// A single column in a relation schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ColumnDef {
    /// Column name.
    pub name: String,
    /// Column data type.
    pub column_type: ColumnType,
    /// Whether this column is part of the key (vs. value).
    ///
    /// Key columns determine uniqueness: a tuple with the same key
    /// columns overwrites the previous tuple (upsert semantics).
    pub is_key: bool,
    /// Whether this column is optional (accepts null).
    pub nullable: bool,
    /// Default value if the column is omitted in an insert.
    pub default: Option<Value>,
}

impl ColumnDef {
    /// Create a required key column.
    #[must_use]
    pub fn key(name: impl Into<String>, column_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            column_type,
            is_key: true,
            nullable: false,
            default: None,
        }
    }

    /// Create a required value column.
    #[must_use]
    pub fn value(name: impl Into<String>, column_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            column_type,
            is_key: false,
            nullable: false,
            default: None,
        }
    }

    /// Create an optional value column (nullable).
    #[must_use]
    pub fn optional(name: impl Into<String>, column_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            column_type,
            is_key: false,
            nullable: true,
            default: None,
        }
    }

    /// Set a default value for this column.
    #[must_use]
    pub fn with_default(mut self, default: Value) -> Self {
        self.default = Some(default);
        self
    }
}

// ---------------------------------------------------------------------------
// Relation schema
// ---------------------------------------------------------------------------

/// Schema for a stored relation.
///
/// Defines the column layout, key/value split, and validation rules.
/// Created by `:create` DDL statements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RelationSchema {
    /// Relation name (e.g., "facts", "entities").
    pub name: String,
    /// Column definitions in declaration order.
    pub columns: Vec<ColumnDef>,
}

impl RelationSchema {
    /// Create a new schema with the given name and columns.
    #[must_use]
    pub fn new(name: impl Into<String>, columns: Vec<ColumnDef>) -> Self {
        Self {
            name: name.into(),
            columns,
        }
    }

    /// Key columns (determine uniqueness).
    pub fn key_columns(&self) -> impl Iterator<Item = &ColumnDef> {
        self.columns.iter().filter(|c| c.is_key)
    }

    /// Value columns (non-key).
    pub fn value_columns(&self) -> impl Iterator<Item = &ColumnDef> {
        self.columns.iter().filter(|c| !c.is_key)
    }

    /// Number of columns.
    #[must_use]
    pub fn arity(&self) -> usize {
        self.columns.len()
    }

    /// Find a column by name.
    #[must_use]
    pub fn column(&self, name: &str) -> Option<&ColumnDef> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Column index by name.
    #[must_use]
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    /// Validate a tuple against this schema.
    ///
    /// Checks: correct arity, type compatibility for each column,
    /// non-null for required columns.
    ///
    /// # Errors
    ///
    /// Returns `Schema` error with a description of the first violation.
    pub fn validate_tuple(&self, tuple: &[Value]) -> Result<()> {
        if tuple.len() != self.columns.len() {
            return Err(error::SchemaSnafu {
                message: format!(
                    "relation '{}': expected {} columns, got {}",
                    self.name,
                    self.columns.len(),
                    tuple.len()
                ),
            }
            .build());
        }

        for (i, (col, val)) in self.columns.iter().zip(tuple.iter()).enumerate() {
            if val.is_null() && !col.nullable {
                return Err(error::SchemaSnafu {
                    message: format!(
                        "relation '{}': column '{}' (index {i}) is not nullable",
                        self.name, col.name
                    ),
                }
                .build());
            }

            if !col.column_type.accepts(val) {
                return Err(error::SchemaSnafu {
                    message: format!(
                        "relation '{}': column '{}' (index {i}) expected {}, got {}",
                        self.name,
                        col.name,
                        column_type_name(col.column_type),
                        val.type_name()
                    ),
                }
                .build());
            }
        }

        Ok(())
    }

    /// Apply defaults for missing value columns, extending a key-only tuple.
    ///
    /// If the input tuple has only key columns, fills in value columns
    /// with their defaults (or null for optional columns).
    #[must_use]
    pub fn apply_defaults(&self, mut tuple: Vec<Value>) -> Vec<Value> {
        while tuple.len() < self.columns.len() {
            let col = &self.columns[tuple.len()];
            if let Some(ref default) = col.default {
                tuple.push(default.clone());
            } else if col.nullable {
                tuple.push(Value::Null);
            } else {
                // Missing required column — validation will catch this.
                break;
            }
        }
        tuple
    }
}

fn column_type_name(ct: ColumnType) -> &'static str {
    match ct {
        ColumnType::String => "string",
        ColumnType::Int => "int",
        ColumnType::Float => "float",
        ColumnType::Bool => "bool",
        ColumnType::Bytes => "bytes",
        ColumnType::Any => "any",
        ColumnType::Vector { .. } => "vector",
        ColumnType::Timestamp => "timestamp",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn facts_schema() -> RelationSchema {
        RelationSchema::new(
            "facts",
            vec![
                ColumnDef::key("id", ColumnType::String),
                ColumnDef::value("content", ColumnType::String),
                ColumnDef::value("confidence", ColumnType::Float),
                ColumnDef::optional("nous_id", ColumnType::String),
                ColumnDef::value("is_forgotten", ColumnType::Bool)
                    .with_default(Value::from(false)),
            ],
        )
    }

    #[test]
    fn schema_arity() {
        let schema = facts_schema();
        assert_eq!(schema.arity(), 5);
        assert_eq!(schema.key_columns().count(), 1);
        assert_eq!(schema.value_columns().count(), 4);
    }

    #[test]
    fn column_lookup() {
        let schema = facts_schema();
        assert!(schema.column("id").is_some());
        assert!(schema.column("missing").is_none());
        assert_eq!(schema.column_index("content"), Some(1));
    }

    #[test]
    fn validate_valid_tuple() {
        let schema = facts_schema();
        let tuple = vec![
            Value::from("fact-001"),
            Value::from("the sky is blue"),
            Value::from(0.95),
            Value::Null, // optional nous_id
            Value::from(false),
        ];
        assert!(schema.validate_tuple(&tuple).is_ok());
    }

    #[test]
    fn validate_wrong_arity() {
        let schema = facts_schema();
        let tuple = vec![Value::from("fact-001")];
        let err = schema.validate_tuple(&tuple).unwrap_err();
        assert!(err.to_string().contains("expected 5 columns, got 1"));
    }

    #[test]
    fn validate_type_mismatch() {
        let schema = facts_schema();
        let tuple = vec![
            Value::from("fact-001"),
            Value::from(42_i64), // should be string
            Value::from(0.95),
            Value::Null,
            Value::from(false),
        ];
        let err = schema.validate_tuple(&tuple).unwrap_err();
        assert!(err.to_string().contains("expected string, got int"));
    }

    #[test]
    fn validate_null_on_required() {
        let schema = facts_schema();
        let tuple = vec![
            Value::Null, // id is required
            Value::from("content"),
            Value::from(0.5),
            Value::Null,
            Value::from(false),
        ];
        let err = schema.validate_tuple(&tuple).unwrap_err();
        assert!(err.to_string().contains("not nullable"));
    }

    #[test]
    fn int_to_float_promotion() {
        let schema = facts_schema();
        let tuple = vec![
            Value::from("fact-001"),
            Value::from("content"),
            Value::from(1_i64), // int in float column — should accept
            Value::Null,
            Value::from(false),
        ];
        assert!(schema.validate_tuple(&tuple).is_ok());
    }

    #[test]
    fn apply_defaults() {
        let schema = facts_schema();
        // Only provide key + content + confidence.
        let tuple = vec![
            Value::from("fact-001"),
            Value::from("content"),
            Value::from(0.9),
        ];
        let filled = schema.apply_defaults(tuple);
        assert_eq!(filled.len(), 5);
        assert!(filled[3].is_null()); // optional nous_id → null
        assert_eq!(filled[4].as_bool(), Some(false)); // default is_forgotten
    }

    #[test]
    fn column_type_accepts() {
        assert!(ColumnType::String.accepts(&Value::from("x")));
        assert!(ColumnType::String.accepts(&Value::Null));
        assert!(!ColumnType::String.accepts(&Value::from(42_i64)));
        assert!(ColumnType::Float.accepts(&Value::from(42_i64))); // int→float
        assert!(ColumnType::Any.accepts(&Value::from("anything")));
    }

    #[test]
    fn schema_serde_roundtrip() {
        let schema = facts_schema();
        let json = serde_json::to_string(&schema).unwrap();
        let deserialized: RelationSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "facts");
        assert_eq!(deserialized.arity(), 5);
    }
}
