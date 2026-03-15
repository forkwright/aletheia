//! Credential provider implementations for LLM API key resolution.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use tracing::{Instrument, info, warn};

use aletheia_koina::credential::{Credential, CredentialProvider, CredentialSource};

/// Return current time as milliseconds since UNIX epoch, warning if the clock
/// is before epoch rather than silently returning zero.
#[expect(clippy::cast_possible_truncation, reason = "ms timestamps fit in u64")]
fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_else(|_| {
            tracing::warn!("system clock before UNIX epoch, using epoch as fallback");
            Duration::default()
        })
        .as_millis() as u64
}

/// Claude Code production OAuth client ID.
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// OAuth token refresh endpoint.
// WHY: must match console.anthropic.com, not platform.claude.com
const OAUTH_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";

/// Refresh when token has less than this many seconds remaining.
const REFRESH_THRESHOLD_SECS: u64 = 3600;

/// How often the background refresh task checks token expiry.
const REFRESH_CHECK_INTERVAL_SECS: u64 = 60;

/// How often to check file mtime for external changes.
const FILE_MTIME_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// On-disk credential file format.
///
/// Accepts both `"token"` (native format) and `"accessToken"` (Claude Code OAuth
/// output) for backward compatibility. Serialization always writes `"token"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialFile {
    /// Access token (API key or OAuth access token).
    #[serde(alias = "accessToken")]
    pub token: String,
    /// OAuth refresh token (absent for static API keys).
    #[serde(rename = "refreshToken", skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Token expiry as milliseconds since epoch.
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// OAuth scopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    /// Subscription tier.
    #[serde(rename = "subscriptionType", skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
}

impl CredentialFile {
    /// Read and parse a credential file.
    ///
    /// Accepts two on-disk layouts:
    ///
    /// * **Flat** — `{"token": "...", "refreshToken": "..."}` (native) or with the
    ///   `"accessToken"` alias produced by older Claude Code versions.
    /// * **Wrapped** — `{"claudeAiOauth": {"accessToken": "...", ...}}` — the nested
    ///   format written by current Claude Code releases.
    ///
    /// WHY: Claude Code changed its `.credentials.json` layout to nest all OAuth fields
    /// under a `claudeAiOauth` top-level key. Without unwrapping it, fresh credentials
    /// are invisible and the chain falls back to a stale env-var token.
    pub fn load(path: &Path) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;

        // Try flat format first (native "token" field or "accessToken" alias).
        if let Ok(cred) = serde_json::from_str::<Self>(&contents) {
            return Some(cred);
        }

        // Try claudeAiOauth wrapper format written by current Claude Code releases.
        let outer: serde_json::Value = serde_json::from_str(&contents).ok()?;
        serde_json::from_value(outer.get("claudeAiOauth")?.clone()).ok()
    }

    /// Write the credential file atomically (write to temp, rename).
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut file = std::fs::File::create(&tmp)?;
        serde_json::to_writer_pretty(&mut file, self).map_err(std::io::Error::other)?;
        file.flush()?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Whether this credential has a refresh token (OAuth flow).
    #[must_use]
    pub fn has_refresh_token(&self) -> bool {
        self.refresh_token.as_ref().is_some_and(|t| !t.is_empty())
    }

    /// Seconds remaining until token expires. Returns `None` if no expiry set.
    #[must_use]
    #[expect(
        clippy::cast_possible_wrap,
        reason = "ms timestamps fit in i64 until year 292M"
    )]
    pub fn seconds_remaining(&self) -> Option<i64> {
        let expires_at_ms = self.expires_at?;
        let now_ms = unix_epoch_ms();
        Some((expires_at_ms as i64 - now_ms as i64) / 1000)
    }

    /// Whether the token needs refresh (expired or within threshold).
    #[must_use]
    #[expect(clippy::cast_possible_wrap, reason = "threshold constant fits in i64")]
    #[expect(dead_code, reason = "credential internal; no caller yet")]
    pub(crate) fn needs_refresh(&self) -> bool {
        match self.seconds_remaining() {
            Some(remaining) => remaining < REFRESH_THRESHOLD_SECS as i64,
            None => false,
        }
    }
}

#[derive(Deserialize)]
struct OAuthResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
    scope: Option<String>,
}

impl std::fmt::Debug for OAuthResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthResponse")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish()
    }
}

fn default_expires_in() -> u64 {
    28800 // 8 hours
}

/// OAuth token prefix used by Claude Code for OAuth access tokens.
const OAUTH_TOKEN_PREFIX: &str = "sk-ant-oat";

/// Decode a base64url-encoded string (no padding required) into raw bytes.
///
/// WHY: extracts JWT payload segments to read `exp` claims without pulling in a
/// dedicated crate for this ~30-line function. Base64url differs from standard
/// Base64 only in the `+`/`-` and `/`/`_` substitutions and the omission of `=` padding.
fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    /// Map a single base64url character to its 6-bit value.
    fn char_val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'-' | b'+' => Some(62),
            b'_' | b'/' => Some(63),
            b'=' => Some(0), // padding — treated as zero bits
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    // Strip trailing padding before computing output length.
    let end = bytes.iter().rposition(|&b| b != b'=').map_or(0, |i| i + 1);
    let bytes = &bytes[..end];

    let mut out = Vec::with_capacity(bytes.len() * 6 / 8 + 1);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in bytes {
        let v = char_val(b)?;
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            // SAFETY: bits is 0-7 after decrement, so buf >> bits yields a value
            // whose lowest 8 bits are the decoded byte; upper bits are stripped.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "bits is 0-7 so buf >> bits fits in u8; upper bits are overflow from accumulation"
            )]
            out.push((buf >> bits) as u8);
        }
    }

    Some(out)
}

/// Attempt to extract the `exp` (expiry, seconds since epoch) claim from a
/// dot-segmented token without verifying its signature.
///
/// WHY: OAuth access tokens stored in env vars carry no separate expiry metadata;
/// reading the `exp` claim embedded in the token's payload segment is the only
/// non-network way to detect a stale token and allow fallthrough to a refreshable
/// file-based provider.
///
/// NOTE: signature is intentionally not verified — only the expiry claim is read.
/// Returns `None` when the token has no recognisable payload segment or no `exp`
/// field; the caller must treat `None` as "expiry unknown" (do not fall through).
fn decode_jwt_exp_secs(token: &str) -> Option<u64> {
    // Dot-segmented format: ignore the first segment (vendor prefix or JWT header)
    // and decode the second segment as a JSON object containing the exp claim.
    let mut segs = token.splitn(4, '.');
    let _first = segs.next()?;
    let payload_b64 = segs.next()?;

    let payload = base64url_decode(payload_b64)?;
    let value: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    // exp is stored as a u64 integer (seconds since epoch) per the JWT spec (RFC 7519).
    value.get("exp").and_then(serde_json::Value::as_u64)
}

/// Reads a credential from an environment variable.
///
/// Automatically detects OAuth tokens by the `sk-ant-oat` prefix and
/// returns [`CredentialSource::OAuth`] so callers use `Bearer` auth.
pub struct EnvCredentialProvider {
    var_name: String,
    /// Force the credential source (e.g. OAuth for `ANTHROPIC_AUTH_TOKEN`).
    force_source: Option<CredentialSource>,
}

impl EnvCredentialProvider {
    #[must_use]
    pub fn new(var_name: impl Into<String>) -> Self {
        Self {
            var_name: var_name.into(),
            force_source: None,
        }
    }

    /// Create a provider that always returns the given source type.
    #[must_use]
    pub fn with_source(var_name: impl Into<String>, source: CredentialSource) -> Self {
        Self {
            var_name: var_name.into(),
            force_source: Some(source),
        }
    }
}

impl CredentialProvider for EnvCredentialProvider {
    fn get_credential(&self) -> Option<Credential> {
        std::env::var(&self.var_name).ok().and_then(|v| {
            if v.is_empty() {
                return None;
            }

            // When the env var holds an OAuth access token, check whether it has
            // an embedded expiry claim. If the token appears expired, fall through
            // to the next provider — typically a file-based provider with a live
            // refresh token — rather than blocking the chain with a stale credential.
            // WHY: static env var tokens cannot be refreshed; a refreshable file
            // provider downstream must get a chance to supply a valid credential.
            if v.starts_with(OAUTH_TOKEN_PREFIX)
                && let Some(exp_secs) = decode_jwt_exp_secs(&v)
            {
                let now_secs = unix_epoch_ms() / 1000;
                if exp_secs < now_secs {
                    warn!(
                        var = %self.var_name,
                        "OAuth token from environment variable appears expired \
                         — falling through to next provider"
                    );
                    return None;
                }
            }

            let source = self.force_source.clone().unwrap_or_else(|| {
                if v.starts_with(OAUTH_TOKEN_PREFIX) {
                    CredentialSource::OAuth
                } else {
                    CredentialSource::Environment
                }
            });
            Some(Credential { secret: v, source })
        })
    }

    fn name(&self) -> &str {
        &self.var_name
    }
}

struct CachedFile {
    token: String,
    mtime: SystemTime,
    checked_at: Instant,
}

/// Reads a credential from a JSON file on disk.
pub struct FileCredentialProvider {
    path: PathBuf,
    cached: RwLock<Option<CachedFile>>,
}

impl FileCredentialProvider {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            cached: RwLock::new(None),
        }
    }

    /// The credential file path.
    #[must_use]
    #[expect(dead_code, reason = "credential internal; no caller yet")]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn current_mtime(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.path).ok()?.modified().ok()
    }

    fn reload(&self) -> Option<String> {
        let cred = CredentialFile::load(&self.path)?;
        let mtime = self.current_mtime().unwrap_or_else(|| {
            tracing::debug!(path = %self.path.display(), "could not read file mtime, using epoch");
            SystemTime::UNIX_EPOCH
        });

        let token = cred.token.clone();
        if let Ok(mut guard) = self.cached.write() {
            *guard = Some(CachedFile {
                token: cred.token,
                mtime,
                checked_at: Instant::now(),
            });
        }
        Some(token)
    }
}

impl CredentialProvider for FileCredentialProvider {
    fn get_credential(&self) -> Option<Credential> {
        if let Ok(guard) = self.cached.read()
            && let Some(cached) = guard.as_ref()
        {
            if cached.checked_at.elapsed() < FILE_MTIME_CHECK_INTERVAL {
                return Some(Credential {
                    secret: cached.token.clone(),
                    source: CredentialSource::File,
                });
            }
            if let Some(mtime) = self.current_mtime()
                && mtime == cached.mtime
            {
                drop(guard);
                if let Ok(mut w) = self.cached.write()
                    && let Some(ref mut c) = *w
                {
                    c.checked_at = Instant::now();
                }
                if let Ok(g) = self.cached.read()
                    && let Some(c) = g.as_ref()
                {
                    return Some(Credential {
                        secret: c.token.clone(),
                        source: CredentialSource::File,
                    });
                }
            }
        }

        self.reload().map(|token| Credential {
            secret: token,
            source: CredentialSource::File,
        })
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait requires &str return"
    )]
    fn name(&self) -> &str {
        "file"
    }
}

struct RefreshState {
    current_token: String,
    refresh_token: String,
    expires_at_ms: u64,
}

/// Wraps a credential file with background OAuth token refresh.
pub struct RefreshingCredentialProvider {
    state: Arc<RwLock<Option<RefreshState>>>,
    file_provider: FileCredentialProvider,
    shutdown: Arc<AtomicBool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl RefreshingCredentialProvider {
    /// Create a refreshing provider from a credential file path.
    ///
    /// Reads the credential file immediately and spawns a background refresh
    /// task. Requires a tokio runtime to be active.
    pub fn new(path: PathBuf) -> Option<Self> {
        let cred = CredentialFile::load(&path)?;
        let refresh_token = cred.refresh_token.clone().filter(|t| !t.is_empty())?;

        let state = Arc::new(RwLock::new(Some(RefreshState {
            current_token: cred.token.clone(),
            refresh_token,
            expires_at_ms: cred.expires_at.unwrap_or_else(|| {
                warn!(
                    "credential has no expiry, treating as immediately expired to trigger refresh"
                );
                0
            }),
        })));

        let shutdown = Arc::new(AtomicBool::new(false));

        let task_state = Arc::clone(&state);
        let task_shutdown = Arc::clone(&shutdown);
        let task_path = path.clone();

        let task = tokio::spawn(
            async move {
                refresh_loop(task_state, task_shutdown, task_path).await;
            }
            .instrument(tracing::info_span!("credential_refresh")),
        );

        Some(Self {
            state,
            file_provider: FileCredentialProvider::new(path),
            shutdown,
            task: Some(task),
        })
    }

    /// Signal the background refresh task to stop.
    #[expect(dead_code, reason = "credential internal; no caller yet")]
    pub(crate) fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl CredentialProvider for RefreshingCredentialProvider {
    fn get_credential(&self) -> Option<Credential> {
        if let Ok(guard) = self.state.read()
            && let Some(ref s) = *guard
            && !s.current_token.is_empty()
        {
            return Some(Credential {
                secret: s.current_token.clone(),
                source: CredentialSource::OAuth,
            });
        }
        self.file_provider.get_credential()
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait requires &str return"
    )]
    fn name(&self) -> &str {
        "oauth"
    }
}

impl Drop for RefreshingCredentialProvider {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn refresh_loop(
    state: Arc<RwLock<Option<RefreshState>>>,
    shutdown: Arc<AtomicBool>,
    path: PathBuf,
) {
    let client = reqwest::Client::new();
    let check_interval = Duration::from_secs(REFRESH_CHECK_INTERVAL_SECS);

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        tokio::time::sleep(check_interval).await;

        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let (refresh_token, needs_refresh) = {
            let Ok(guard) = state.read() else {
                continue;
            };
            let Some(s) = guard.as_ref() else {
                continue;
            };
            let now_ms = unix_epoch_ms();
            #[expect(clippy::cast_possible_wrap, reason = "ms timestamps fit in i64")]
            let remaining_secs = (s.expires_at_ms as i64 - now_ms as i64) / 1000;
            #[expect(clippy::cast_possible_wrap, reason = "threshold constant fits in i64")]
            let needs = remaining_secs < REFRESH_THRESHOLD_SECS as i64;
            (s.refresh_token.clone(), needs)
        };

        if !needs_refresh {
            continue;
        }

        info!("credential refresh needed — refreshing OAuth token");

        match do_refresh(&client, &refresh_token).await {
            Some(resp) => {
                let now_ms = unix_epoch_ms();
                let expires_at_ms = now_ms + resp.expires_in * 1000;

                if let Ok(mut guard) = state.write() {
                    *guard = Some(RefreshState {
                        current_token: resp.access_token.clone(),
                        refresh_token: resp.refresh_token.clone(),
                        expires_at_ms,
                    });
                }

                let scopes = resp
                    .scope
                    .map(|s| s.split_whitespace().map(String::from).collect());
                let cred_file = CredentialFile {
                    token: resp.access_token,
                    refresh_token: Some(resp.refresh_token),
                    expires_at: Some(expires_at_ms),
                    scopes,
                    subscription_type: None,
                };
                if let Err(e) = cred_file.save(&path) {
                    warn!(error = %e, "failed to write refreshed credential file");
                }

                info!(expires_in_secs = resp.expires_in, "OAuth token refreshed");
            }
            None => {
                warn!("OAuth token refresh failed — will retry next cycle");
            }
        }
    }
}

async fn do_refresh(client: &reqwest::Client, refresh_token: &str) -> Option<OAuthResponse> {
    // WHY: Anthropic OAuth endpoint expects form-urlencoded, not JSON
    let body = format!(
        "grant_type=refresh_token&refresh_token={refresh_token}&client_id={OAUTH_CLIENT_ID}",
    );

    let resp = client
        .post(OAUTH_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| {
            warn!(error = %e, "OAuth refresh request failed");
            e
        })
        .ok()?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_else(|e| {
            warn!("failed to read OAuth error response body: {e}");
            String::new()
        });
        warn!(status = %status, body = %body, "OAuth refresh returned error");
        return None;
    }

    resp.json::<OAuthResponse>()
        .await
        .map_err(|e| {
            warn!(error = %e, "failed to parse OAuth refresh response");
            e
        })
        .ok()
}

/// Force a one-shot OAuth token refresh (for CLI `credential refresh`).
pub async fn force_refresh(path: &Path) -> Result<CredentialFile, String> {
    let cred = CredentialFile::load(path)
        .ok_or_else(|| format!("cannot read credential file: {}", path.display()))?;

    let refresh_token = cred
        .refresh_token
        .as_ref()
        .filter(|t| !t.is_empty())
        .ok_or("no refresh token in credential file")?;

    let client = reqwest::Client::new();
    let resp = do_refresh(&client, refresh_token)
        .await
        .ok_or("OAuth refresh failed")?;

    let now_ms = unix_epoch_ms();
    let expires_at_ms = now_ms + resp.expires_in * 1000;

    let scopes = resp
        .scope
        .map(|s| s.split_whitespace().map(String::from).collect());
    let updated = CredentialFile {
        token: resp.access_token,
        refresh_token: Some(resp.refresh_token),
        expires_at: Some(expires_at_ms),
        scopes,
        subscription_type: cred.subscription_type,
    };
    updated
        .save(path)
        .map_err(|e| format!("failed to write credential file: {e}"))?;

    Ok(updated)
}

/// Default path to the Claude Code credentials file.
///
/// Returns `~/.claude/.credentials.json`, resolving `~` via `$HOME`.
/// Returns `None` if `$HOME` is not set.
#[must_use]
pub fn claude_code_default_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".claude")
            .join(".credentials.json")
    })
}

/// Build a credential provider from a Claude Code credentials file.
///
/// If the file contains a refresh token, returns a [`RefreshingCredentialProvider`]
/// that keeps the token fresh in the background. Otherwise returns a
/// [`FileCredentialProvider`] for static token reads.
///
/// Returns `None` if the file does not exist or cannot be parsed.
pub fn claude_code_provider(path: &Path) -> Option<Box<dyn CredentialProvider>> {
    if !path.exists() {
        return None;
    }
    let cred = CredentialFile::load(path)?;
    if cred.has_refresh_token()
        && let Some(refreshing) = RefreshingCredentialProvider::new(path.to_path_buf())
    {
        info!(
            path = %path.display(),
            "Claude Code credentials found (OAuth auto-refresh)"
        );
        return Some(Box::new(refreshing));
    }
    info!(
        path = %path.display(),
        "Claude Code credentials found (static token)"
    );
    Some(Box::new(FileCredentialProvider::new(path.to_path_buf())))
}

/// Ordered list of credential providers. First to return `Some` wins.
pub struct CredentialChain {
    providers: Vec<Box<dyn CredentialProvider>>,
    resolved_name: RwLock<String>,
}

impl CredentialChain {
    #[must_use]
    pub fn new(providers: Vec<Box<dyn CredentialProvider>>) -> Self {
        Self {
            providers,
            resolved_name: RwLock::new("chain".to_owned()),
        }
    }
}

impl CredentialProvider for CredentialChain {
    fn get_credential(&self) -> Option<Credential> {
        for provider in &self.providers {
            if let Some(cred) = provider.get_credential() {
                if let Ok(mut name) = self.resolved_name.write() {
                    provider.name().clone_into(&mut *name);
                }
                return Some(cred);
            }
        }
        None
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait requires &str return"
    )]
    fn name(&self) -> &str {
        "chain"
    }
}

#[cfg(test)]
#[path = "credential_tests.rs"]
mod tests;
