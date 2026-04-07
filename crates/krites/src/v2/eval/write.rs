//! Write operations for krites v2.
//!
//! Handles :put, :create, :replace, and :remove statements.

use std::collections::BTreeMap;

use crate::v2::error::{self, Result};
use crate::v2::eval::expr::eval_expr;
use crate::v2::parse::ast::{Expr, SchemaSpec};
use crate::v2::schema::{ColumnDef, ColumnType, RelationSchema};
use crate::v2::storage::{Storage, StorageTx};
use crate::v2::value::Value;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Internal relation name for storing schemas.
const SCHEMAS_RELATION: &str = "_schemas";

// ---------------------------------------------------------------------------
// Put operation
// ---------------------------------------------------------------------------

/// Evaluate a :put statement.
pub fn evaluate_put<S>(
    relation: &str,
    rows: &[Vec<(String, Expr)>],
    params: &BTreeMap<String, Value>,
    storage: &S,
    schema: &RelationSchema,
) -> Result<usize>
where
    S: Storage,
{
    let mut tx = storage.begin(true)?;
    let mut count = 0;

    for row_exprs in rows {
        // Evaluate expressions to get field values.
        let mut field_values: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
        for (col_name, expr) in row_exprs {
            let value = eval_expr(expr, &std::collections::HashMap::new(), params)?;
            field_values.insert(col_name.clone(), value);
        }

        // Build the full tuple according to schema.
        let tuple = build_tuple(schema, &field_values)?;

        // Validate tuple against schema.
        schema.validate_tuple(&tuple)?;

        // Serialize to key-value.
        let (key, value) = serialize_tuple(relation, schema, &tuple)?;

        // Write to storage.
        tx.put(&key, &value)?;
        count += 1;
    }

    tx.commit()?;
    Ok(count)
}

/// Build a tuple vector from field values, applying defaults.
fn build_tuple(
    schema: &RelationSchema,
    field_values: &std::collections::HashMap<String, Value>,
) -> Result<Vec<Value>> {
    let mut tuple = Vec::with_capacity(schema.arity());

    for col in &schema.columns {
        if let Some(val) = field_values.get(&col.name) {
            tuple.push(val.clone());
        } else if let Some(ref default) = col.default {
            tuple.push(default.clone());
        } else if col.nullable {
            tuple.push(Value::Null);
        } else {
            return Err(error::SchemaSnafu {
                message: format!(
                    "missing required column '{}' in relation '{}'",
                    col.name, schema.name
                ),
            }
            .build());
        }
    }

    Ok(tuple)
}

/// Serialize a tuple to storage key and value.
fn serialize_tuple(
    relation: &str,
    schema: &RelationSchema,
    tuple: &[Value],
) -> Result<(Vec<u8>, Vec<u8>)> {
    // Build key: "relation:key_col1:key_col2:..."
    let key_cols: Vec<_> = schema.key_columns().collect();
    let mut key_parts: Vec<String> = vec![relation.to_string()];

    for col in &key_cols {
        let idx = schema.column_index(&col.name).ok_or_else(|| error::SchemaSnafu {
            message: format!("key column '{}' not found in schema", col.name),
        }.build())?;
        let val = tuple.get(idx).ok_or_else(|| error::SchemaSnafu {
            message: format!("missing value for key column '{}'", col.name),
        }.build())?;
        key_parts.push(value_to_string(val));
    }

    let key = key_parts.join(":").into_bytes();

    // Build value: msgpack-serialized value columns.
    let value_cols: Vec<Value> = schema
        .value_columns()
        .filter_map(|col| {
            schema.column_index(&col.name).and_then(|idx| tuple.get(idx).cloned())
        })
        .collect();

    let value = rmp_serde::to_vec(&value_cols).map_err(|e| {
        error::StorageSnafu {
            message: format!("msgpack serialize error: {e}"),
        }
        .build()
    })?;

    Ok((key, value))
}

/// Convert a value to string for key encoding.
fn value_to_string(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => s.to_string(),
        Value::Bytes(b) => base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b),
        Value::List(_) => "<list>".to_string(),
        Value::Vector(_) => "<vector>".to_string(),
        Value::Timestamp(ts) => ts.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Create operation
// ---------------------------------------------------------------------------

/// Evaluate a :create statement.
pub fn evaluate_create<S>(
    relation: &str,
    schema_spec: &SchemaSpec,
    storage: &S,
) -> Result<RelationSchema>
where
    S: Storage,
{
    // Build RelationSchema from SchemaSpec.
    let mut columns = Vec::new();

    // Key columns come first.
    for key_col_name in &schema_spec.key_columns {
        columns.push(ColumnDef::key(
            key_col_name.clone(),
            infer_key_column_type(schema_spec, key_col_name),
        ));
    }

    // Value columns follow.
    for val_col_spec in &schema_spec.value_columns {
        let default = val_col_spec
            .default
            .as_ref()
            .map(|expr| eval_default_expr(expr))
            .transpose()?;

        columns.push(ColumnDef {
            name: val_col_spec.name.clone(),
            column_type: val_col_spec.column_type,
            is_key: false,
            nullable: default.is_none(), // Has default or is nullable.
            default,
        });
    }

    let schema = RelationSchema::new(relation, columns);

    // Persist schema to storage.
    persist_schema(&schema, storage)?;

    Ok(schema)
}

/// Infer the type of a key column from the schema spec.
fn infer_key_column_type(_schema_spec: &SchemaSpec, _key_col_name: &str) -> ColumnType {
    // For now, default to String. In a more complete implementation,
    // we might allow explicit type annotation in key columns.
    ColumnType::String
}

/// Evaluate a default expression to a value.
fn eval_default_expr(expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Literal(val) => Ok(val.clone()),
        _ => Err(error::EvalSnafu {
            message: "only literal values are supported as defaults".to_string(),
        }
        .build()),
    }
}

/// Persist a schema to storage.
fn persist_schema<S>(schema: &RelationSchema, storage: &S) -> Result<()>
where
    S: Storage,
{
    let mut tx = storage.begin(true)?;

    // Serialize schema to JSON.
    let schema_json = serde_json::to_vec(schema).map_err(|e| {
        error::StorageSnafu {
            message: format!("json serialize error: {e}"),
        }
        .build()
    })?;

    // Store in _schemas relation.
    let key = format!("{SCHEMAS_RELATION}:{}", schema.name).into_bytes();
    tx.put(&key, &schema_json)?;

    tx.commit()
}

// ---------------------------------------------------------------------------
// Replace operation
// ---------------------------------------------------------------------------

/// Evaluate a :replace statement (drop and recreate).
pub fn evaluate_replace<S>(
    relation: &str,
    schema_spec: &SchemaSpec,
    storage: &S,
) -> Result<RelationSchema>
where
    S: Storage,
{
    // First, remove the old relation if it exists.
    let _ = evaluate_remove(relation, storage);

    // Then create the new one.
    evaluate_create(relation, schema_spec, storage)
}

// ---------------------------------------------------------------------------
// Remove operation
// ---------------------------------------------------------------------------

/// Evaluate a :remove statement.
pub fn evaluate_remove<S>(relation: &str, storage: &S) -> Result<()>
where
    S: Storage,
{
    // Remove all data for this relation.
    let mut tx = storage.begin(true)?;

    let prefix = format!("{relation}:").into_bytes();
    let to_delete = tx.scan_prefix(&prefix)?;

    for (key, _) in to_delete {
        tx.delete(&key)?;
    }

    // Remove schema entry.
    let schema_key = format!("{SCHEMAS_RELATION}:{relation}").into_bytes();
    tx.delete(&schema_key)?;

    tx.commit()
}
