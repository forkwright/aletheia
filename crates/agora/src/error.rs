//! Error types for the agora crate.

use snafu::Snafu;

/// Errors from channel registry and provider operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// Requested channel does not exist in the registry.
    #[snafu(display("unknown channel: {id}"))]
    UnknownChannel {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A channel with this ID is already registered.
    #[snafu(display("duplicate channel: {id}"))]
    DuplicateChannel {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with agora's [`Error`] type.
pub(crate) type Result<T> = std::result::Result<T, Error>;
