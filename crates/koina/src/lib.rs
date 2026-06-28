#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
//! aletheia-koina: core types, errors, and tracing for Aletheia
//!
//! Koina (κοινά): "shared things." The common foundation that every crate depends on.
//! Imports nothing from other Aletheia crates. Contains only types, error definitions,
//! and tracing initialization.

/// Shared agent lifecycle/status values.
pub mod agent;
/// RFC 4648 base64 encoding and decoding (standard and URL-safe variants).
pub mod base64;
/// Setup-time cleanup registration via [`cleanup::CleanupRegistry`].
pub mod cleanup;
/// Credential provider trait for dynamic API key resolution.
pub mod credential;
/// Shared configuration defaults (token budgets, timeouts, iteration limits).
pub mod defaults;
/// Disk space monitoring: threshold checks, cached monitor, write guards.
pub mod disk_space;
/// Error types shared across all Aletheia crates (file I/O, JSON, identifiers).
pub mod error;
/// Error classification for intelligent retry and escalation decisions.
pub mod error_class;
/// Internal event system coupling metrics and structured logs.
pub mod event;
/// Shared fjall storage helpers: database open, temp stores, timestamp formatting.
#[cfg(feature = "fjall")]
pub mod fjall;
/// Restricted filesystem helpers for writing sensitive files.
pub mod fs;
/// Shared HTTP constants (content types, auth prefix, API paths).
pub mod http;
/// Newtype wrappers for domain identifiers ([`id::NousId`], [`id::SessionId`], [`id::TurnId`], [`id::ToolName`]).
pub mod id;
/// Shared Prometheus metrics registry (prometheus-client wrapper).
pub mod metrics;
/// Shared model catalog and tier defaults.
pub mod models;
/// Multi-output pipeline stages via the OutputBuffer pattern.
pub mod output_buffer;
/// Sensitive value redaction for safe log output (API keys, tokens, passwords).
pub mod redact;
/// Tracing layer that redacts sensitive field values before output.
pub mod redacting_layer;
/// Configurable retry strategies and backoff computation ([`retry::BackoffStrategy`], [`retry::RetryConfig`]).
pub mod retry;
/// Secret string newtype that prevents accidental leakage of sensitive values.
pub mod secret;
/// Trait abstractions for filesystem, clock, and environment operations.
pub mod system;
/// Internal ULID generation (Crockford base32, 48-bit timestamp + 80-bit random).
pub mod ulid;
/// Internal UUID v4 generation (dependency-free).
pub mod uuid;

#[cfg(test)]
mod assertions {
    use super::id::*;

    const _: fn() = || {
        fn assert<T: Send + Sync>() {}
        assert::<NousId>();
        assert::<SessionId>();
        assert::<TurnId>();
        assert::<ToolName>();
    };
}
