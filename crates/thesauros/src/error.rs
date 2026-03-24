//! Thesauros-specific errors.

use std::path::PathBuf;

use snafu::Snafu;

/// Errors from domain pack loading.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (path, source, location, reason, type_name, tool_name, pack_name, name, pack) are self-documenting via display format"
)]
pub enum Error {
    /// Pack directory does not exist.
    #[snafu(display("pack not found: {}", path.display()))]
    PackNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Manifest file (pack.toml) not found in pack directory.
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

    /// Failed to parse TOML manifest.
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

    /// Context file path escapes the pack root directory.
    #[snafu(display("context file path escapes pack root: {}", path.display()))]
    ContextFileEscape {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool command script not found at declared path.
    #[snafu(display("tool command not found: {}", path.display()))]
    ToolCommandNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool command path resolves outside the pack root.
    #[snafu(display("tool command escapes pack root: {}", path.display()))]
    ToolCommandEscape {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Unknown property type in a tool's input schema.
    #[snafu(display("unknown property type '{type_name}' in tool '{tool_name}'"))]
    UnknownPropertyType {
        type_name: String,
        tool_name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to register a pack tool in the registry.
    #[snafu(display("failed to register tool '{tool_name}' from pack '{pack_name}': {reason}"))]
    ToolRegistration {
        tool_name: String,
        pack_name: String,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Pack name fails validation (must be 1--64 alphanumeric/hyphen characters).
    #[snafu(display(
        "invalid pack name '{name}': must be 1-64 characters, alphanumeric and hyphens only"
    ))]
    InvalidPackName {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Pack version is an empty string.
    #[snafu(display("pack '{pack}' has an empty version string"))]
    InvalidPackVersion {
        pack: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for results with [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use snafu::ResultExt;

    use super::*;

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
        #[expect(
            clippy::disallowed_methods,
            reason = "thesauros pack loader reads binary assets from disk; synchronous I/O is inherent to asset loading"
        )]
        let result: Result<Vec<u8>> = std::fs::read("/nonexistent/path").context(ReadFileSnafu {
            path: PathBuf::from("/nonexistent/path"),
        });
        let err = result.unwrap_err();
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn parse_manifest_display() {
        let err = Error::ParseManifest {
            path: PathBuf::from("/some/pack.toml"),
            reason: "expected mapping".to_owned(),
            location: snafu::Location::new("test", 0, 0),
        };
        let msg = err.to_string();
        assert!(msg.contains("pack.toml"));
        assert!(msg.contains("expected mapping"));
    }

    #[test]
    fn errors_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }
}
