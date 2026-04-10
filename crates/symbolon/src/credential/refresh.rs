//! OAuth token refresh: background loop and one-shot force-refresh.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, warn};
use zeroize::Zeroize;

use aletheia_koina::credential::{Credential, CredentialProvider, CredentialSource};
use aletheia_koina::secret::SecretString;
use aletheia_koina::system::{Environment, RealSystem};

use super::file_ops::CredentialFile;
use super::providers::FileCredentialProvider;
use super::{
    OAUTH_CLIENT_ID, OAUTH_TOKEN_URL, REFRESH_CHECK_INTERVAL_SECS, REFRESH_THRESHOLD_SECS,
    unix_epoch_ms,
};
use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};

/// OAuth error response from the token endpoint.
#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Outcome of an OAuth refresh attempt.
pub(super) enum RefreshOutcome {
    /// Refresh succeeded.
    Success(OAuthResponse),
    /// Refresh token is permanently invalid (e.g. `invalid_grant`).
    /// Retrying will never succeed — the user must re-authenticate.
    InvalidGrant,
    /// Transient failure (network, server error, etc.). Safe to retry.
    TransientError,
}

/// Minimum `expires_in` accepted from OAuth responses (seconds).
const MIN_EXPIRES_IN_SECS: u64 = 60;

/// Maximum `expires_in` accepted from OAuth responses (seconds).
const MAX_EXPIRES_IN_SECS: u64 = 86400;

#[derive(Deserialize)]
pub(super) struct OAuthResponse {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    #[serde(default = "default_expires_in")]
    pub expires_in: u64,
    pub scope: Option<String>,
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
    28800 // NOTE: 8 hours
}

/// Clamp `expires_in` to [`MIN_EXPIRES_IN_SECS`, `MAX_EXPIRES_IN_SECS`].
///
/// // WHY: A zero or negative value from a buggy OAuth server causes infinite
/// // refresh loops (immediate re-trigger). An absurdly large value delays
/// // legitimate re-auth after revocation. Clamping bounds the behavior.
pub(super) fn clamp_expires_in(raw: u64) -> u64 {
    let clamped = raw.clamp(MIN_EXPIRES_IN_SECS, MAX_EXPIRES_IN_SECS);
    if clamped != raw {
        warn!(
            raw_expires_in = raw,
            clamped_expires_in = clamped,
            "OAuth expires_in outside [{MIN_EXPIRES_IN_SECS}, {MAX_EXPIRES_IN_SECS}], clamped"
        );
    }
    clamped
}

pub(super) struct RefreshState {
    pub current_token: SecretString,
    pub refresh_token: SecretString,
    pub expires_at_ms: u64,
    pub subscription_type: Option<String>,
}

/// Wraps a credential file with background OAuth token refresh.
///
/// Cleanup is registered at construction time via [`CleanupRegistry`](aletheia_koina::cleanup::CleanupRegistry): the
/// background task is cancelled and aborted when the provider is dropped,
/// regardless of whether the drop occurs during normal execution, early
/// error return, or panic unwind.
///
/// // WHY: `RwLock` allows concurrent readers (`get_credential` calls) with a
/// // single writer (the background refresh task). This avoids blocking
/// // LLM requests during token refresh, which may take 100-500ms.
pub struct RefreshingCredentialProvider {
    /// Current OAuth token and refresh metadata. `None` after a fatal
    /// refresh failure. Writers: the background refresh task (exclusive).
    /// Readers: `get_credential()` on any thread.
    state: Arc<RwLock<Option<RefreshState>>>,
    file_provider: FileCredentialProvider,
    shutdown: CancellationToken,
    /// Cleanup registered at task spawn time; fires on drop (LIFO order).
    _cleanup: aletheia_koina::cleanup::CleanupRegistry,
}

impl RefreshingCredentialProvider {
    /// Create a refreshing provider from a credential file path.
    ///
    /// Reads the credential file immediately and spawns a background refresh
    /// task with a default circuit breaker. Requires a tokio runtime to be active.
    #[must_use]
    pub fn new(path: PathBuf) -> Option<Self> {
        Self::with_circuit_breaker(path, CircuitBreakerConfig::default())
    }

    /// Create a refreshing provider with a custom circuit breaker configuration.
    ///
    /// Reads the credential file immediately and spawns a background refresh
    /// task. Requires a tokio runtime to be active.
    pub(crate) fn with_circuit_breaker(
        path: PathBuf,
        cb_config: CircuitBreakerConfig,
    ) -> Option<Self> {
        let cred = CredentialFile::load(&path)?;
        let refresh_token = cred
            .refresh_token
            .clone()
            .filter(|t| !t.expose_secret().is_empty())?;

        let state = Arc::new(RwLock::new(Some(RefreshState {
            current_token: cred.token.clone(),
            refresh_token,
            expires_at_ms: cred.expires_at.unwrap_or_else(|| {
                warn!(
                    "credential has no expiry, treating as immediately expired to trigger refresh"
                );
                0
            }),
            subscription_type: cred.subscription_type,
        })));

        let shutdown = CancellationToken::new();
        let circuit_breaker = Arc::new(CircuitBreaker::new(cb_config));

        let task_state = Arc::clone(&state);
        let task_shutdown = shutdown.clone();
        let task_path = path.clone();
        let task_cb = Arc::clone(&circuit_breaker);

        let task = tokio::spawn(
            async move {
                refresh_loop(task_state, task_shutdown, task_path, task_cb).await;
            }
            .instrument(tracing::info_span!("credential_refresh")),
        );

        // WHY: Register cleanup at spawn time so the task is cancelled+aborted
        // on drop even if construction is only partially completed in the caller.
        let mut cleanup = aletheia_koina::cleanup::CleanupRegistry::new();
        let shutdown_for_cleanup = shutdown.clone();
        let abort_handle = task.abort_handle();
        cleanup.register(move || {
            shutdown_for_cleanup.cancel();
            abort_handle.abort();
        });

        Some(Self {
            state,
            file_provider: FileCredentialProvider::new(path),
            shutdown,
            _cleanup: cleanup,
        })
    }

    /// Signal the background refresh task to stop.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "called from tests; will be wired from server shutdown path"
        )
    )]
    pub(crate) fn shutdown(&self) {
        self.shutdown.cancel();
    }
}

impl CredentialProvider for RefreshingCredentialProvider {
    fn get_credential(&self) -> Option<Credential> {
        if let Ok(guard) = self.state.read()
            && let Some(ref s) = *guard
            && !s.current_token.expose_secret().is_empty()
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

// NOTE: No Drop impl — cleanup is registered at setup time via CleanupRegistry.
// The registry fires its callbacks (cancel token + abort task) on drop.

/// Track last-observed mtime for file change detection.
struct FileMtimeTracker {
    last_mtime: Option<SystemTime>,
}

impl FileMtimeTracker {
    fn new(path: &Path) -> Self {
        let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
        Self { last_mtime: mtime }
    }

    /// Returns `true` if the file's mtime has changed since last check.
    fn has_changed(&mut self, path: &Path) -> bool {
        let current = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
        if current == self.last_mtime {
            return false;
        }
        self.last_mtime = current;
        true
    }
}

/// Reload credentials from file when circuit is open and file has changed.
fn try_reload_from_file(
    state: &RwLock<Option<RefreshState>>,
    path: &Path,
    circuit_breaker: &CircuitBreaker,
) {
    let Some(file_cred) = CredentialFile::load(path) else {
        return;
    };
    info!("credential file changed externally while circuit open, reloading");
    if let Ok(mut guard) = state.write() {
        *guard = Some(RefreshState {
            current_token: file_cred.token,
            refresh_token: file_cred.refresh_token.unwrap_or_else(|| {
                guard
                    .as_ref()
                    .map_or_else(|| SecretString::from(""), |s| s.refresh_token.clone())
            }),
            expires_at_ms: file_cred.expires_at.unwrap_or(0),
            subscription_type: file_cred.subscription_type,
        });
    }
    circuit_breaker.reset();
}

/// Build the post-refresh state, adopting the on-disk credential if newer.
fn resolve_post_refresh_state(
    path: &Path,
    resp: OAuthResponse,
    new_expires_at_ms: u64,
    subscription_type: Option<String>,
) -> RefreshState {
    if let Some(on_disk) = CredentialFile::load(path)
        && on_disk.expires_at.unwrap_or(0) > new_expires_at_ms
    {
        info!(
            our_expiry = new_expires_at_ms,
            file_expiry = on_disk.expires_at.unwrap_or(0),
            "file has newer credential than our refresh, adopting"
        );
        RefreshState {
            current_token: on_disk.token,
            refresh_token: on_disk.refresh_token.unwrap_or(resp.refresh_token),
            expires_at_ms: on_disk.expires_at.unwrap_or(new_expires_at_ms),
            subscription_type: on_disk.subscription_type,
        }
    } else {
        RefreshState {
            current_token: resp.access_token,
            refresh_token: resp.refresh_token,
            expires_at_ms: new_expires_at_ms,
            subscription_type,
        }
    }
}

/// Persist a successful refresh response and update in-memory state.
fn persist_refresh_success(
    state: &Arc<RwLock<Option<RefreshState>>>,
    path: &Path,
    mtime_tracker: &mut FileMtimeTracker,
    circuit_breaker: &CircuitBreaker,
    resp: OAuthResponse,
    subscription_type: Option<String>,
) {
    circuit_breaker.record_success();
    let expires_in = clamp_expires_in(resp.expires_in);
    let new_expires_at_ms = unix_epoch_ms() + expires_in * 1000;

    let scopes = resp
        .scope
        .as_deref()
        .map(|s| s.split_whitespace().map(String::from).collect());
    let cred_file = CredentialFile {
        token: resp.access_token.clone(),
        refresh_token: Some(resp.refresh_token.clone()),
        expires_at: Some(new_expires_at_ms),
        scopes,
        subscription_type: subscription_type.clone(),
    };

    match cred_file.save(path) {
        Ok(()) => {
            mtime_tracker.has_changed(path);
            let final_state =
                resolve_post_refresh_state(path, resp, new_expires_at_ms, subscription_type);
            if let Ok(mut guard) = state.write() {
                *guard = Some(final_state);
            }
            crate::metrics::record_token_refresh(true);
            info!(expires_in_secs = expires_in, "OAuth token refreshed");
        }
        Err(e) => {
            error!(error = %e, "failed to write refreshed credential file, keeping previous in-memory token");
            crate::metrics::record_credential_write_failure();
            crate::metrics::record_token_refresh(true);
        }
    }
}

/// Read current in-memory state and decide whether a refresh is due.
///
/// Returns `None` when no refresh is required (no state, stale lock, or still
/// inside the refresh window). Returns `Some((refresh_token, subscription_type,
/// expires_at_ms))` when the caller should perform a refresh.
fn plan_refresh(
    state: &Arc<RwLock<Option<RefreshState>>>,
) -> Option<(String, Option<String>, u64)> {
    let guard = state.read().ok()?;
    let s = guard.as_ref()?;
    let now_ms = unix_epoch_ms();
    let expires_i64 = i64::try_from(s.expires_at_ms).unwrap_or(i64::MAX);
    let now_i64 = i64::try_from(now_ms).unwrap_or(i64::MAX);
    let remaining_secs = (expires_i64 - now_i64) / 1000;
    let threshold = i64::try_from(REFRESH_THRESHOLD_SECS).unwrap_or(i64::MAX);
    if remaining_secs >= threshold {
        return None;
    }
    Some((
        s.refresh_token.expose_secret().to_owned(),
        s.subscription_type.clone(),
        s.expires_at_ms,
    ))
}

async fn refresh_loop(
    state: Arc<RwLock<Option<RefreshState>>>,
    shutdown: CancellationToken,
    path: PathBuf,
    circuit_breaker: Arc<CircuitBreaker>,
) {
    let client = reqwest::Client::new();
    let check_interval = Duration::from_secs(REFRESH_CHECK_INTERVAL_SECS);
    let mut mtime_tracker = FileMtimeTracker::new(&path);

    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                info!("credential refresh loop shutting down");
                break;
            }
            () = tokio::time::sleep(check_interval) => {}
        }

        // When circuit is open, poll file for external credential updates.
        // This allows manual credential fixes (e.g., `aletheia auth login`)
        // to take effect without restarting the refresh loop.
        if !circuit_breaker.check_allowed() {
            if mtime_tracker.has_changed(&path) {
                try_reload_from_file(&state, &path, &circuit_breaker);
            } else {
                debug!(
                    state = %circuit_breaker.state(),
                    "OAuth refresh circuit breaker is open, skipping refresh attempt"
                );
            }
            continue;
        }

        let Some((mut refresh_token_value, subscription_type, expires_at_ms)) = plan_refresh(&state)
        else {
            continue;
        };

        info!(
            expires_at_ms,
            now_ms = unix_epoch_ms(),
            "credential refresh needed"
        );

        let refresh_result = do_refresh(&client, &refresh_token_value).await;
        // SAFETY: The refresh token is zeroized immediately after use to
        // limit the window for memory disclosure attacks. The token is
        // still in the OAuthResponse if we need to persist it to disk.
        refresh_token_value.zeroize();
        match refresh_result {
            RefreshOutcome::Success(resp) => {
                persist_refresh_success(
                    &state,
                    &path,
                    &mut mtime_tracker,
                    &circuit_breaker,
                    resp,
                    subscription_type,
                );
            }
            RefreshOutcome::InvalidGrant => {
                // WHY: invalid_grant is permanent — the refresh token has been
                // revoked, expired, or was never valid. Continuing to retry wastes
                // resources and floods logs. Clear the in-memory state so callers
                // fall through to file-based providers, and stop the refresh loop.
                if let Ok(mut guard) = state.write() {
                    *guard = None;
                }
                crate::metrics::record_token_refresh(false);
                error!(
                    "OAuth refresh loop stopping: refresh token is permanently invalid. \
                     Re-authenticate to resume automatic token refresh."
                );
                break;
            }
            RefreshOutcome::TransientError => {
                circuit_breaker.record_failure();
                crate::metrics::record_token_refresh(false);
                warn!("OAuth token refresh failed, will retry next cycle");
            }
        }
    }
}

pub(super) async fn do_refresh(client: &reqwest::Client, refresh_token: &str) -> RefreshOutcome {
    // NOTE: Anthropic OAuth endpoint expects form-urlencoded, not JSON.
    // This matches RFC 6749 Section 4.1.3 for token refresh requests.
    let body = format!(
        "grant_type=refresh_token&refresh_token={refresh_token}&client_id={OAUTH_CLIENT_ID}",
    );

    let resp = match client
        .post(OAUTH_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "OAuth refresh request failed");
            return RefreshOutcome::TransientError;
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_else(|e| {
            warn!("failed to read OAuth error response body: {e}");
            String::new()
        });

        // `invalid_grant` means the refresh token is revoked, expired, or
        // otherwise permanently invalid. Retrying will never succeed — the user
        // must re-authenticate to obtain a new refresh token.
        if let Ok(err_resp) = serde_json::from_str::<OAuthErrorResponse>(&body_text)
            && err_resp.error == "invalid_grant"
        {
            error!(
                status = %status,
                error = %err_resp.error,
                description = err_resp.error_description.as_deref().unwrap_or(""),
                "OAuth refresh token is invalid — re-authentication required. \
                 Run `aletheia auth login` or re-authorize via Claude Code to obtain \
                 a new refresh token."
            );
            return RefreshOutcome::InvalidGrant;
        }

        warn!(status = %status, body = %body_text, "OAuth refresh returned error");
        return RefreshOutcome::TransientError;
    }

    match resp.json::<OAuthResponse>().await {
        Ok(oauth) => RefreshOutcome::Success(oauth),
        Err(e) => {
            warn!(error = %e, "failed to parse OAuth refresh response");
            RefreshOutcome::TransientError
        }
    }
}

/// Force a one-shot OAuth token refresh (for CLI `credential refresh`).
///
/// # Errors
///
/// Returns an error if the credential file cannot be read, contains no refresh
/// token, the OAuth refresh request fails, or the updated credentials cannot
/// be saved.
#[tracing::instrument(skip_all)]
#[must_use = "refreshed credentials must be used or persisted"]
pub async fn force_refresh(path: &Path) -> Result<CredentialFile, String> {
    let cred = CredentialFile::load(path)
        .ok_or_else(|| format!("cannot read credential file: {}", path.display()))?;

    let refresh_token = cred
        .refresh_token
        .as_ref()
        .filter(|t| !t.expose_secret().is_empty())
        .ok_or("no refresh token in credential file")?;

    let client = reqwest::Client::new();
    let resp = match do_refresh(&client, refresh_token.expose_secret()).await {
        RefreshOutcome::Success(r) => r,
        RefreshOutcome::InvalidGrant => {
            return Err(
                "OAuth refresh token is invalid (invalid_grant). Re-authenticate with \
                 `aletheia auth login` or re-authorize via Claude Code."
                    .to_owned(),
            );
        }
        RefreshOutcome::TransientError => {
            return Err("OAuth refresh failed (transient error)".to_owned());
        }
    };

    let expires_in = clamp_expires_in(resp.expires_in);
    let now_ms = unix_epoch_ms();
    let expires_at_ms = now_ms + expires_in * 1000;

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
    RealSystem.var_os("HOME").map(|home| {
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
#[must_use]
pub fn claude_code_provider(path: &Path) -> Option<Box<dyn CredentialProvider>> {
    claude_code_provider_with_config(path, CircuitBreakerConfig::default())
}

/// Build a credential provider with a custom circuit breaker configuration.
///
/// See [`claude_code_provider`] for behavior details.
pub fn claude_code_provider_with_config(
    path: &Path,
    cb_config: CircuitBreakerConfig,
) -> Option<Box<dyn CredentialProvider>> {
    if !path.exists() {
        return None;
    }
    let cred = CredentialFile::load(path)?;
    if cred.has_refresh_token()
        && let Some(refreshing) =
            RefreshingCredentialProvider::with_circuit_breaker(path.to_path_buf(), cb_config)
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
