//! Shared authenticated HTTP client for view-layer API requests.

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
        .build()
        .unwrap_or_default()
}
