//! Error types for the agora crate.

use snafu::Snafu;

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

    /// Channel send operation failed.
    #[snafu(display("send failed on channel {channel}: {message}"))]
    Send {
        channel: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Signal-specific error.
    #[snafu(display("signal error: {source}"))]
    Signal {
        source: crate::semeion::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
