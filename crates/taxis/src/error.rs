//! Taxis-specific errors.
//!
//! Covers instance root discovery, configuration file reading, and
//! TOML/JSON/Figment parsing failures during the configuration cascade.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors from configuration and path resolution.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, path, reason) are self-documenting via display format"
)]
pub enum Error {
    /// The instance root directory does not exist.
    #[snafu(display("instance root not found: {}", path.display()))]
    InstanceNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A required configuration file was not found.
    #[snafu(display("config not found: {}", path.display()))]
    ConfigNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to read a configuration file.
    #[snafu(display("failed to read config from {}", path.display()))]
    ReadConfig {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to parse JSON configuration.
    #[snafu(display("failed to parse JSON config at {}", path.display()))]
    ParseJson {
        path: PathBuf,
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Figment configuration error.
    #[snafu(display("configuration error: {source}"))]
    Figment {
        source: figment::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to serialize configuration to TOML.
    #[snafu(display("failed to serialize config to TOML: {reason}"))]
    SerializeToml {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to write configuration to disk.
    #[snafu(display("failed to write config to {}", path.display()))]
    WriteConfig {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The instance root directory does not exist (startup validation).
    #[snafu(display(
        "instance root not found: {}\n  help: set ALETHEIA_ROOT or run `aletheia init`",
        path.display()
    ))]
    InstanceRootNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A required subdirectory (config/ or data/) is missing from the instance root.
    #[snafu(display(
        "required directory missing: {}\n  help: run `aletheia init` to create the instance layout",
        path.display()
    ))]
    RequiredDirMissing {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The data directory is not writable.
    #[snafu(display(
        "data directory is not writable: {}\n  help: check permissions or run `aletheia init`",
        path.display()
    ))]
    NotWritable {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A workspace path from agent config does not resolve to a directory.
    #[snafu(display(
        "agent workspace path does not exist: {}\n  help: create the directory or update the workspace path in config",
        path.display()
    ))]
    WorkspacePathInvalid {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The primary key file is invalid (wrong length, bad hex).
    #[snafu(display("invalid primary key at {}: {reason}", path.display()))]
    InvalidPrimaryKey {
        path: PathBuf,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The primary key file already exists.
    #[snafu(display(
        "primary key already exists at {}\n  help: delete the file first if you want to regenerate",
        path.display()
    ))]
    PrimaryKeyExists {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Encryption operation failed.
    #[snafu(display("encryption failed: {reason}"))]
    Encrypt {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Decryption operation failed.
    #[snafu(display("decryption failed: {reason}"))]
    Decrypt {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A `${VAR:?message}` expression in the config resolved to an unset variable.
    ///
    /// Emitted when the TOML config contains `${VAR:?some message}` and `VAR`
    /// is not present in the environment. Startup aborts with the user-supplied message.
    #[snafu(display("required env var `{var}` is not set: {message}"))]
    EnvVarRequired {
        var: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// An unterminated env-var expression was found in the configuration file.
    ///
    /// Emitted when a `${` opener has no matching `}`.
    #[snafu(display("unterminated env-var expression in config file near: {}", excerpt))]
    EnvVarUnterminated {
        excerpt: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;
