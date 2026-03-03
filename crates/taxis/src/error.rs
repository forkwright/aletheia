//! Taxis-specific errors.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors from configuration and path resolution.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
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

    /// Failed to parse YAML configuration.
    #[snafu(display("failed to parse YAML config at {}: {reason}", path.display()))]
    ParseYaml {
        path: PathBuf,
        reason: String,
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
}

pub type Result<T> = std::result::Result<T, Error>;
