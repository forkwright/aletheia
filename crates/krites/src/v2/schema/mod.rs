//! Schema definitions for stored relations.
//!
//! A schema defines the column layout of a stored relation: column names,
//! types, key/value split, and optional defaults. The engine validates all
//! writes against the schema and rejects type mismatches.
//!
//! WHY: CozoDB's schema was implicit (defined by `:create` DDL at runtime).
//! Making schemas explicit enables compile-time validation, better error
//! messages, and safer migrations.

pub mod relation;

pub use relation::{ColumnDef, ColumnType, RelationSchema, VectorDtype};
