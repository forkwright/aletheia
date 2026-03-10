//! Error types for the Datalog engine.
use snafu::Snafu;

/// Top-level error type for the mneme-engine public API.
///
/// Internal modules use `DbResult<T>` (see below). The public `Db` facade
/// converts internal errors to this type at the boundary.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// A database engine operation failed.
    #[snafu(display("{message}"))]
    Engine {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A running query was cancelled via poison/timeout.
    ///
    /// Replaces fragile string-matching on "killed before completion".
    /// Consumers can match this variant instead of parsing error messages.
    #[snafu(display("Running query was killed before completion"))]
    QueryKilled {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Internal error type replacing `miette::Box<dyn std::error::Error + Send + Sync>`.
///
/// All engine-internal modules use this for `?`-based error propagation.
/// At the public `Db` facade boundary, this is converted to `Error::Engine`.
pub(crate) type BoxErr = Box<dyn std::error::Error + Send + Sync + 'static>;
pub(crate) type DbResult<T> = std::result::Result<T, BoxErr>;

/// Ad-hoc string error, replaces `bail!("message")` at internal call sites.
#[derive(Debug)]
pub(crate) struct AdhocError(pub(crate) String);

impl std::fmt::Display for AdhocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AdhocError {}

/// Compatibility `bail!` macro for engine-internal error propagation.
///
/// Supports both string-format form (`bail!("msg")`, `bail!("fmt {}", val)`)
/// and struct form (`bail!(SomeErrorStruct { ... })`).
#[macro_export]
macro_rules! bail {
    // String literal with format args
    ($fmt:literal, $($arg:tt)+) => {
        return Err(Box::new($crate::engine::error::AdhocError(format!($fmt, $($arg)+)))
            as Box<dyn std::error::Error + Send + Sync + 'static>)
    };
    // String literal alone (optional trailing comma)
    ($msg:literal $(,)?) => {
        return Err(Box::new($crate::engine::error::AdhocError($msg.to_string()))
            as Box<dyn std::error::Error + Send + Sync + 'static>)
    };
    // Struct/enum expression form
    ($e:expr) => {
        return Err(Box::new($e) as Box<dyn std::error::Error + Send + Sync + 'static>)
    };
}

/// Compatibility `miette!` macro: creates a `BoxErr` from a format string or struct expression.
///
/// Creates a `BoxErr` from a format string or struct expression.
#[macro_export]
macro_rules! miette {
    ($fmt:literal, $($arg:tt)+) => {
        Box::new($crate::engine::error::AdhocError(format!($fmt, $($arg)+)))
            as Box<dyn std::error::Error + Send + Sync + 'static>
    };
    ($msg:literal $(,)?) => {
        Box::new($crate::engine::error::AdhocError($msg.to_string()))
            as Box<dyn std::error::Error + Send + Sync + 'static>
    };
    ($e:expr) => {
        Box::new($e) as Box<dyn std::error::Error + Send + Sync + 'static>
    };
}

/// Compatibility `ensure!` macro for engine-internal error propagation.
#[macro_export]
macro_rules! ensure {
    // Format string with args (optional trailing comma)
    ($cond:expr, $fmt:literal, $($arg:tt)+) => {
        if !($cond) {
            $crate::bail!($fmt, $($arg)+)
        }
    };
    // String literal alone (optional trailing comma)
    ($cond:expr, $msg:literal $(,)?) => {
        if !($cond) {
            $crate::bail!($msg)
        }
    };
    // Struct/enum expression form
    ($cond:expr, $e:expr) => {
        if !($cond) {
            $crate::bail!($e)
        }
    };
}
