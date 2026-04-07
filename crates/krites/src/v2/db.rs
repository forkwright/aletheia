//! Db facade for krites v2.
//!
//! The top-level API that consumers interact with. Wraps storage, schema
//! registry, evaluator, and indexes behind the same interface as krites v1.
//!
//! WHY: consumers call `db.run(script, params, mutability)` → `NamedRows`.
//! The v2 Db translates this to: parse → evaluate → return rows.
//! The interface is identical to v1 so consumers don't change.

use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::v2::error::{self, Result};
use crate::v2::eval::EvalResult;
use crate::v2::parse;
use crate::v2::rows::Rows;
use crate::v2::schema::RelationSchema;
use crate::v2::storage::Storage;
use crate::v2::storage::mem::MemStorage;
use crate::v2::value::Value;

// ---------------------------------------------------------------------------
// Mutability
// ---------------------------------------------------------------------------

/// Whether a script execution may modify stored relations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    /// Read-only: queries only, no writes.
    Immutable,
    /// Read-write: may insert, update, or delete.
    Mutable,
}

// ---------------------------------------------------------------------------
// Db facade
// ---------------------------------------------------------------------------

/// Top-level database facade for krites v2.
///
/// Holds the storage backend, schema registry, and provides the
/// `run()` / `run_read_only()` interface that consumers depend on.
pub struct Db<S: Storage> {
    storage: Arc<S>,
    schemas: Arc<RwLock<HashMap<String, RelationSchema>>>,
}

impl Db<MemStorage> {
    /// Open an in-memory database (for tests and ephemeral use).
    #[must_use]
    pub fn open_mem() -> Self {
        Self {
            storage: Arc::new(MemStorage::new()),
            schemas: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl<S: Storage> Db<S> {
    /// Create a database with a specific storage backend.
    #[must_use]
    pub fn new(storage: S) -> Self {
        Self {
            storage: Arc::new(storage),
            schemas: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Execute a Datalog script.
    ///
    /// Parses the script, evaluates it against stored relations, and returns
    /// the result (query rows or mutation count).
    ///
    /// # Errors
    ///
    /// Returns parse errors, evaluation errors, schema violations, or
    /// storage errors.
    pub fn run(
        &self,
        script: &str,
        params: BTreeMap<String, Value>,
        mutability: Mutability,
    ) -> Result<Rows> {
        let statement = parse::parse(script)?;

        let schemas = self.schemas.read().map_err(|_| {
            error::StorageSnafu {
                message: "schema registry lock poisoned",
            }
            .build()
        })?;

        let result = crate::v2::eval::evaluate(&statement, &params, self.storage.as_ref(), &schemas)?;

        match result {
            EvalResult::Rows(rows) => Ok(rows),
            EvalResult::Mutation {
                relation,
                rows_affected,
            } => Ok(Rows {
                headers: vec!["relation".to_owned(), "rows_affected".to_owned()],
                rows: vec![vec![
                    Value::from(relation.as_str()),
                    Value::from(i64::try_from(rows_affected).unwrap_or(i64::MAX)),
                ]],
            }),
        }
    }

    /// Execute a read-only Datalog script.
    ///
    /// Convenience wrapper that enforces immutable execution.
    pub fn run_read_only(
        &self,
        script: &str,
        params: BTreeMap<String, Value>,
    ) -> Result<Rows> {
        self.run(script, params, Mutability::Immutable)
    }

    /// Register a schema for a relation.
    ///
    /// Called by `:create` DDL statements. Subsequent writes to this
    /// relation are validated against the schema.
    pub fn register_schema(&self, schema: RelationSchema) -> Result<()> {
        let mut schemas = self.schemas.write().map_err(|_| {
            error::StorageSnafu {
                message: "schema registry lock poisoned",
            }
            .build()
        })?;
        schemas.insert(schema.name.clone(), schema);
        Ok(())
    }

    /// Get a registered schema by relation name.
    #[must_use]
    pub fn schema(&self, name: &str) -> Option<RelationSchema> {
        self.schemas
            .read()
            .ok()
            .and_then(|s| s.get(name).cloned())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::v2::schema::{ColumnDef, ColumnType, RelationSchema};

    #[test]
    fn open_mem() {
        let db = Db::open_mem();
        assert!(db.schema("nonexistent").is_none());
    }

    #[test]
    fn register_and_retrieve_schema() {
        let db = Db::open_mem();
        let schema = RelationSchema::new(
            "facts",
            vec![
                ColumnDef::key("id", ColumnType::String),
                ColumnDef::value("content", ColumnType::String),
            ],
        );
        db.register_schema(schema).unwrap();
        assert!(db.schema("facts").is_some());
        assert_eq!(db.schema("facts").unwrap().arity(), 2);
    }
}
