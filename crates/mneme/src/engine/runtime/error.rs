//! Error types for the Datalog engine runtime.
use snafu::Snafu;

/// Structured error type for the engine runtime module.
///
/// Replaces ad-hoc `bail!("message")` strings with typed variants that carry
/// context and can be matched by callers. Functions in this module still return
/// `DbResult<T>` — `RuntimeError` auto-converts to `BoxErr` via the std blanket
/// `From<E: Error + Send + Sync + 'static>` impl.
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
}

/// Bridge: allows `RuntimeError` to be used in functions returning `DbResult<T>`
/// via the `?` operator. The std blanket `From<E: Error + Send + Sync>` impl
/// already handles this, but we provide an explicit method for clarity.
impl RuntimeError {
    pub(crate) fn into_box_err(self) -> crate::engine::error::BoxErr {
        Box::new(self)
    }
}
