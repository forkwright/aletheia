//! Error types for the Datalog engine.
use snafu::Snafu;

/// Public error type for consumers of the engine.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (message, source, location) are self-documenting via display format"
)]
pub enum Error {
    /// A database engine operation failed.
    #[snafu(display("{message}"))]
    Engine {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A running query was cancelled via poison/timeout.
    #[snafu(display("Running query was killed before completion"))]
    QueryKilled {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A parse error (query syntax).
    #[snafu(display("parse error: {source}"))]
    Parse {
        source: crate::parse::error::ParseError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A storage error.
    #[snafu(display("storage error: {source}"))]
    Storage {
        source: crate::storage::error::StorageError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result alias using the engine's public [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

/// Internal error enum composing all module error types.
///
/// Used for engine-internal error propagation. At the public boundary,
/// `InternalError` is converted to `Error` via `convert_internal()`.
///
/// Each variant uses `#[snafu(context(false))]` so that `From<ModuleError>`
/// impls are generated, allowing `?`-based propagation from any module result.
#[derive(Debug, Snafu)]
pub(crate) enum InternalError {
    #[snafu(display("{source}"))]
    #[snafu(context(false))]
    Data {
        source: crate::data::error::DataError,
    },

    #[snafu(display("{source}"))]
    #[snafu(context(false))]
    Parse {
        source: crate::parse::error::ParseError,
    },

    #[snafu(display("{source}"))]
    #[snafu(context(false))]
    Query {
        source: crate::query::error::QueryError,
    },

    #[snafu(display("{source}"))]
    #[snafu(context(false))]
    Runtime {
        source: crate::runtime::error::RuntimeError,
    },

    #[snafu(display("{source}"))]
    #[snafu(context(false))]
    Storage {
        source: crate::storage::error::StorageError,
    },

    #[snafu(display("{source}"))]
    #[snafu(context(false))]
    Fts { source: crate::fts::error::FtsError },

    #[snafu(display("{source}"))]
    #[snafu(context(false))]
    FixedRule {
        source: crate::fixed_rule::error::FixedRuleError,
    },
}

pub(crate) type InternalResult<T> = std::result::Result<T, InternalError>;

impl From<crate::data::program::FixedRuleOptionNotFoundError> for InternalError {
    fn from(e: crate::data::program::FixedRuleOptionNotFoundError) -> Self {
        InternalError::FixedRule {
            source: crate::fixed_rule::error::ConfigSnafu {
                rule: e.rule_name,
                param: e.name,
                message: "required option not found",
            }
            .build(),
        }
    }
}

impl From<crate::data::program::WrongFixedRuleOptionError> for InternalError {
    fn from(e: crate::data::program::WrongFixedRuleOptionError) -> Self {
        InternalError::FixedRule {
            source: crate::fixed_rule::error::ConfigSnafu {
                rule: e.rule_name,
                param: e.name,
                message: e.help,
            }
            .build(),
        }
    }
}

impl From<crate::fixed_rule::BadExprValueError> for InternalError {
    fn from(e: crate::fixed_rule::BadExprValueError) -> Self {
        InternalError::FixedRule {
            source: crate::fixed_rule::error::InvalidInputSnafu {
                rule: "expression",
                message: format!("bad expression value {:?}: {}", e.0, e.1),
            }
            .build(),
        }
    }
}

impl From<crate::data::program::NoEntryError> for InternalError {
    fn from(e: crate::data::program::NoEntryError) -> Self {
        InternalError::Data {
            source: crate::data::error::ProgramConstraintSnafu {
                message: e.to_string(),
            }
            .build(),
        }
    }
}

impl From<crate::runtime::relation::StoredRelArityMismatch> for InternalError {
    fn from(e: crate::runtime::relation::StoredRelArityMismatch) -> Self {
        InternalError::Runtime {
            source: crate::runtime::error::InvalidOperationSnafu {
                op: "stored relation",
                reason: e.to_string(),
            }
            .build(),
        }
    }
}

impl From<crate::runtime::db::ProcessKilled> for InternalError {
    fn from(_: crate::runtime::db::ProcessKilled) -> Self {
        InternalError::Runtime {
            source: crate::runtime::error::QueryKilledSnafu.build(),
        }
    }
}

impl From<crate::fixed_rule::NodeNotFoundError> for InternalError {
    fn from(e: crate::fixed_rule::NodeNotFoundError) -> Self {
        InternalError::FixedRule {
            source: crate::fixed_rule::error::InvalidInputSnafu {
                rule: "graph",
                message: format!("required node with key {:?} not found", e.missing),
            }
            .build(),
        }
    }
}

impl From<crate::fixed_rule::BadEdgeWeightError> for InternalError {
    fn from(e: crate::fixed_rule::BadEdgeWeightError) -> Self {
        InternalError::FixedRule {
            source: crate::fixed_rule::error::InvalidInputSnafu {
                rule: "graph",
                message: format!("value {:?} cannot be interpreted as edge weight", e.val),
            }
            .build(),
        }
    }
}
