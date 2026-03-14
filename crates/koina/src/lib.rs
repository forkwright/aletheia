//! aletheia-koina — core types, errors, and tracing for Aletheia
//!
//! Koina (κοινά) — "shared things." The common foundation that every crate depends on.
//! Imports nothing from other Aletheia crates. Contains only types, error definitions,
//! and tracing initialization.

/// Credential provider trait for dynamic API key resolution.
pub mod credential;
/// Error types shared across all Aletheia crates (file I/O, JSON, identifiers).
pub mod error;
/// Newtype wrappers for domain identifiers ([`id::NousId`], [`id::SessionId`], [`id::TurnId`], [`id::ToolName`]).
pub mod id;
/// Sensitive value redaction for safe log output (API keys, tokens, passwords).
pub mod redact;
/// Tracing subscriber initialization for human-readable and JSON log output.
pub mod tracing_init;

#[cfg(test)]
mod assertions {
    use super::id::*;
    use static_assertions::assert_impl_all;

    assert_impl_all!(NousId: Send, Sync);
    assert_impl_all!(SessionId: Send, Sync);
    assert_impl_all!(TurnId: Send, Sync);
    assert_impl_all!(ToolName: Send, Sync);
}
