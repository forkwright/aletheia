//! Symbolon-specific errors.

use snafu::Snafu;

/// Errors from authentication and authorization operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
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

    /// `SQLite` operation failed.
    #[snafu(display("database error: {source}"))]
    Database {
        source: rusqlite::Error,
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

    /// JWT signing key is the insecure default placeholder.
    #[snafu(display(
        "insecure JWT signing key: default placeholder active with auth mode '{auth_mode}'. Set auth.jwt_secret in config or the ALETHEIA_JWT_SECRET env var. Generate one with: openssl rand -hex 32"
    ))]
    InsecureKey {
        auth_mode: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Auth database schema version table is corrupted or unreadable.
    #[snafu(display("auth database schema is corrupted: {source}"))]
    SchemaCorrupted {
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
