#![deny(missing_docs)]
//! aletheia-symbolon: authentication and authorization
//!
//! Symbolon (σύμβολον: "token, credential") handles JWT sessions,
//! API key validation, Argon2id password hashing, and RBAC permission checks.

/// API key generation, validation, and revocation.
pub(crate) mod api_key;
/// Unified auth facade composing JWT, API keys, passwords, and RBAC.
pub(crate) mod auth;
/// Three-state circuit breaker for OAuth token refresh.
pub(crate) mod circuit_breaker;
/// Credential provider implementations for LLM API key resolution.
pub mod credential;
/// AES-256-GCM encryption for credential files at rest.
pub(crate) mod encrypt;
/// Symbolon-specific error types and result alias.
pub(crate) mod error;
/// JWT token issuance, validation, and refresh.
pub mod jwt;
/// Argon2id password hashing and verification.
pub(crate) mod password;
/// `SQLite`-backed credential and token storage.
pub(crate) mod store;
/// Shared auth types: claims, roles, actions, token kinds.
pub mod types;
/// Internal utilities shared across modules.
pub(crate) mod util;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    use super::auth::AuthService;
    use super::jwt::JwtManager;
    use super::store::AuthStore;

    assert_impl_all!(AuthService: Send);
    assert_impl_all!(AuthStore: Send);
    assert_impl_all!(JwtManager: Send, Sync);
}
