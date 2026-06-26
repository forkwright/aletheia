//! Shared OAuth 2.0 token and error response types.
//!
//! WHY: the refresh, PKCE, and device-code flows all parse structurally
//! identical OAuth token and error responses. Centralizing the definitions
//! here removes private duplicates across `refresh.rs`, `pkce.rs`, and
//! `device_code.rs` and guarantees that future field changes propagate to
//! every caller.

use koina::secret::SecretString;
use serde::Deserialize;

/// OAuth 2.0 token response from the token endpoint.
///
/// Fields follow RFC 6749. `refresh_token` and `expires_in` are optional
/// because some `IdPs` omit them; callers are responsible for choosing sensible
/// defaults where their flow requires them.
#[derive(Debug, Deserialize)]
pub(super) struct OAuthTokenResponse {
    pub access_token: SecretString,
    #[serde(default)]
    pub refresh_token: Option<SecretString>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
    /// Token type (typically "Bearer").
    #[serde(default)]
    #[expect(dead_code, reason = "field provided by OAuth but not currently used")]
    pub token_type: String, // kanon:ignore RUST/plain-string-secret
}

/// OAuth 2.0 error response from the token endpoint.
///
/// WHY: `error_description` is intentionally exposed as a typed field so that
/// flows which need to report it (PKCE, device code) can do so; the refresh
/// flow still logs only the normalized `error` code.
#[derive(Debug, Deserialize)]
pub(super) struct OAuthErrorResponse {
    pub error: String,
    #[serde(default)]
    pub error_description: Option<String>,
}
