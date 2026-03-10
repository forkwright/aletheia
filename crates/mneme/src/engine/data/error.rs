//! Error types for the Datalog engine data layer.
use snafu::Snafu;

/// Errors from the data layer (types, functions, expressions, encoding).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub(crate) enum DataError {
    /// An operation received a value of the wrong type.
    #[snafu(display("'{op}' requires {expected}"))]
    TypeMismatch {
        op: String,
        expected: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Comparison between incompatible types.
    #[snafu(display("comparison can only be done between the same datatypes, got {left} and {right}"))]
    ComparisonTypeMismatch {
        left: String,
        right: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Vector length mismatch in a binary operation.
    #[snafu(display("'{op}' requires two vectors of the same length"))]
    VectorLengthMismatch {
        op: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Byte operand length mismatch in a bitwise operation.
    #[snafu(display("operands of '{op}' must have the same lengths"))]
    ByteLengthMismatch {
        op: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Division by zero.
    #[snafu(display("'{op}' requires non-zero divisor"))]
    DivisionByZero {
        op: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Index out of bounds.
    #[snafu(display("index {index} out of bound"))]
    IndexOutOfBounds {
        index: i64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Value domain violation (out of range, invalid format, etc.).
    #[snafu(display("{message}"))]
    InvalidValue {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Assertion failure.
    #[snafu(display("assertion failed: {message}"))]
    AssertionFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// String could not be parsed as the target type.
    #[snafu(display("The string cannot be interpreted as {target}"))]
    ParseFailed {
        target: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Regex compilation failed.
    #[snafu(display("The string cannot be interpreted as regex: {source}"))]
    InvalidRegex {
        source: regex::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Encoding or decoding failure.
    #[snafu(display("{message}"))]
    EncodingFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON path operation error.
    #[snafu(display("{message}"))]
    JsonPath {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A timestamp or timezone could not be parsed.
    #[snafu(display("{message}"))]
    BadTime {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// An unbound or missing variable in expression evaluation.
    #[snafu(display("{message}"))]
    UnboundVariable {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No implementation found for an operation.
    #[snafu(display("{message}"))]
    NotImplemented {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A named field or column is missing or invalid.
    #[snafu(display("{message}"))]
    FieldNotFound {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Data coercion failed during relation input processing.
    #[snafu(display("{message}"))]
    CoercionFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Symbol is not valid for the requested use.
    #[snafu(display("{message}"))]
    InvalidSymbol {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Input relation or fixed rule constraint violation.
    #[snafu(display("{message}"))]
    ProgramConstraint {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Access level insufficient.
    #[snafu(display("{message}"))]
    InsufficientAccess {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization/deserialization error.
    #[snafu(display("JSON error: {source}"))]
    Json {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub(crate) type DataResult<T> = std::result::Result<T, DataError>;

impl DataError {
    /// Convert into the engine's `BoxErr` for cross-module boundaries.
    ///
    /// This bridge exists during the migration period. It will be removed
    /// once all engine modules use typed errors.
    pub(crate) fn into_box_err(self) -> crate::engine::error::BoxErr {
        Box::new(self)
    }
}

/// Extension trait to convert `DataResult<T>` into `DbResult<T>` at module boundaries.
pub(crate) trait IntoDbResult<T> {
    fn into_db_result(self) -> crate::engine::error::DbResult<T>;
}

impl<T> IntoDbResult<T> for DataResult<T> {
    fn into_db_result(self) -> crate::engine::error::DbResult<T> {
        self.map_err(|e| e.into_box_err())
    }
}
