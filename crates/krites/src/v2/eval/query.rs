//! Query evaluation for krites v2.
//!
//! Implements semi-naive bottom-up evaluation of Datalog rules with:
//! - Stored relation scans
//! - Nested-loop joins
//! - Filter predicates
//! - Aggregations (count, sum, min, max, mean)
//! - Ordering and limit

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::v2::error::{self, Result};
use crate::v2::eval::expr::eval_expr;
use crate::v2::parse::ast::{
    Aggregation, Atom, Binding, Expr, Filter, OrderSpec, OutputCol, Query, Rule,
};
use crate::v2::rows::Rows;
use crate::v2::schema::RelationSchema;
use crate::v2::storage::{Storage, StorageTx};
use crate::v2::value::Value;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Evaluate a query and return the result rows.
pub fn evaluate_query<S>(
    query: &Query,
    params: &BTreeMap<String, Value>,
    storage: &S,
    schemas: &HashMap<String, RelationSchema>,
) -> Result<Rows>
where
    S: Storage,
{
    // Evaluate each rule (disjunction) and collect results.
    let mut all_rows: Vec<HashMap<String, Value>> = Vec::new();

    for rule in &query.rules {
        let rule_results = evaluate_rule(rule, params, storage, schemas)?;
        all_rows.extend(rule_results);
    }

    // Deduplicate rows (Datalog semantics: union is set union).
    all_rows = deduplicate_rows(all_rows);

    // Apply aggregations if any output columns have aggregation functions.
    let has_aggregation = query.outputs.iter().any(|o| o.aggregation.is_some());
    let result_rows = if has_aggregation {
        apply_aggregations(&all_rows, &query.outputs)?
    } else {
        // Project to output columns.
        project_rows(&all_rows, &query.outputs)?
    };

    // Apply ordering.
    let mut result_rows = result_rows;
    apply_ordering(&mut result_rows, &query.ordering)?;

    // Apply limit.
    apply_limit(&mut result_rows, query.limit.as_ref(), params)?;

    // Build result with headers.
    let headers: Vec<String> = query.outputs.iter().map(|o| o.name.clone()).collect();

    // Convert HashMap rows to Vec<Value> rows.
    let rows: Vec<Vec<Value>> = result_rows
        .into_iter()
        .map(|row| {
            headers
                .iter()
                .map(|h| row.get(h).cloned().unwrap_or(Value::Null))
                .collect()
        })
        .collect();

    Ok(Rows { headers, rows })
}

// ---------------------------------------------------------------------------
// Rule evaluation
// ---------------------------------------------------------------------------

/// Evaluate a single rule (conjunction of atoms with filters).
fn evaluate_rule<S>(
    rule: &Rule,
    params: &BTreeMap<String, Value>,
    storage: &S,
    schemas: &HashMap<String, RelationSchema>,
) -> Result<Vec<HashMap<String, Value>>>
where
    S: Storage,
{
    // Start with an empty binding set (single empty tuple).
    let mut bindings: Vec<HashMap<String, Value>> = vec![HashMap::new()];

    // Evaluate each atom, joining with current bindings.
    for atom in &rule.atoms {
        let atom_results = evaluate_atom(atom, params, storage, schemas)?;
        bindings = join_bindings(bindings, atom_results);
    }

    // Apply filters.
    bindings.retain(|binding| {
        rule.filters
            .iter()
            .all(|filter| eval_filter(filter, binding, params))
    });

    Ok(bindings)
}

// ---------------------------------------------------------------------------
// Atom evaluation
// ---------------------------------------------------------------------------

/// Evaluate an atom and return variable bindings.
fn evaluate_atom<S>(
    atom: &Atom,
    params: &BTreeMap<String, Value>,
    storage: &S,
    schemas: &HashMap<String, RelationSchema>,
) -> Result<Vec<HashMap<String, Value>>>
where
    S: Storage,
{
    match atom {
        Atom::Stored { relation, bindings } => {
            evaluate_stored_atom(relation, bindings, storage, schemas)
        }
        // Temp atoms are not yet supported (would require rule rewriting).
        Atom::Temp { name, .. } => Err(error::EvalSnafu {
            message: format!("temp relation '{name}' not yet supported"),
        }
        .build()),
        // Index lookups are deferred to separate PR.
        Atom::Index { relation, index, .. } => Err(error::EvalSnafu {
            message: format!("index lookup '{relation}:{index}' not yet supported"),
        }
        .build()),
        // Fixed rules are deferred to separate PR.
        Atom::FixedRule { name, .. } => Err(error::EvalSnafu {
            message: format!("fixed rule '{name}' not yet supported"),
        }
        .build()),
    }
}

/// Evaluate a stored relation atom by scanning.
fn evaluate_stored_atom<S>(
    relation: &str,
    bindings: &[Binding],
    storage: &S,
    schemas: &HashMap<String, RelationSchema>,
) -> Result<Vec<HashMap<String, Value>>>
where
    S: Storage,
{
    let schema = schemas.get(relation).ok_or_else(|| {
        error::EvalSnafu {
            message: format!("unknown relation: {relation}"),
        }
        .build()
    })?;

    let tx = storage.begin(false)?;
    let prefix = format!("{relation}:").into_bytes();
    let kvs = tx.scan_prefix(&prefix)?;

    let mut results: Vec<HashMap<String, Value>> = Vec::new();

    for (key, value) in kvs {
        // Parse the key and value to extract the full tuple.
        let tuple = deserialize_tuple(&key, &value, relation, schema)?;

        // Create variable bindings from this tuple.
        let mut binding = HashMap::new();
        for b in bindings {
            if let Some(column_name) = &b.column {
                // Named binding: col: var
                if let Some(idx) = schema.column_index(column_name) {
                    if let Some(val) = tuple.get(idx) {
                        binding.insert(b.variable.clone(), val.clone());
                    }
                }
            } else {
                // Positional binding: just the variable name as column name
                if let Some(idx) = schema.column_index(&b.variable) {
                    if let Some(val) = tuple.get(idx) {
                        binding.insert(b.variable.clone(), val.clone());
                    }
                }
            }
        }

        // Only include if we bound at least one variable.
        if !binding.is_empty() {
            results.push(binding);
        }
    }

    Ok(results)
}

/// Deserialize a key-value pair into a full tuple.
fn deserialize_tuple(
    key: &[u8],
    value: &[u8],
    relation: &str,
    schema: &RelationSchema,
) -> Result<Vec<Value>> {
    // Key format: "relation:key_col1:key_col2:..."
    let key_str = String::from_utf8_lossy(key);
    let parts: Vec<&str> = key_str.split(':').collect();

    // First part is relation name, rest are key column values.
    if parts.len() < 2 || parts[0] != relation {
        return Err(error::StorageSnafu {
            message: format!("invalid key format: {key_str}"),
        }
        .build());
    }

    // Parse key columns from the key string.
    let key_columns: Vec<&str> = parts[1..].to_vec();
    let key_col_defs: Vec<_> = schema.key_columns().collect();

    if key_columns.len() != key_col_defs.len() {
        return Err(error::StorageSnafu {
            message: format!(
                "key column count mismatch: expected {}, got {}",
                key_col_defs.len(),
                key_columns.len()
            ),
        }
        .build());
    }

    // Parse key column values.
    let mut tuple: Vec<Value> = Vec::with_capacity(schema.arity());
    for (col_def, key_val) in key_col_defs.iter().zip(key_columns.iter()) {
        let val = parse_value(key_val, &col_def.column_type)?;
        tuple.push(val);
    }

    // Deserialize value columns from msgpack.
    let value_cols: Vec<Value> = rmp_serde::from_slice(value).map_err(|e| {
        error::StorageSnafu {
            message: format!("msgpack deserialize error: {e}"),
        }
        .build()
    })?;

    tuple.extend(value_cols);

    Ok(tuple)
}

/// Parse a string value into the expected type.
fn parse_value(s: &str, column_type: &crate::v2::schema::ColumnType) -> Result<Value> {
    match column_type {
        crate::v2::schema::ColumnType::String => Ok(Value::from(s)),
        crate::v2::schema::ColumnType::Int => {
            let n = s.parse::<i64>().map_err(|e| {
                error::StorageSnafu {
                    message: format!("cannot parse '{s}' as int: {e}"),
                }
                .build()
            })?;
            Ok(Value::Int(n))
        }
        crate::v2::schema::ColumnType::Float => {
            let f = s.parse::<f64>().map_err(|e| {
                error::StorageSnafu {
                    message: format!("cannot parse '{s}' as float: {e}"),
                }
                .build()
            })?;
            Ok(Value::Float(f))
        }
        crate::v2::schema::ColumnType::Bool => match s {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(error::StorageSnafu {
                message: format!("cannot parse '{s}' as bool"),
            }
            .build()),
        },
        _ => Ok(Value::from(s)), // For other types, store as string.
    }
}

// ---------------------------------------------------------------------------
// Join operations
// ---------------------------------------------------------------------------

/// Join two sets of bindings on shared variables (nested loop join).
fn join_bindings(
    left: Vec<HashMap<String, Value>>,
    right: Vec<HashMap<String, Value>>,
) -> Vec<HashMap<String, Value>> {
    if left.is_empty() || right.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();

    for l in &left {
        for r in &right {
            // Check if shared variables have consistent values.
            let mut compatible = true;
            for (var, l_val) in l {
                if let Some(r_val) = r.get(var) {
                    if l_val != r_val {
                        compatible = false;
                        break;
                    }
                }
            }

            if compatible {
                // Merge bindings.
                let mut merged = l.clone();
                for (var, r_val) in r {
                    merged.entry(var.clone()).or_insert_with(|| r_val.clone());
                }
                result.push(merged);
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Filter evaluation
// ---------------------------------------------------------------------------

/// Evaluate a filter expression with the given bindings.
fn eval_filter(filter: &Filter, bindings: &HashMap<String, Value>, params: &BTreeMap<String, Value>) -> bool {
    match eval_expr(&filter.expr, bindings, params) {
        Ok(Value::Bool(b)) => b,
        Ok(other) => {
            // Non-boolean values are truthy if not null/false.
            !other.is_null() && other != Value::Bool(false)
        }
        Err(_) => false, // Evaluation errors are treated as false.
    }
}

// ---------------------------------------------------------------------------
// Projection and aggregation
// ---------------------------------------------------------------------------

/// Project bindings to output columns.
fn project_rows(
    bindings: &[HashMap<String, Value>],
    outputs: &[OutputCol],
) -> Result<Vec<HashMap<String, Value>>> {
    let mut result = Vec::new();

    for binding in bindings {
        let mut row = HashMap::new();
        for output in outputs {
            // Output column name is the variable name.
            if let Some(val) = binding.get(&output.name) {
                row.insert(output.name.clone(), val.clone());
            } else {
                // Try to evaluate as expression (for future support).
                row.insert(output.name.clone(), Value::Null);
            }
        }
        result.push(row);
    }

    Ok(result)
}

/// Apply aggregations to bindings.
fn apply_aggregations(
    bindings: &[HashMap<String, Value>],
    outputs: &[OutputCol],
) -> Result<Vec<HashMap<String, Value>>> {
    // For now, support single aggregation queries like count(*).
    // Full GROUP BY support can be added later.

    let mut result_row = HashMap::new();

    for output in outputs {
        if let Some(agg) = output.aggregation {
            let value = compute_aggregation(agg, bindings, &output.name)?;
            result_row.insert(output.name.clone(), value);
        } else {
            // Non-aggregated column without GROUP BY - take first value if any.
            if let Some(first) = bindings.first() {
                if let Some(val) = first.get(&output.name) {
                    result_row.insert(output.name.clone(), val.clone());
                }
            }
        }
    }

    Ok(vec![result_row])
}

/// Compute a single aggregation value.
fn compute_aggregation(
    agg: Aggregation,
    bindings: &[HashMap<String, Value>],
    var_name: &str,
) -> Result<Value> {
    match agg {
        Aggregation::Count => {
            // Count non-null values.
            let count = bindings
                .iter()
                .filter(|b| {
                    b.get(var_name)
                        .map(|v| !v.is_null())
                        .unwrap_or(false)
                })
                .count();
            Ok(Value::Int(count as i64))
        }
        Aggregation::Sum => {
            let mut sum = 0.0_f64;
            let mut has_values = false;
            for b in bindings {
                if let Some(val) = b.get(var_name) {
                    if let Some(n) = val.to_f64() {
                        sum += n;
                        has_values = true;
                    }
                }
            }
            if has_values {
                Ok(Value::Float(sum))
            } else {
                Ok(Value::Null)
            }
        }
        Aggregation::Max => {
            let mut max: Option<&Value> = None;
            for b in bindings {
                if let Some(val) = b.get(var_name) {
                    if max.is_none() || val > max.unwrap() {
                        max = Some(val);
                    }
                }
            }
            Ok(max.cloned().unwrap_or(Value::Null))
        }
        Aggregation::Min => {
            let mut min: Option<&Value> = None;
            for b in bindings {
                if let Some(val) = b.get(var_name) {
                    if min.is_none() || val < min.unwrap() {
                        min = Some(val);
                    }
                }
            }
            Ok(min.cloned().unwrap_or(Value::Null))
        }
        Aggregation::Mean => {
            let mut sum = 0.0_f64;
            let mut count = 0_i64;
            for b in bindings {
                if let Some(val) = b.get(var_name) {
                    if let Some(n) = val.to_f64() {
                        sum += n;
                        count += 1;
                    }
                }
            }
            if count > 0 {
                Ok(Value::Float(sum / count as f64))
            } else {
                Ok(Value::Null)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Ordering and limit
// ---------------------------------------------------------------------------

/// Apply ordering to result rows.
fn apply_ordering(
    rows: &mut [HashMap<String, Value>],
    ordering: &[OrderSpec],
) -> Result<()> {
    if ordering.is_empty() {
        return Ok(());
    }

    rows.sort_by(|a, b| {
        for spec in ordering {
            let a_val = a.get(&spec.column);
            let b_val = b.get(&spec.column);

            let cmp = match (a_val, b_val) {
                (Some(av), Some(bv)) => av.cmp(bv),
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            };

            if cmp != std::cmp::Ordering::Equal {
                return if spec.descending { cmp.reverse() } else { cmp };
            }
        }
        std::cmp::Ordering::Equal
    });

    Ok(())
}

/// Apply limit to result rows.
fn apply_limit(
    rows: &mut Vec<HashMap<String, Value>>,
    limit: Option<&Expr>,
    params: &BTreeMap<String, Value>,
) -> Result<()> {
    let limit_val = match limit {
        Some(expr) => match eval_expr(expr, &HashMap::new(), params)? {
            Value::Int(n) if n >= 0 => n as usize,
            Value::Float(f) if f >= 0.0 => f as usize,
            _ => {
                return Err(error::EvalSnafu {
                    message: "limit must be a non-negative number".to_string(),
                }
                .build())
            }
        },
        None => return Ok(()),
    };

    if rows.len() > limit_val {
        rows.truncate(limit_val);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Deduplication
// ---------------------------------------------------------------------------

/// Deduplicate rows based on all variable bindings.
fn deduplicate_rows(rows: Vec<HashMap<String, Value>>) -> Vec<HashMap<String, Value>> {
    let mut seen: HashSet<Vec<(String, Value)>> = HashSet::new();
    let mut result = Vec::new();

    for row in rows {
        // Create a deterministic key from the row.
        let mut keys: Vec<(String, Value)> = row
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));

        if seen.insert(keys) {
            result.push(row);
        }
    }

    result
}
