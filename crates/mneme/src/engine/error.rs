// Public and internal error types for mneme-engine.
use snafu::Snafu;

/// Top-level error type for the mneme-engine public API.
///
/// Consumers match on `QueryKilled` for cancellation handling; all other
/// engine failures surface through `Engine`.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    #[snafu(display("{message}"))]
    Engine {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("Running query was killed before completion"))]
    QueryKilled {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Internal error type replacing `BoxErr`.
///
/// All engine modules propagate errors through this enum. Per-module error
/// enums feed in via `#[snafu(transparent)]` which auto-generates `From` impls.
/// At the public `Db` facade, `EngineError` converts to `Error` via `From`.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub(crate) enum EngineError {
    #[snafu(display("{message}"))]
    Internal {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("Running query was killed before completion"))]
    ProcessKilled {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(transparent)]
    Storage {
        source: crate::engine::storage::error::StorageError,
    },

    #[snafu(transparent)]
    Data {
        source: crate::engine::data::error::DataError,
    },

    #[snafu(transparent)]
    Fts {
        source: crate::engine::fts::error::FtsError,
    },

    #[snafu(transparent)]
    Parse {
        source: crate::engine::parse::error::CozoParseError,
    },

    #[snafu(transparent)]
    Query {
        source: crate::engine::query::error::QueryError,
    },

    #[snafu(transparent)]
    Runtime {
        source: crate::engine::runtime::error::RuntimeError,
    },

    #[snafu(transparent)]
    FixedRule {
        source: crate::engine::fixed_rule::error::FixedRuleError,
    },
}

impl From<EngineError> for Error {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::ProcessKilled { .. } => QueryKilledSnafu.build(),
            other => EngineSnafu {
                message: other.to_string(),
            }
            .build(),
        }
    }
}

pub(crate) type DbResult<T> = std::result::Result<T, EngineError>;

impl EngineError {
    #[track_caller]
    pub(crate) fn from_display(e: impl std::fmt::Display) -> Self {
        EngineError::Internal {
            message: e.to_string(),
            location: snafu::Location::default(),
        }
    }
}

/// Compatibility `bail!` macro — produces `EngineError::Internal` for string forms.
#[macro_export]
macro_rules! bail {
    ($fmt:literal, $($arg:tt)+) => {
        return Err($crate::engine::error::EngineError::Internal {
            message: format!($fmt, $($arg)+),
            location: snafu::Location::default(),
        })
    };
    ($msg:literal $(,)?) => {
        return Err($crate::engine::error::EngineError::Internal {
            message: $msg.to_string(),
            location: snafu::Location::default(),
        })
    };
    ($e:expr) => {
        return Err($crate::engine::error::EngineError::from_display($e))
    };
}

/// Compatibility `miette!` macro — creates an `EngineError` value (no return).
#[macro_export]
macro_rules! miette {
    ($fmt:literal, $($arg:tt)+) => {
        $crate::engine::error::EngineError::Internal {
            message: format!($fmt, $($arg)+),
            location: snafu::Location::default(),
        }
    };
    ($msg:literal $(,)?) => {
        $crate::engine::error::EngineError::Internal {
            message: $msg.to_string(),
            location: snafu::Location::default(),
        }
    };
    ($e:expr) => {
        $crate::engine::error::EngineError::from_display($e)
    };
}

/// Compatibility `ensure!` macro — conditional bail.
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $fmt:literal, $($arg:tt)+) => {
        if !($cond) {
            $crate::bail!($fmt, $($arg)+)
        }
    };
    ($cond:expr, $msg:literal $(,)?) => {
        if !($cond) {
            $crate::bail!($msg)
        }
    };
    ($cond:expr, $e:expr) => {
        if !($cond) {
            $crate::bail!($e)
        }
    };
}
