//! Matrix-specific error types.
//!
//! Errors surface from three sources: HTTP transport to the homeserver,
//! MessagePack (de)serialization against the fjall crypto store, and fjall
//! storage itself. Keep them separate so callers can distinguish a failing
//! probe (HTTP) from a corrupted store (serialize/fjall).

use snafu::Snafu;

/// Errors from Matrix transport and the fjall-backed crypto store.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, url) are self-documenting via display format"
)]
pub enum Error {
    /// HTTP transport error talking to the Matrix homeserver.
    #[snafu(display("matrix HTTP error: {source}"))]
    Http {
        source: reqwest::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The homeserver URL could not be parsed or was not an absolute `http(s)` URL.
    #[snafu(display("invalid matrix homeserver URL '{url}': {message}"))]
    InvalidUrl {
        url: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fjall key-value store open/partition/read/write failure.
    #[snafu(display("matrix crypto store: {message}"))]
    Store {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// rmp-serde encode/decode failure against the crypto store.
    #[snafu(display("matrix crypto store codec: {message}"))]
    Codec {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with Matrix's [`Error`] type.
pub(crate) type Result<T> = std::result::Result<T, Error>;
