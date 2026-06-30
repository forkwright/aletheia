//! Credential provider implementations for LLM API key resolution.

pub(crate) mod admin;
mod file_ops;
#[cfg(feature = "keyring")]
mod keyring_provider;
mod oauth_types;
mod providers;
mod refresh;

/// OAuth 2.0 PKCE Authorization Code Flow (RFC 7636 + RFC 8252).
pub mod pkce;

/// OAuth 2.0 Device Code Flow (RFC 8628).
pub mod device_code;

use std::time::{Duration, SystemTime};

pub use file_ops::CredentialFile;
#[cfg(feature = "keyring")]
pub use keyring_provider::KeyringCredentialProvider;
pub use providers::{CredentialChain, EnvCredentialProvider, FileCredentialProvider};
pub use refresh::{
    RefreshingCredentialProvider, claude_code_credential_path, claude_code_default_path,
    claude_code_provider, force_refresh,
};

pub use pkce::OAuthProvider;

/// Alias for the canonical clock-skew leeway constant defined in [`crate::jwt`].
///
/// WHY: the OAuth credential chain and the JWT validation path must share one
/// canonical value so policy changes cannot drift apart.
use crate::jwt::DEFAULT_CLOCK_SKEW_LEEWAY_SECS as CLOCK_SKEW_LEEWAY_SECS;

/// Caller-rendered action emitted by interactive OAuth credential flows.
// kanon:ignore RUST/no-debug-derive-on-public-types — OAuthRequiredAction is a UI-facing action enum with no secrets; Debug is required for CLI logging
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OAuthRequiredAction {
    /// Open or display a browser authorization URL.
    BrowserOpenUrl {
        /// URL the operator must visit.
        url: String,
    },
    /// Show a device authorization code and verification URI.
    DeviceCode {
        /// Verification URI the operator must visit.
        verification_uri: String,
        /// User code the operator must enter.
        user_code: String,
        /// Optional direct URI with the user code pre-filled.
        verification_uri_complete: Option<String>,
        /// Seconds until the device authorization expires.
        expires_in_secs: u64,
    },
    /// Wait for the OAuth callback on the local loopback listener.
    WaitingForCallback {
        /// Timeout in seconds for the callback wait.
        timeout_secs: u64,
    },
    /// Wait for the provider to complete device authorization.
    WaitingForDeviceAuthorization {
        /// Seconds until the device authorization expires.
        expires_in_secs: u64,
    },
    /// The OAuth flow completed and credentials were received.
    AuthorizationSucceeded,
}

/// Return current time as milliseconds since UNIX epoch, warning if the clock
/// is before epoch rather than silently returning zero.
fn unix_epoch_ms() -> u64 {
    // WHY: as_millis() returns u128 but ms timestamps fit in u64 for ~500M years
    let ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_else(|_| {
            tracing::warn!("system clock before UNIX epoch, using epoch as fallback");
            Duration::default()
        })
        .as_millis();
    u64::try_from(ms).unwrap_or(u64::MAX)
}

/// Claude Code production OAuth client ID.
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// OAuth token refresh endpoint.
// WHY: must match console.anthropic.com, not platform.claude.com
const OAUTH_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";

/// Refresh when token has less than this many seconds remaining.
/// Fallback default; runtime reads `CredentialConfig::refresh_threshold_secs`.
pub const REFRESH_THRESHOLD_SECS: u64 = 3600;

// INVARIANT: the credential module does not redeclare the leeway value.
// The alias imported above forces the OAuth expiry path to use the same
// definition as the JWT validation path.
const _: () = assert!(
    CLOCK_SKEW_LEEWAY_SECS == crate::jwt::DEFAULT_CLOCK_SKEW_LEEWAY_SECS,
    "CLOCK_SKEW_LEEWAY_SECS must match DEFAULT_CLOCK_SKEW_LEEWAY_SECS to keep OAuth and JWT expiry checks consistent"
);

/// How often the background refresh task checks token expiry.
const REFRESH_CHECK_INTERVAL_SECS: u64 = 60;

/// How often to check file mtime for external changes.
const FILE_MTIME_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// OAuth token prefix used by Claude Code for OAuth access tokens.
const OAUTH_TOKEN_PREFIX: &str = "sk-ant-oat";

#[cfg(test)]
mod chain_tests;
#[cfg(test)]
mod claude_code_tests;
#[cfg(test)]
mod credential_file_tests;
#[cfg(test)]
mod env_provider_tests;
#[cfg(test)]
mod refreshing_tests;

#[cfg(test)]
mod clock_skew_tests {
    // WHY: regression for #5478. The OAuth expiry path must derive its
    // clock-skew leeway from the same canonical constant as the JWT path.
    #[test]
    fn credential_leeway_matches_jwt_canonical_value() {
        assert_eq!(
            super::CLOCK_SKEW_LEEWAY_SECS,
            crate::jwt::DEFAULT_CLOCK_SKEW_LEEWAY_SECS
        );
        assert_eq!(super::CLOCK_SKEW_LEEWAY_SECS, 30);
    }
}
