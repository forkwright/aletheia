//! Thesauros-specific errors.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors from domain pack loading.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// Pack directory does not exist.
    #[snafu(display("pack not found: {}", path.display()))]
    PackNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Manifest file (pack.yaml) not found in pack directory.
    #[snafu(display("manifest not found: {}", path.display()))]
    ManifestNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to read a file.
    #[snafu(display("failed to read {}", path.display()))]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to parse YAML manifest.
    #[snafu(display("failed to parse manifest at {}: {reason}", path.display()))]
    ParseManifest {
        path: PathBuf,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A context file referenced by the manifest was not found.
    #[snafu(display("context file not found: {}", path.display()))]
    ContextFileNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use snafu::ResultExt;

    #[test]
    fn pack_not_found_display() {
        let err = Error::PackNotFound {
            path: PathBuf::from("/missing/pack"),
            location: snafu::Location::new("test", 0, 0),
        };
        assert!(err.to_string().contains("/missing/pack"));
    }

    #[test]
    fn read_file_chains_source() {
        let result: Result<Vec<u8>> =
            std::fs::read("/nonexistent/path").context(ReadFileSnafu {
                path: PathBuf::from("/nonexistent/path"),
            });
        let err = result.unwrap_err();
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn parse_manifest_display() {
        let err = Error::ParseManifest {
            path: PathBuf::from("/some/pack.yaml"),
            reason: "expected mapping".to_owned(),
            location: snafu::Location::new("test", 0, 0),
        };
        let msg = err.to_string();
        assert!(msg.contains("pack.yaml"));
        assert!(msg.contains("expected mapping"));
    }

    #[test]
    fn errors_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }
}
