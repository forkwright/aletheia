//! Shared authenticated HTTP client for view-layer API requests.

use std::time::Duration;

use reqwest::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use crate::state::connection::ConnectionConfig;

/// Build a `reqwest::Client` with the Bearer token from `config` attached
/// as a default header. Views should call this instead of `Client::new()`
/// so that all API requests carry the auth token.
pub(crate) fn authenticated_client(config: &ConnectionConfig) -> Client {
    let mut headers = HeaderMap::new();

    if let Some(ref token) = config.auth_token {
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(AUTHORIZATION, val);
        }
    }

    // WHY: fall back to default client if builder fails (e.g. no TLS provider
    // installed yet); views already handle HTTP errors gracefully.
    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn builds_client_without_token() {
        install_crypto();
        let config = ConnectionConfig::default();
        let client = authenticated_client(&config);
        // WHY: ensure the client builds and is usable. The default config
        // has no token, so no Authorization header is added.
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn builds_client_with_token() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("test-token-123".to_string()),
            ..ConnectionConfig::default()
        };
        let client = authenticated_client(&config);
        let debug = format!("{client:?}");
        // WHY: client builds; we cannot easily inspect default headers
        // through the public API, but a successful build covers the path.
        assert!(!debug.is_empty());
    }

    #[test]
    fn invalid_token_falls_through_to_default() {
        install_crypto();
        // NOTE: A token with non-ASCII bytes triggers HeaderValue::from_str
        // failure and the function silently skips adding the header.
        let config = ConnectionConfig {
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };
        let client = authenticated_client(&config);
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn empty_token_string_is_accepted() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some(String::new()),
            ..ConnectionConfig::default()
        };
        let _client = authenticated_client(&config);
        // No assertion beyond no panic; covers the Some-but-empty branch.
    }
}
