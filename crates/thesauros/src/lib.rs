//! aletheia-thesauros — domain pack loader
//!
//! Thesauros (θησαυρός) — "treasure house." Loads external domain packs that
//! inject context, tools, and configuration overlays into the agent runtime.
//!
//! A pack is a directory containing a `pack.yaml` manifest and referenced files.
//! The loader resolves manifests, reads context files, and returns structured
//! data for the bootstrap assembler and tool registry to consume.
//!
//! Depends only on `aletheia-koina`.

pub mod error;
pub mod loader;
pub mod manifest;
