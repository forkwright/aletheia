//! OAuth 2.0 Device Code Flow (RFC 8628).
//!
//! This module implements the Device Authorization Grant for OAuth 2.0,
//! suitable for devices with limited input capabilities or no browser.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use tracing::{debug, info, warn};

use aletheia_koina::secret::SecretString;

use super::OAuthProvider;
use super::file_ops::CredentialFile;
use super::pkce::url_encode;

/// Errors from Device Code authentication flow.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum DeviceCodeError {
    /// HTTP request failed.
    #[snafu(display("HTTP request failed: {source}"))]
    HttpRequest {
        /// Underlying HTTP error.
        source: reqwest::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// OAuth error response.
    #[snafu(display("OAuth error: {error}"))]
    OAuthError {
        /// OAuth error code.
        error: String,
        /// OAuth error description.
        error_description: Option<String>,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to parse response.
    #[snafu(display("failed to parse response: {source}"))]
    ParseResponse {
        /// JSON parse error.
        source: serde_json::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Device code expired before user completed authorization.
    #[snafu(display("device code expired - please try again"))]
    ExpiredToken {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// User denied the authorization request.
    #[snafu(display("access denied by user"))]
    AccessDenied {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The device authorization endpoint is not configured.
    #[snafu(display("device authorization endpoint not configured for this provider"))]
    MissingDeviceEndpoint {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Polling was cancelled.
    #[snafu(display("authentication was cancelled"))]
    Cancelled {
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to save credential file.
    #[snafu(display("failed to save credentials: {source}"))]
    SaveCredential {
        /// IO error from save operation.
        source: std::io::Error,
        /// Source location of the error.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result type for Device Code operations.
pub type Result<T> = std::result::Result<T, DeviceCodeError>;

/// OAuth 2.0 Device Authorization Response.
///
/// Returned by the device authorization endpoint.
#[derive(Debug, Deserialize)]
struct DeviceAuthorizationResponse {
    /// The device verification code.
    device_code: String,
    /// The user verification code.
    user_code: String,
    /// The verification URI.
    verification_uri: String,
    /// Optional verification URI with user code pre-filled.
    #[serde(rename = "verification_uri_complete")]
    verification_uri_complete: Option<String>,
    /// Lifetime of the device code in seconds.
    #[serde(default = "default_expires_in")]
    expires_in: u64,
    /// Minimum polling interval in seconds.
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_expires_in() -> u64 {
    1800 // 30 minutes default per RFC 8628
}

fn default_interval() -> u64 {
    5 // 5 seconds default per RFC 8628
}

/// OAuth 2.0 Token Response.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: SecretString,
    #[serde(default)]
    refresh_token: Option<SecretString>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    /// Token type (typically "Bearer").
    #[serde(default)]
    #[expect(dead_code, reason = "field provided by OAuth but not currently used")]
    token_type: String, // kanon:ignore RUST/plain-string-secret
}

/// OAuth 2.0 Token Error Response (for device flow polling).
#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Extended OAuth provider configuration with device authorization endpoint.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DeviceOAuthProvider {
    /// Base OAuth provider configuration.
    pub base: OAuthProvider,
    /// Device authorization endpoint URL.
    pub device_authorization_url: String,
}

impl DeviceOAuthProvider {
    /// Create a new device OAuth provider configuration.
    #[must_use]
    pub fn new(
        client_id: impl Into<String>,
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
        device_authorization_url: impl Into<String>,
    ) -> Self {
        Self {
            base: OAuthProvider::new(client_id, authorization_url, token_url),
            device_authorization_url: device_authorization_url.into(),
        }
    }

    /// Add a scope to the provider configuration.
    #[must_use]
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.base.scopes.push(scope.into());
        self
    }

    /// Set the redirect URI (used for refresh, not device flow).
    #[must_use]
    pub fn with_redirect_uri(mut self, uri: impl Into<String>) -> Self {
        self.base.redirect_uri = Some(uri.into());
        self
    }
}

/// Build form-urlencoded body from params with string keys.
fn build_form_body_str(params: &HashMap<String, String>) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Request device authorization from the OAuth provider.
async fn request_device_authorization(
    client: &reqwest::Client,
    provider: &DeviceOAuthProvider,
) -> Result<DeviceAuthorizationResponse> {
    let mut params = HashMap::new();
    params.insert("client_id".to_string(), provider.base.client_id.clone());

    if !provider.base.scopes.is_empty() {
        params.insert("scope".to_string(), provider.base.scopes.join(" "));
    }

    let body = build_form_body_str(&params);

    let response = client
        .post(&provider.device_authorization_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context(HttpRequestSnafu)?;

    let status = response.status();
    let body_text = response.text().await.context(HttpRequestSnafu)?;

    if !status.is_success() {
        // Try to parse as OAuth error
        if let Ok(err) = serde_json::from_str::<TokenErrorResponse>(&body_text) {
            return Err(DeviceCodeError::OAuthError {
                error: err.error,
                error_description: err.error_description,
                location: snafu::location!(),
            });
        }
        return Err(DeviceCodeError::OAuthError {
            error: format!("HTTP {}: {}", status, body_text),
            error_description: None,
            location: snafu::location!(),
        });
    }

    serde_json::from_str(&body_text).context(ParseResponseSnafu)
}

/// Poll the token endpoint until authorization is complete or fails.
async fn poll_token_endpoint(
    client: &reqwest::Client,
    provider: &DeviceOAuthProvider,
    device_code: &str,
    interval_secs: u64,
    expires_in_secs: u64,
) -> Result<TokenResponse> {
    let start_time = std::time::Instant::now();
    let expires_after = Duration::from_secs(expires_in_secs);

    let mut current_interval = Duration::from_secs(interval_secs);

    loop {
        // Check if we've exceeded the expiration time
        if start_time.elapsed() >= expires_after {
            return Err(DeviceCodeError::ExpiredToken {
                location: snafu::location!(),
            });
        }

        // Wait for the polling interval
        tokio::time::sleep(current_interval).await;

        let mut params = HashMap::new();
        params.insert(
            "grant_type".to_string(),
            "urn:ietf:params:oauth:grant-type:device_code".to_string(),
        );
        params.insert("device_code".to_string(), device_code.to_string());
        params.insert("client_id".to_string(), provider.base.client_id.clone());

        let body = build_form_body_str(&params);

        let response = client
            .post(&provider.base.token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context(HttpRequestSnafu)?;

        let status = response.status();
        let body_text = response.text().await.context(HttpRequestSnafu)?;

        if status.is_success() {
            // Success! Parse the token response
            return serde_json::from_str(&body_text).context(ParseResponseSnafu);
        }

        // Handle error responses
        let error_resp: TokenErrorResponse =
            serde_json::from_str(&body_text).context(ParseResponseSnafu)?;

        match error_resp.error.as_str() {
            "authorization_pending" => {
                // User hasn't completed authorization yet, continue polling
                debug!("authorization pending, continuing to poll");
                continue;
            }
            "slow_down" => {
                // Server wants us to poll less frequently
                warn!("server requested slow down, increasing polling interval");
                current_interval += Duration::from_secs(5);
                continue;
            }
            "expired_token" => {
                return Err(DeviceCodeError::ExpiredToken {
                    location: snafu::location!(),
                });
            }
            "access_denied" => {
                return Err(DeviceCodeError::AccessDenied {
                    location: snafu::location!(),
                });
            }
            _ => {
                return Err(DeviceCodeError::OAuthError {
                    error: error_resp.error,
                    error_description: error_resp.error_description,
                    location: snafu::location!(),
                });
            }
        }
    }
}

/// Perform the complete Device Code authorization flow.
///
/// This function:
/// 1. Requests device authorization from the provider
/// 2. Displays the user code and verification URI
/// 3. Polls the token endpoint until the user completes authorization
/// 4. Returns the credential file data
///
/// # Errors
///
/// Returns an error if any step of the flow fails, the user denies access,
/// or the device code expires.
///
/// # Example
///
/// ```no_run
/// use aletheia_symbolon::credential::device_code::{DeviceOAuthProvider, device_code_login};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = DeviceOAuthProvider::new(
///     "my-client-id",
///     "https://auth.example.com/authorize",
///     "https://auth.example.com/token",
///     "https://auth.example.com/device",
/// )
/// .with_scope("read");
///
/// let credential = device_code_login(&provider).await?;
/// # Ok(())
/// # }
/// ```
pub async fn device_code_login(provider: &DeviceOAuthProvider) -> Result<CredentialFile> {
    let client = reqwest::Client::new();

    // Step 1: Request device authorization
    info!("requesting device authorization");
    let device_auth = request_device_authorization(&client, provider).await?;

    // Step 2: Display instructions to the user
    eprintln!("\n🔐 Device Authentication Required\n");
    eprintln!("Please visit: {}", device_auth.verification_uri);
    eprintln!("And enter code: {}\n", device_auth.user_code);

    if let Some(complete_uri) = &device_auth.verification_uri_complete {
        eprintln!("Or visit this direct link:\n  {}\n", complete_uri);
    }

    eprintln!(
        "Waiting for authorization... (expires in {} seconds)\n",
        device_auth.expires_in
    );

    // Step 3: Poll for token
    let token_response = poll_token_endpoint(
        &client,
        provider,
        &device_auth.device_code,
        device_auth.interval,
        device_auth.expires_in,
    )
    .await?;

    info!("successfully obtained access token via device flow");
    eprintln!("\n✓ Authentication successful!\n");

    // Step 4: Build credential file
    let expires_at = token_response
        .expires_in
        .map(|secs| super::unix_epoch_ms() + secs * 1000);

    let scopes = token_response
        .scope
        .map(|s| s.split_whitespace().map(String::from).collect());

    Ok(CredentialFile {
        token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        expires_at,
        scopes,
        subscription_type: None,
    })
}

/// Perform Device Code flow and save credentials to a file.
///
/// This is a convenience wrapper around [`device_code_login`] that saves
/// the resulting credentials to the specified path.
///
/// # Errors
///
/// Returns an error if the flow fails or if the credential file cannot be saved.
pub async fn device_code_login_and_save(
    provider: &DeviceOAuthProvider,
    path: &std::path::Path,
) -> Result<CredentialFile> {
    let cred = device_code_login(provider).await?;
    cred.save(path).context(SaveCredentialSnafu)?;
    info!(path = %path.display(), "credentials saved");
    Ok(cred)
}

/// Device Code flow with custom user interaction callback.
///
/// This variant allows the caller to customize how the user code and
/// verification URI are presented to the user.
///
/// # Type Parameters
///
/// * `F` - A function or closure that takes the user code and verification URI
///
/// # Errors
///
/// Returns an error if any step of the flow fails.
pub async fn device_code_login_with_callback<F>(
    provider: &DeviceOAuthProvider,
    display_callback: F,
) -> Result<CredentialFile>
where
    F: FnOnce(&str, &str, Option<&str>),
{
    let client = reqwest::Client::new();

    // Step 1: Request device authorization
    info!("requesting device authorization");
    let device_auth = request_device_authorization(&client, provider).await?;

    // Step 2: Call the display callback
    display_callback(
        &device_auth.user_code,
        &device_auth.verification_uri,
        device_auth.verification_uri_complete.as_deref(),
    );

    // Step 3: Poll for token
    let token_response = poll_token_endpoint(
        &client,
        provider,
        &device_auth.device_code,
        device_auth.interval,
        device_auth.expires_in,
    )
    .await?;

    info!("successfully obtained access token via device flow");

    // Step 4: Build credential file
    let expires_at = token_response
        .expires_in
        .map(|secs| super::unix_epoch_ms() + secs * 1000);

    let scopes = token_response
        .scope
        .map(|s| s.split_whitespace().map(String::from).collect());

    Ok(CredentialFile {
        token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        expires_at,
        scopes,
        subscription_type: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_oauth_provider_builder() {
        let provider = DeviceOAuthProvider::new(
            "test-client",
            "https://example.com/auth",
            "https://example.com/token",
            "https://example.com/device",
        )
        .with_scope("read")
        .with_scope("write")
        .with_redirect_uri("http://localhost/callback");

        assert_eq!(provider.base.client_id, "test-client");
        assert_eq!(
            provider.device_authorization_url,
            "https://example.com/device"
        );
        assert_eq!(provider.base.scopes, vec!["read", "write"]);
        assert_eq!(
            provider.base.redirect_uri,
            Some("http://localhost/callback".to_string())
        );
    }

    #[test]
    fn test_default_expires_in() {
        assert_eq!(default_expires_in(), 1800);
    }

    #[test]
    fn test_default_interval() {
        assert_eq!(default_interval(), 5);
    }

    #[test]
    fn test_build_form_body_str() {
        let mut params = HashMap::new();
        params.insert(
            "grant_type".to_string(),
            "urn:ietf:params:oauth:grant-type:device_code".to_string(),
        );
        params.insert("device_code".to_string(), "abc123".to_string());
        params.insert("client_id".to_string(), "my client".to_string());

        let body = build_form_body_str(&params);

        // HashMap iteration order is not guaranteed, so check for presence of each param
        assert!(body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"));
        assert!(body.contains("device_code=abc123"));
        assert!(body.contains("client_id=my%20client"));
        assert_eq!(body.matches('&').count(), 2);
    }

    // Note: Integration tests for the full flow would require a mock OAuth server
    // and are not included here. The polling logic and HTTP interactions are
    // tested through integration tests in the tests/ directory.
}
