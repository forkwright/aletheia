#![deny(missing_docs)]
//! aletheia-koina: core types, errors, and tracing for Aletheia
//!
//! Koina (κοινά): "shared things." The common foundation that every crate depends on.
//! Imports nothing from other Aletheia crates. Contains only types, error definitions,
//! and tracing initialization.

/// Setup-time cleanup registration via RAII guards ([`cleanup::CleanupGuard`], [`cleanup::CleanupRegistry`]).
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
/// Restricted filesystem helpers for writing sensitive files.
pub mod fs;
/// Shared HTTP constants (content types, auth prefix, API paths).
pub mod http;
/// Newtype wrappers for domain identifiers ([`id::NousId`], [`id::SessionId`], [`id::TurnId`], [`id::ToolName`]).
pub mod id;
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
/// Tracing subscriber initialization for human-readable and JSON log output.
pub mod tracing_init;
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
