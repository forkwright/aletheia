//! OAuth token refresh: background loop and one-shot force-refresh.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info, warn};
use zeroize::Zeroize;

use koina::credential::{Credential, CredentialProvider, CredentialSource};
use koina::secret::SecretString;
use koina::system::{Environment, RealSystem};

use super::file_ops::CredentialFile;
use super::oauth_types::{OAuthErrorResponse, OAuthTokenResponse};
use super::providers::FileCredentialProvider;
use super::{OAUTH_CLIENT_ID, OAUTH_TOKEN_URL, REFRESH_CHECK_INTERVAL_SECS, unix_epoch_ms};
use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};

const CLAUDE_CODE_CREDS_ENV: &str = "CLAUDE_CODE_CREDS";

/// Outcome of an OAuth refresh attempt.
pub(super) enum RefreshOutcome {
    /// Refresh succeeded.
    Success {
        access_token: SecretString,
        refresh_token: SecretString,
        expires_in: u64,
        scope: Option<String>,
    },
    /// Refresh token is permanently invalid (e.g. `invalid_grant`).
    /// Retrying will never succeed — the user must re-authenticate.
    InvalidGrant,
    /// Transient failure (network, server error, etc.). Safe to retry.
    TransientError,
}

/// Token data from a successful OAuth refresh.
///
/// WHY: bundled as a named struct so `persist_refresh_success` stays within
/// clippy's argument-count limit while keeping all fields named.
pub(super) struct RefreshSuccessPayload {
    access_token: SecretString,
    refresh_token: SecretString,
    expires_in: u64,
    scope: Option<String>,
    subscription_type: Option<String>,
}

impl RefreshSuccessPayload {
    fn new(
        access_token: SecretString,
        refresh_token: SecretString,
        expires_in: u64,
        scope: Option<String>,
        subscription_type: Option<String>,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_in,
            scope,
            subscription_type,
        }
    }
}

/// Minimum `expires_in` accepted from OAuth responses (seconds).
const MIN_EXPIRES_IN_SECS: u64 = 60;

/// Maximum `expires_in` accepted from OAuth responses (seconds).
const MAX_EXPIRES_IN_SECS: u64 = 86400;
const MAX_OAUTH_ERROR_PREVIEW_CHARS: usize = 160;

fn default_expires_in() -> u64 {
    28800 // NOTE: 8 hours
}

/// Clamp `expires_in` to [`MIN_EXPIRES_IN_SECS`, `MAX_EXPIRES_IN_SECS`].
///
/// A zero `expires_in` from a buggy OAuth server causes infinite refresh
/// loops (immediate re-trigger); an absurdly large value delays legitimate
/// re-auth after revocation. Clamping bounds both.
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

impl RefreshState {
    /// Returns `true` if the in-memory access token is non-empty and can be used.
    pub(super) fn has_token(&self) -> bool {
        !self.current_token.expose_secret().is_empty()
    }
}

/// Wraps a credential file with background OAuth token refresh.
///
/// Cleanup is registered at construction time via [`CleanupRegistry`](koina::cleanup::CleanupRegistry): the
/// background task is cancelled and aborted when the provider is dropped,
/// regardless of whether the drop occurs during normal execution, early
/// error return, or panic unwind.
///
/// The `RwLock` allows concurrent readers (`get_credential` calls) with a
/// single writer (the background refresh task), so LLM requests are not
/// blocked during a token refresh, which may take 100-500ms.
// kanon:ignore RUST/pub-visibility -- WHY: re-exported as symbolon::credential credential-provider API and constructed by the aletheia runtime.
pub struct RefreshingCredentialProvider {
    /// Current OAuth token and refresh metadata. `None` after a fatal
    /// refresh failure. Writers: the background refresh task (exclusive).
    /// Readers: `get_credential()` on any thread.
    state: Arc<RwLock<Option<RefreshState>>>,
    file_provider: FileCredentialProvider,
    shutdown: CancellationToken,
    /// Cleanup registered at task spawn time; fires on drop (LIFO order).
    _cleanup: koina::cleanup::CleanupRegistry,
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
        let mut cleanup = koina::cleanup::CleanupRegistry::new();
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
    ///
    /// Called automatically by [`Drop`] so application shutdown cancels the
    /// refresh loop eagerly; tests may also call it explicitly.
    fn signal_shutdown(&self) {
        self.shutdown.cancel();
    }
}

impl CredentialProvider for RefreshingCredentialProvider {
    fn shutdown(&self) {
        self.signal_shutdown();
    }

    fn get_credential(&self) -> Option<Credential> {
        if let Ok(guard) = self.state.read()
            && let Some(ref s) = *guard
            && s.has_token()
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
        // WHY: Eagerly cancel the background refresh loop when the provider is
        // dropped during application shutdown (or any other scope exit), so
        // in-flight OAuth requests are aborted before the runtime tears down.
        self.shutdown();
    }
}

// NOTE: Cleanup is also registered at setup time via CleanupRegistry as a
// defence-in-depth measure: the registry aborts the background task on drop
// even if construction is only partially completed in the caller.

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
    // SAFETY: logging reload event, not credential value
    info!("credential file changed externally while circuit open, reloading"); // kanon:ignore SECURITY/credential-logging -- logs reload event, not credential value
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
    access_token: SecretString,
    refresh_token: SecretString,
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
            refresh_token: on_disk.refresh_token.unwrap_or(refresh_token),
            expires_at_ms: on_disk.expires_at.unwrap_or(new_expires_at_ms),
            subscription_type: on_disk.subscription_type,
        }
    } else {
        RefreshState {
            current_token: access_token,
            refresh_token,
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
    payload: RefreshSuccessPayload,
) {
    circuit_breaker.record_success();
    let expires_in = clamp_expires_in(payload.expires_in);
    let new_expires_at_ms = unix_epoch_ms() + expires_in * 1000;

    let scopes = payload
        .scope
        .as_deref()
        .map(|s| s.split_whitespace().map(String::from).collect());
    let cred_file = CredentialFile {
        token: payload.access_token.clone(),
        refresh_token: Some(payload.refresh_token.clone()),
        expires_at: Some(new_expires_at_ms),
        scopes,
        subscription_type: payload.subscription_type.clone(),
    };

    match cred_file.save(path) {
        Ok(()) => {
            mtime_tracker.has_changed(path);
            let final_state = resolve_post_refresh_state(
                path,
                payload.access_token,
                payload.refresh_token,
                new_expires_at_ms,
                payload.subscription_type,
            );
            if let Ok(mut guard) = state.write() {
                *guard = Some(final_state);
            }
            crate::metrics::record_token_refresh(true);
            // SAFETY: logging expiry duration, not the token value
            info!(expires_in_secs = expires_in, "OAuth token refreshed"); // kanon:ignore SECURITY/credential-logging -- logs expiry duration, not the token
        }
        Err(e) => {
            // SAFETY: logging write-failure error, not the token value
            error!(error = %e, "failed to write refreshed credential file, keeping previous in-memory token"); // kanon:ignore SECURITY/credential-logging -- logs write-failure error, not the token
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

    // WHY: delegate the threshold decision to `CredentialFile::needs_refresh` so
    // the live refresh loop and the unit tests share exactly one copy of the
    // expiry-vs-threshold comparison.
    let probe = CredentialFile {
        token: s.current_token.clone(),
        refresh_token: Some(s.refresh_token.clone()),
        expires_at: Some(s.expires_at_ms),
        scopes: None,
        subscription_type: s.subscription_type.clone(),
    };
    if !probe.needs_refresh() {
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
                // SAFETY: logging shutdown event, not credential value
                info!("credential refresh loop shutting down"); // kanon:ignore SECURITY/credential-logging -- logs shutdown event, not credential value
                break;
            }
            () = tokio::time::sleep(check_interval) => {}
        }

        // WHY: while the circuit is open, poll the file for external updates so
        // manual credential fixes (e.g. `aletheia auth login`) take effect
        // without restarting the refresh loop.
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

        let Some((mut refresh_token_value, subscription_type, expires_at_ms)) =
            plan_refresh(&state)
        else {
            continue;
        };

        info!(
            expires_at_ms,
            now_ms = unix_epoch_ms(),
            "credential refresh needed"
        );

        let refresh_result = do_refresh(&client, &refresh_token_value, OAUTH_TOKEN_URL).await;
        // SAFETY: The refresh token is zeroized immediately after use to
        // limit the window for memory disclosure attacks. The token is
        // still in the OAuthTokenResponse if we need to persist it to disk.
        refresh_token_value.zeroize();
        match refresh_result {
            RefreshOutcome::Success {
                access_token,
                refresh_token,
                expires_in,
                scope,
            } => {
                persist_refresh_success(
                    &state,
                    &path,
                    &mut mtime_tracker,
                    &circuit_breaker,
                    RefreshSuccessPayload::new(
                        access_token,
                        refresh_token,
                        expires_in,
                        scope,
                        subscription_type,
                    ),
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
                // SAFETY: logging retry status, not the token value
                warn!("OAuth token refresh failed, will retry next cycle"); // kanon:ignore SECURITY/credential-logging -- logs retry status, not the token
            }
        }
    }
}

pub(super) async fn do_refresh(
    client: &reqwest::Client,
    refresh_token: &str,
    token_url: &str,
) -> RefreshOutcome {
    // NOTE: Anthropic OAuth endpoint expects form-urlencoded, not JSON.
    // This matches RFC 6749 Section 4.1.3 for token refresh requests.
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", OAUTH_CLIENT_ID),
    ];

    let resp = match client
        .post(token_url)
        .form(&form)
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
        let error_response = serde_json::from_str::<OAuthErrorResponse>(&body_text).ok();
        let error_code = error_response
            .as_ref()
            .map_or("unparseable_oauth_error", |err_resp| {
                err_resp.error.as_str()
            });
        let body_preview = oauth_error_body_preview(&body_text, error_response.as_ref());

        // WHY: `invalid_grant` means the refresh token is revoked, expired, or
        // otherwise permanently invalid; retrying will never succeed — the user
        // must re-authenticate to obtain a new refresh token.
        if error_code == "invalid_grant" {
            // WHY: error_description is provider-controlled and may contain account
            // identifiers or token fragments — log only normalized structured fields.
            error!(
                status = %status,
                oauth_error = %error_code,
                body_preview = %body_preview,
                "OAuth refresh token is invalid — re-authentication required. \
                 Run `aletheia auth login` or re-authorize via Claude Code to obtain \
                 a new refresh token."
            );
            return RefreshOutcome::InvalidGrant;
        }

        // WHY: raw body may contain provider-echoed request material; log only
        // structured status/code plus a sanitized bounded preview.
        warn!(
            status = %status,
            oauth_error = %error_code,
            body_preview = %body_preview,
            "OAuth refresh returned error"
        );
        return RefreshOutcome::TransientError;
    }

    match resp.json::<OAuthTokenResponse>().await {
        Ok(oauth) => {
            // WHY: the refresh contract expects a new refresh token; absence is
            // treated as a malformed response rather than silently reusing the
            // old one, matching the prior required-field behavior.
            let Some(refresh_token) = oauth.refresh_token else {
                warn!("OAuth refresh response missing refresh_token");
                return RefreshOutcome::TransientError;
            };
            RefreshOutcome::Success {
                access_token: oauth.access_token,
                refresh_token,
                expires_in: oauth.expires_in.unwrap_or_else(default_expires_in),
                scope: oauth.scope,
            }
        }
        Err(e) => {
            warn!(error = %e, "failed to parse OAuth refresh response");
            RefreshOutcome::TransientError
        }
    }
}

fn oauth_error_body_preview(
    body_text: &str,
    error_response: Option<&OAuthErrorResponse>,
) -> String {
    if let Some(error_response) = error_response {
        return truncate_oauth_error_preview(
            &serde_json::json!({ "error": error_response.error.as_str() }).to_string(),
        );
    }
    if body_text.is_empty() {
        return "[empty OAuth error body]".to_owned();
    }
    format!("[redacted OAuth error body: {} bytes]", body_text.len())
}

fn truncate_oauth_error_preview(preview: &str) -> String {
    let mut chars = preview.chars();
    let truncated: String = chars.by_ref().take(MAX_OAUTH_ERROR_PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
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
    let (access_token, refresh_token, expires_in, scope) =
        match do_refresh(&client, refresh_token.expose_secret(), OAUTH_TOKEN_URL).await {
            RefreshOutcome::Success {
                access_token,
                refresh_token,
                expires_in,
                scope,
            } => (access_token, refresh_token, expires_in, scope),
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

    let expires_in = clamp_expires_in(expires_in);
    let now_ms = unix_epoch_ms();
    let expires_at_ms = now_ms + expires_in * 1000;

    let scopes = scope.map(|s| s.split_whitespace().map(String::from).collect());
    let updated = CredentialFile {
        token: access_token,
        refresh_token: Some(refresh_token),
        expires_at: Some(expires_at_ms),
        scopes,
        subscription_type: cred.subscription_type,
    };
    updated
        .save(path)
        .map_err(|e| format!("failed to write credential file: {e}"))?;

    Ok(updated)
}

fn expand_tilde_path(path: &str, env: &impl Environment) -> PathBuf {
    if path == "~" {
        return env
            .var_os("HOME")
            .map_or_else(|| PathBuf::from(path), PathBuf::from);
    }

    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = env.var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }

    PathBuf::from(path)
}

pub(super) fn claude_code_credential_path_with_env(
    configured_path: Option<&str>,
    env: &impl Environment,
) -> Option<PathBuf> {
    if let Some(path) = env
        .var_os(CLAUDE_CODE_CREDS_ENV)
        .filter(|path| !path.is_empty())
    {
        if let Some(path_str) = path.to_str() {
            return Some(expand_tilde_path(path_str, env));
        }
        return Some(PathBuf::from(path));
    }

    configured_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| expand_tilde_path(path, env))
}

/// Resolve an explicitly configured Claude Code credentials file.
///
/// Precedence is:
///
/// 1. `CLAUDE_CODE_CREDS`
/// 2. `configured_path` from Aletheia configuration
///
/// Returns `None` when neither is set. Claude Code's private
/// `~/.claude/.credentials.json` path is intentionally not discovered by
/// default; operators must opt in by setting the env var or config path.
#[must_use]
// kanon:ignore RUST/pub-visibility -- WHY: aletheia runtime setup consumes this re-export to resolve the configured Claude Code credential path.
pub fn claude_code_credential_path(configured_path: Option<&str>) -> Option<PathBuf> {
    claude_code_credential_path_with_env(configured_path, &RealSystem)
}

/// Backward-compatible explicit Claude Code credential path lookup.
///
/// Returns `CLAUDE_CODE_CREDS` when set, otherwise `None`.
#[must_use]
// kanon:ignore RUST/pub-visibility -- WHY: pylon health checks and aletheia credential commands consume this re-export for explicit Claude Code credential detection.
pub fn claude_code_default_path() -> Option<PathBuf> {
    claude_code_credential_path(None)
}

/// Build a credential provider from a Claude Code credentials file.
///
/// If the file contains a refresh token, returns a [`RefreshingCredentialProvider`]
/// that keeps the token fresh in the background. Otherwise returns a
/// [`FileCredentialProvider`] for static token reads.
///
/// Returns `None` if the file does not exist or cannot be parsed.
#[must_use]
// kanon:ignore RUST/pub-visibility -- WHY: aletheia runtime setup consumes this re-export to build the configured Claude Code provider.
pub fn claude_code_provider(path: &Path) -> Option<Box<dyn CredentialProvider>> {
    claude_code_provider_with_config(path, CircuitBreakerConfig::default())
}

fn claude_code_provider_with_config(
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

#[cfg(test)]
#[path = "refresh_path_tests.rs"]
mod refresh_path_tests;
#[cfg(test)]
#[path = "refresh_tests.rs"]
mod refresh_tests;
