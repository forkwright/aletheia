//! aletheia-thesauros — domain pack loader
//!
//! Thesauros (θησαυρός) — "treasure house." Loads external domain packs that
//! inject context, tools, and configuration overlays into the agent runtime.
//!
//! A pack is a directory containing a `pack.yaml` manifest and referenced files.
//! The loader resolves manifests, reads context files, and returns structured
//! data for the bootstrap assembler and tool registry to consume.
//!
//! Depends on `aletheia-koina` and `aletheia-organon` (tool types).

/// Error types for domain pack operations.
pub mod error;
/// Pack loading and context resolution into [`loader::LoadedPack`] values.
pub mod loader;
/// Pack manifest parsing — reads `pack.yaml` into [`manifest::PackManifest`].
pub mod manifest;
/// Registration of pack-declared tools into the [`aletheia_organon::registry::ToolRegistry`].
pub mod tools;
