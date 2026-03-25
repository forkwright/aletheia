//! Crate-level error type for the aletheia binary.
//!
//! WHY: The project standard restricts `anyhow` to `main.rs` only. Command
//! handlers and internal modules use this snafu-based Whatever type, which
//! provides ergonomic error wrapping via `whatever_context()` and `whatever!()`
//! while keeping typed errors available for future extraction into library crates.

use snafu::prelude::*;

#[derive(Debug, Snafu)]
#[snafu(whatever, display("{message}"))]
pub(crate) struct Error {
    message: String,
    #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, Some)))]
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl Error {
    /// Create an error with a message and no source.
    ///
    /// WHY: Avoids requiring `snafu::FromString` in scope at every call site.
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }
}

pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;
