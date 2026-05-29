use std::path::PathBuf;

use snafu::Snafu;

use crate::id::InvalidThemeId;

/// The crate's top-level error type.
///
/// Every public entry point — parse, registry discovery, resolution, sink
/// emission — funnels its failure cases through one of these variants. Each
/// variant carries the smallest payload that lets a caller produce a
/// human-actionable message; no variant swallows context.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ThemeError {
    /// A theme name failed [`ThemeId`](crate::ThemeId) parsing.
    #[snafu(display("invalid theme id {candidate:?}: {source}"))]
    InvalidId {
        /// The string presented at the boundary.
        candidate: String,
        /// The parse failure reason.
        source: InvalidThemeId,
    },

    /// A `themes/<name>.toml` file could not be read.
    #[snafu(display("failed to read theme file {path}"))]
    ReadTheme {
        /// The path that failed to read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// A `themes/<name>.toml` file failed TOML parsing.
    #[snafu(display("failed to parse theme TOML at {path}"))]
    ParseToml {
        /// The path that failed to parse.
        path: String,
        /// Underlying toml deserialization error.
        source: toml::de::Error,
    },

    /// A `themes/` discovery directory could not be enumerated.
    #[snafu(display("failed to enumerate themes directory {path}"))]
    Discovery {
        /// The directory that failed enumeration.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// The on-disk filename did not match the parsed `[meta].id`.
    #[snafu(display(
        "theme file {path:?} declares id {declared:?}, expected {expected:?} from filename"
    ))]
    IdMismatch {
        /// Source file path on disk.
        path: PathBuf,
        /// `id` field declared in the TOML.
        declared: String,
        /// Id derived from the filename stem.
        expected: String,
    },

    /// A token reference pointed at a node the theme does not define.
    ///
    /// This is the runtime sibling of the [`crate::lint::UnknownTokenRule`]
    /// shape: the lint rule rejects unknown tokens at the spec boundary; this
    /// variant catches the same condition during resolution if a renderer
    /// somehow asks for a token that survived the spec gate.
    #[snafu(display(
        "theme {theme_id} does not define token {reference}; available tokens in this namespace: {available}"
    ))]
    UnknownToken {
        /// The theme that was queried.
        theme_id: String,
        /// The token reference that missed.
        reference: String,
        /// Comma-joined list of tokens defined in the same namespace.
        available: String,
    },

    /// A `[color.tone]` entry pointed at a role that does not exist.
    #[snafu(display(
        "tone {tone_name:?} references unknown color role {role:?} in theme {theme_id}"
    ))]
    UnknownRole {
        /// The theme that was being resolved.
        theme_id: String,
        /// The tone whose target failed lookup.
        tone_name: String,
        /// The missing role name.
        role: String,
    },

    /// The registry was asked for a theme it does not carry.
    #[snafu(display("theme {theme_id} not found in registry; available: {available}"))]
    NotFound {
        /// The theme that was requested.
        theme_id: String,
        /// Comma-joined list of registry entries.
        available: String,
    },

    /// A sink could not write to the supplied buffer.
    #[snafu(display("sink {sink:?} failed to emit"))]
    Sink {
        /// The sink that failed (e.g. `"css"`, `"ooxml"`).
        sink: String,
        /// Underlying formatter error.
        source: std::fmt::Error,
    },
}
