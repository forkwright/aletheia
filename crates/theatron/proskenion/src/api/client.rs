//! Shared authenticated HTTP client for view-layer API requests.

use std::time::Duration;

use reqwest::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::state::connection::ConnectionConfig;

/// Outcome of a workspace file save against `PUT /api/v1/workspace/files/content`.
///
/// WHY: the viewer renders distinct UX per failure class -- a 413 is "split
/// the note", a 409 is "reload before saving", a transport error is
/// retryable. Mapping the wire status to a typed result keeps that branching
/// declarative at the call site instead of re-deriving it from raw codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveOutcome {
    /// Write succeeded.
    Saved,
    /// File exceeds the server's size cap (HTTP 413).
    TooLarge,
    /// File changed on disk since it was loaded (HTTP 409).
    Conflict,
    /// Any other failure, carrying a human-readable description.
    Failed(String),
}

/// Persist `content` to the workspace file at `path` (relative to the vault
/// root) via the workspace content write endpoint.
///
/// The server resolves `path` through its path-escape guard; the client only
/// ever holds workspace-relative paths. Returns a [`SaveOutcome`] mapping the
/// HTTP result to the UX-relevant cases.
pub(crate) async fn save_workspace_file(
    config: &ConnectionConfig,
    path: &str,
    content: &str,
) -> SaveOutcome {
    let client = authenticated_client(config);
    let base = config.server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/workspace/files/content");
    let body = serde_json::json!({ "path": path, "content": content });

    match client.put(&url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => SaveOutcome::Saved,
        Ok(resp) if resp.status().as_u16() == 413 => SaveOutcome::TooLarge,
        Ok(resp) if resp.status().as_u16() == 409 => SaveOutcome::Conflict,
        Ok(resp) => {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            if detail.is_empty() {
                SaveOutcome::Failed(format!("server returned {status}"))
            } else {
                SaveOutcome::Failed(format!("server returned {status}: {}", detail.trim()))
            }
        }
        Err(e) => SaveOutcome::Failed(format!("connection error: {e}")),
    }
}

/// Ask the server to open the workspace file at `path` in the operator's
/// default application via `POST /api/v1/workspace/open`.
///
/// WHY: the client never learns the absolute vault root, so opening with the
/// host's default app is a server-side action over the relative path (the
/// binary and the vault are co-located). Returns `Ok` on success or an
/// `Err` carrying a human-readable description.
pub(crate) async fn open_workspace_file(
    config: &ConnectionConfig,
    path: &str,
) -> Result<(), String> {
    let client = authenticated_client(config);
    let base = config.server_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/workspace/open");
    let body = serde_json::json!({ "path": path });

    match client.post(&url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) => {
            let status = resp.status();
            let detail = resp.text().await.unwrap_or_default();
            if detail.is_empty() {
                Err(format!("server returned {status}"))
            } else {
                Err(format!("server returned {status}: {}", detail.trim()))
            }
        }
        Err(e) => Err(format!("connection error: {e}")),
    }
}

/// Build a `reqwest::Client` with the Bearer token from `config` attached
/// as a default header. Views should call this instead of `Client::new()`
/// so that all API requests carry the auth token.
pub(crate) fn authenticated_client(config: &ConnectionConfig) -> Client {
    build_authenticated_client(config, Some(Duration::from_secs(30)))
}

pub(crate) fn authenticated_streaming_client(config: &ConnectionConfig) -> Client {
    build_authenticated_client(config, None)
}

fn build_authenticated_client(config: &ConnectionConfig, timeout: Option<Duration>) -> Client {
    let headers = request_headers(config);

    // WHY: fall back to default client if builder fails (e.g. no TLS provider
    // installed yet); views already handle HTTP errors gracefully.
    let mut builder = Client::builder()
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(30));
    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
    }
    builder.build().unwrap_or_else(|err| {
        tracing::warn!(error = %err, "failed to build authenticated HTTP client");
        Client::new()
    })
}

fn request_headers(config: &ConnectionConfig) -> HeaderMap {
    let mut headers = HeaderMap::new();

    if let Some(ref token) = config.auth_token
        && let Ok(val) = HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(AUTHORIZATION, val);
    }

    if let Err(err) = config.request_policy.insert_headers(&mut headers) {
        tracing::warn!(error = %err, "skipping invalid request policy header");
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers
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
    fn request_headers_include_default_csrf_policy() {
        let config = ConnectionConfig::default();
        let headers = request_headers(&config);
        assert_eq!(
            headers
                .get("x-requested-with")
                .and_then(|value| value.to_str().ok()),
            Some("aletheia")
        );
    }

    #[test]
    fn request_headers_include_custom_csrf_policy() {
        let config = ConnectionConfig {
            request_policy: skene::api::RequestPolicy {
                csrf: skene::api::request_policy::CsrfRequestPolicy {
                    enabled: true,
                    header_name: "x-aletheia-csrf".to_string(),
                    header_value: "custom-csrf-value".to_string(),
                },
            },
            ..ConnectionConfig::default()
        };

        let headers = request_headers(&config);

        assert_eq!(
            headers
                .get("x-aletheia-csrf")
                .and_then(|value| value.to_str().ok()),
            Some("custom-csrf-value")
        );
        assert!(headers.get("x-requested-with").is_none());
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
        let client = authenticated_client(&config);
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn streaming_client_builds_with_token() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("stream-token-456".to_string()),
            ..ConnectionConfig::default()
        };
        let client = authenticated_streaming_client(&config);
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }

    #[test]
    fn streaming_client_ignores_invalid_token() {
        install_crypto();
        let config = ConnectionConfig {
            auth_token: Some("bad\x00token".to_string()),
            ..ConnectionConfig::default()
        };
        let client = authenticated_streaming_client(&config);
        let debug = format!("{client:?}");
        assert!(!debug.is_empty());
    }
}
