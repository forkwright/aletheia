//! Credential provider implementations for LLM API key resolution.

mod file_ops;
mod providers;
mod refresh;

use std::time::{Duration, SystemTime};

pub use file_ops::CredentialFile;
pub use providers::{CredentialChain, EnvCredentialProvider, FileCredentialProvider};
pub use refresh::{
    RefreshingCredentialProvider, claude_code_default_path, claude_code_provider,
    claude_code_provider_with_config, force_refresh,
};

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
const REFRESH_THRESHOLD_SECS: u64 = 3600;

/// Clock skew tolerance for token expiry checks (seconds).
///
/// WHY: clock differences between the OAuth provider and local system
/// cause freshly obtained tokens to appear expired. 30 seconds is
/// conservative enough to catch genuine expiry while tolerating
/// typical NTP drift.
const CLOCK_SKEW_LEEWAY_SECS: u64 = 30;

/// How often the background refresh task checks token expiry.
const REFRESH_CHECK_INTERVAL_SECS: u64 = 60;

/// How often to check file mtime for external changes.
const FILE_MTIME_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// OAuth token prefix used by Claude Code for OAuth access tokens.
const OAUTH_TOKEN_PREFIX: &str = "sk-ant-oat";

#[cfg(test)]
#[path = "../credential_tests.rs"]
mod tests;
