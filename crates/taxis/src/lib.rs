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
/// Environment variable interpolation for TOML configuration strings.
pub mod interpolate;
/// Figment-based configuration loader with TOML file and environment variable cascade.
pub mod loader;
/// Instance directory structure and path resolution for all Aletheia subsystems.
pub mod oikos;
/// Resource precondition checks at startup (disk space, port availability, permissions).
pub mod preflight;
/// Config redaction: strips secrets before API exposure.
pub mod redact;
/// Hot-reload classification: restart vs live update.
pub mod reload;
/// Config section validation.
pub mod validate;
/// Config-time validation of agent workspace directory structure.
pub mod workspace_schema;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_modules_accessible() {
        // INVARIANT: all public modules must be importable
        let _ = std::any::type_name::<config::AletheiaConfig>();
        let _ = std::any::type_name::<cascade::Tier>();
        let _ = std::any::type_name::<error::Error>();
        let _ = std::any::type_name::<oikos::Oikos>();
    }
}
