//! Error types for the Datalog engine runtime.
//!
//! `RuntimeError` is the snafu-derived error enum for all runtime operations.
//! Each variant carries structured context (names, messages) and an implicit
//! `snafu::Location` for source-location tracking. At the crate boundary,
//! these are wrapped into `InternalError::Runtime` and then converted to the
//! public `Error` type.
use snafu::Snafu;

use crate::runtime::poison::QueryCancellationReason;

/// Structured error type for the engine runtime module.
///
/// Typed variants that carry context and can be matched by callers.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub(crate) enum RuntimeError {
    /// A running query was cancelled (poison/timeout).
    #[snafu(display("Running query is killed before completion"))]
    QueryKilled {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A running query was cancelled by the query-budget model.
    #[snafu(display("query cancelled: reason={reason}, observed={observed:?}, limit={limit:?}"))]
    QueryCancelled {
        reason: QueryCancellationReason,
        observed: Option<u64>,
        limit: Option<u64>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// An operation was attempted in read-only mode.
    #[snafu(display("{operation} requires write access"))]
    ReadOnlyViolation {
        operation: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Insufficient access level for stored relation operation.
    #[snafu(display("Insufficient access level for {operation}"))]
    InsufficientAccess {
        operation: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Relation not found in the database.
    #[snafu(display("Stored relation not found: '{name}'"))]
    RelationNotFound {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A relation with the given name already exists.
    #[snafu(display("Cannot create relation '{name}' as one with the same name already exists"))]
    RelationAlreadyExists {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// An index with the given name already exists on the relation.
    #[snafu(display("Index '{index_name}' for relation '{relation_name}' already exists"))]
    IndexAlreadyExists {
        index_name: String,
        relation_name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Index not found for the given relation.
    #[snafu(display("Index not found for relation '{relation_name}'"))]
    IndexNotFound {
        relation_name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Storage version mismatch or unversioned storage.
    #[snafu(display("{message}"))]
    StorageVersion {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Query assertion failed (expected results vs actual).
    #[snafu(display("{message}"))]
    AssertionFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// An unsupported operation was attempted (removed feature, etc.).
    #[snafu(display("{operation} is not supported: {reason}"))]
    Unsupported {
        operation: String,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A generic invalid operation with context.
    #[snafu(display("{op}: {reason}"))]
    InvalidOperation {
        op: String,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Serialization of internal data failed (msgpack/cbor).
    #[snafu(display("serialization failed: {message}"))]
    Serialization {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
