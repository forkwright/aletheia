//! Query and mutation evaluator for krites v2.
//!
//! The evaluator executes parsed AST statements against a storage backend.
//! Uses semi-naive bottom-up evaluation for Datalog rules.

mod expr;
mod query;
mod write;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};

use crate::v2::error::Result;
use crate::v2::parse::ast::Statement;
use crate::v2::rows::Rows;
use crate::v2::schema::RelationSchema;
use crate::v2::storage::Storage;
use crate::v2::value::Value;

pub use expr::eval_expr;

// ---------------------------------------------------------------------------
// EvalResult
// ---------------------------------------------------------------------------

/// Result of evaluating a statement.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum EvalResult {
    /// Query result with rows.
    Rows(Rows),
    /// Mutation result (insert, update, DDL).
    Mutation {
        /// Relation affected.
        relation: String,
        /// Number of rows affected.
        rows_affected: usize,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Evaluate a parsed statement.
///
/// # Arguments
///
/// * `statement` - The parsed AST statement to evaluate.
/// * `params` - Runtime parameter values (e.g., `$id` → Value).
/// * `storage` - The storage backend to read from and write to.
/// * `schemas` - Map of relation names to their schemas.
///
/// # Errors
///
/// Returns an error if evaluation fails (e.g., undefined relation, type mismatch).
pub fn evaluate<S>(
    statement: &Statement,
    params: &BTreeMap<String, Value>,
    storage: &S,
    schemas: &HashMap<String, RelationSchema>,
) -> Result<EvalResult>
where
    S: Storage,
{
    match statement {
        Statement::Query(query) => {
            let rows = query::evaluate_query(query, params, storage, schemas)?;
            Ok(EvalResult::Rows(rows))
        }
        Statement::Put { relation, rows } => {
            // Look up the schema for this relation.
            let schema = schemas.get(relation).ok_or_else(|| {
                crate::v2::error::EvalSnafu {
                    message: format!("unknown relation for put: {relation}"),
                }
                .build()
            })?;
            let count = write::evaluate_put(relation, rows, params, storage, schema)?;
            Ok(EvalResult::Mutation {
                relation: relation.clone(),
                rows_affected: count,
            })
        }
        Statement::Create { relation, schema } => {
            let new_schema = write::evaluate_create(relation, schema, storage)?;
            Ok(EvalResult::Mutation {
                relation: new_schema.name,
                rows_affected: 0,
            })
        }
        Statement::Replace { relation, schema } => {
            let new_schema = write::evaluate_replace(relation, schema, storage)?;
            Ok(EvalResult::Mutation {
                relation: new_schema.name,
                rows_affected: 0,
            })
        }
        Statement::Remove { relation } => {
            write::evaluate_remove(relation, storage)?;
            Ok(EvalResult::Mutation {
                relation: relation.clone(),
                rows_affected: 0,
            })
        }
        // Index and fixed-rule operations are deferred to separate PRs.
        Statement::FtsCreate { relation, .. } => Ok(EvalResult::Mutation {
            relation: relation.clone(),
            rows_affected: 0,
        }),
        Statement::HnswCreate { relation, .. } => Ok(EvalResult::Mutation {
            relation: relation.clone(),
            rows_affected: 0,
        }),
    }
}

/// Load schemas from storage.
///
/// Reads all schemas stored in the `_schemas` relation and returns
/// them as a map from relation name to schema.
pub fn load_schemas<S>(storage: &S) -> Result<HashMap<String, RelationSchema>>
where
    S: Storage,
{
    use crate::v2::storage::StorageTx;

    let mut schemas = HashMap::new();
    let tx = storage.begin(false)?;
    
    let prefix = b"_schemas:".to_vec();
    let results = tx.scan_prefix(&prefix)?;
    
    for (_, value) in results {
        let schema: RelationSchema = serde_json::from_slice(&value).map_err(|e| {
            crate::v2::error::StorageSnafu {
                message: format!("failed to deserialize schema: {e}"),
            }
            .build()
        })?;
        schemas.insert(schema.name.clone(), schema);
    }
    
    tx.rollback()?;
    Ok(schemas)
}
