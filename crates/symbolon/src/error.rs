//! Symbolon-specific errors.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors from authentication and authorization operations.
// kanon:ignore RUST/no-debug-derive-on-public-types -- WHY: error enum; Debug is required by std::error::Error and Result ergonomics. Variants carry only error messages, RBAC identity metadata (role, action, entity, id), paths, and source errors — never plaintext credentials or tokens.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "Snafu variant fields are documented by variant prose and display strings"
)]
#[non_exhaustive]
pub enum Error {
    /// JWT token is malformed or has an invalid signature.
    #[snafu(display("invalid token: {message}"))]
    InvalidToken {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JWT token has expired.
    #[snafu(display("token expired"))]
    ExpiredToken {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Username or password is incorrect.
    #[snafu(display("invalid credentials"))]
    InvalidCredentials {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The authenticated principal lacks permission for the requested action.
    #[snafu(display("permission denied: {role} cannot {action}"))]
    PermissionDenied {
        action: String,
        role: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Password hashing or verification failed.
    #[snafu(display("hash error: {message}"))]
    Hash {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JWT encoding failed.
    #[snafu(display("token encode error: {message}"))]
    TokenEncode {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JWT decoding failed.
    #[snafu(display("token decode error: {message}"))]
    TokenDecode {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// API key format is invalid.
    #[snafu(display("invalid API key format"))]
    InvalidApiKey {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Managed provider credential secret is malformed.
    #[snafu(display("invalid credential secret: {reason}"))]
    InvalidCredentialSecret {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Entity not found.
    #[snafu(display("{entity} not found: {id}"))]
    NotFound {
        entity: String,
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Duplicate entity.
    #[snafu(display("duplicate {entity}: {id}"))]
    Duplicate {
        entity: String,
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Removing the last usable primary credential for a provider.
    #[snafu(display("cannot remove the last primary credential for provider '{provider}'"))]
    RemoveLastPrimary {
        provider: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JWT signing key is the insecure default placeholder.
    #[snafu(display(
        "insecure JWT signing key: default placeholder active with auth mode '{auth_mode}'. Set auth.jwt_secret in config or the ALETHEIA_JWT_SECRET env var. Generate one with: openssl rand -hex 32"
    ))]
    InsecureKey {
        auth_mode: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Storage backend operation failed.
    ///
    /// Used by the fjall backend for LSM-tree and JSON encoding errors.
    #[snafu(display("storage error: {message}"))]
    Storage {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Filesystem I/O error.
    ///
    /// Used by the fjall backend when creating the store directory.
    #[snafu(display("I/O error at {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with symbolon's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;
