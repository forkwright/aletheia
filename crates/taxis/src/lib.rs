#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
//! aletheia-taxis: configuration cascade and path resolution
//!
//! Taxis (τάξις): "arrangement, ordering." Resolves configuration and files
//! through the oikos three-tier hierarchy: nous/{id}/ → shared/ → theke/.
//!
//! Depends on `aletheia-koina` and `aletheia-eidos`.

/// Three-tier file discovery and resolution through the oikos hierarchy.
pub mod cascade;
/// Configuration types for an Aletheia instance (agents, gateway, channels, embedding).
pub mod config;
/// TOML decryption pipeline for `enc:`-prefixed config values.
mod config_decrypt;
/// Encryption at rest for sensitive configuration values.
pub mod encrypt;
/// Taxis-specific error types for configuration loading and path resolution.
pub mod error;
/// Environment variable interpolation for TOML configuration strings.
pub mod interpolate;
/// Configuration loader with TOML file and environment variable cascade.
pub mod loader;
/// Instance directory structure and path resolution for all Aletheia subsystems.
pub mod oikos;
/// Resource precondition checks at startup (disk space, port availability, permissions).
pub mod preflight;
/// Config redaction: strips secrets before API exposure.
pub mod redact;
/// Static parameter registry: metadata for every tunable constant.
pub mod registry;
/// Hot-reload classification: restart vs live update.
pub mod reload;
/// Shared "is this config key sensitive?" predicate used by redaction and
/// at-rest encryption. Kept private to the crate so the two paths cannot
/// drift again.
mod sensitive;
/// Test-only helpers (EnvJail) for unit + integration tests.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
/// Config section validation.
pub mod validate;
/// Config-time validation of agent workspace directory structure.
pub mod workspace_schema;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_modules_accessible() {
        // INVARIANT: all public modules must be importable and have expected type names
        let config_name = std::any::type_name::<config::AletheiaConfig>();
        let cascade_name = std::any::type_name::<cascade::Tier>();
        let error_name = std::any::type_name::<error::Error>();
        let oikos_name = std::any::type_name::<oikos::Oikos>();

        assert!(
            config_name.contains("AletheiaConfig"),
            "config module should export AletheiaConfig"
        );
        assert!(
            cascade_name.contains("Tier"),
            "cascade module should export Tier"
        );
        assert!(
            error_name.contains("Error"),
            "error module should export Error"
        );
        assert!(
            oikos_name.contains("Oikos"),
            "oikos module should export Oikos"
        );
    }
}
