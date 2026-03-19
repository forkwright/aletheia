#![deny(missing_docs)]
//! aletheia-taxis: configuration cascade and path resolution
//!
//! Taxis (τάξις): "arrangement, ordering." Resolves configuration and files
//! through the oikos three-tier hierarchy: nous/{id}/ → shared/ → theke/.
//!
//! Depends only on `aletheia-koina`.

/// Three-tier file discovery and resolution through the oikos hierarchy.
pub mod cascade;
/// Configuration types for an Aletheia instance (agents, gateway, channels, embedding).
pub mod config;
/// Encryption at rest for sensitive configuration values.
pub mod encrypt;
/// Taxis-specific error types for configuration loading and path resolution.
pub mod error;
/// Figment-based configuration loader with TOML file and environment variable cascade.
pub mod loader;
/// Instance directory structure and path resolution for all Aletheia subsystems.
pub mod oikos;
/// Config redaction: strips secrets before API exposure.
pub mod redact;
/// Hot-reload classification: restart vs live update.
pub mod reload;
/// Config section validation.
pub mod validate;
