//! aletheia-symbolon — authentication and authorization
//!
//! Symbolon (σύμβολον — "token, credential") handles JWT sessions,
//! API key validation, Argon2id password hashing, and RBAC permission checks.
//!
//! # Architecture
//!
//! - [`auth::AuthService`] — unified facade composing all auth subsystems
//! - [`store::AuthStore`] — `SQLite` backend for users, API keys, and token revocation
//! - [`jwt::JwtManager`] — HMAC-SHA256 JWT issuance and validation
//! - [`api_key`] — blake3-hashed API key generation and validation
//! - [`password`] — Argon2id password hashing and verification
//!
//! # Key Types
//!
//! - [`types::Claims`] — decoded JWT payload with role and nous scope
//! - [`types::Role`] — RBAC roles: Operator, Agent, Readonly
//! - [`types::TokenKind`] — distinguishes access from refresh tokens
//! - [`types::ApiKeyRecord`] — stored API key metadata (never the secret)
//! - [`types::Action`] — RBAC action descriptors for [`auth::AuthService::authorize`]

pub mod api_key;
pub mod auth;
pub mod error;
pub mod jwt;
pub mod password;
pub mod store;
pub mod types;

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
