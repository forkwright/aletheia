//! aletheia-symbolon — authentication and authorization
//!
//! Symbolon (σύμβολον — "token, credential") handles JWT sessions,
//! API key validation, Argon2id password hashing, and RBAC permission checks.

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
