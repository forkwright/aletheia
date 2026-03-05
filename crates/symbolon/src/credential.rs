//! Credential provider implementations for LLM API key resolution.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use aletheia_koina::credential::{Credential, CredentialProvider, CredentialSource};

/// Claude Code production OAuth client ID.
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// OAuth token refresh endpoint.
const OAUTH_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";

/// Refresh when token has less than this many seconds remaining.
const REFRESH_THRESHOLD_SECS: u64 = 3600;

/// How often the background refresh task checks token expiry.
const REFRESH_CHECK_INTERVAL_SECS: u64 = 60;

/// How often to check file mtime for external changes.
const FILE_MTIME_CHECK_INTERVAL: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Credential file format (matches TS/Python)
// ---------------------------------------------------------------------------

/// On-disk credential file format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialFile {
    /// Access token (API key or OAuth access token).
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
    pub fn load(path: &Path) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    /// Write the credential file atomically (write to temp, rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut file = std::fs::File::create(&tmp)?;
        serde_json::to_writer_pretty(&mut file, self)
            .map_err(std::io::Error::other)?;
        file.flush()?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;

        // Set restrictive permissions on Unix.
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
        self.refresh_token
            .as_ref()
            .is_some_and(|t| !t.is_empty())
    }

    /// Seconds remaining until token expires. Returns `None` if no expiry set.
    #[must_use]
    #[expect(clippy::cast_possible_wrap, reason = "ms timestamps fit in i64 until year 292M")]
    pub fn seconds_remaining(&self) -> Option<i64> {
        let expires_at_ms = self.expires_at?;
        #[expect(clippy::cast_possible_truncation, reason = "ms timestamps fit in u64 until year 584M")]
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Some((expires_at_ms as i64 - now_ms as i64) / 1000)
    }

    /// Whether the token needs refresh (expired or within threshold).
    #[must_use]
    #[expect(clippy::cast_possible_wrap, reason = "threshold constant fits in i64")]
    pub fn needs_refresh(&self) -> bool {
        match self.seconds_remaining() {
            Some(remaining) => remaining < REFRESH_THRESHOLD_SECS as i64,
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// OAuth response
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OAuthResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
    scope: Option<String>,
}

fn default_expires_in() -> u64 {
    28800 // 8 hours
}

// ---------------------------------------------------------------------------
// EnvCredentialProvider
// ---------------------------------------------------------------------------

/// Reads a credential from an environment variable.
pub struct EnvCredentialProvider {
    var_name: String,
}

impl EnvCredentialProvider {
    #[must_use]
    pub fn new(var_name: impl Into<String>) -> Self {
        Self {
            var_name: var_name.into(),
        }
    }
}

impl CredentialProvider for EnvCredentialProvider {
    fn get_credential(&self) -> Option<Credential> {
        std::env::var(&self.var_name).ok().and_then(|v| {
            if v.is_empty() {
                None
            } else {
                Some(Credential {
                    secret: v,
                    source: CredentialSource::Environment,
                })
            }
        })
    }

    fn name(&self) -> &str {
        &self.var_name
    }
}

// ---------------------------------------------------------------------------
// FileCredentialProvider
// ---------------------------------------------------------------------------

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
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn current_mtime(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.path).ok()?.modified().ok()
    }

    fn reload(&self) -> Option<String> {
        let cred = CredentialFile::load(&self.path)?;
        let mtime = self.current_mtime().unwrap_or(SystemTime::UNIX_EPOCH);

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
        // Check cache validity
        if let Ok(guard) = self.cached.read() {
            if let Some(cached) = guard.as_ref() {
                if cached.checked_at.elapsed() < FILE_MTIME_CHECK_INTERVAL {
                    return Some(Credential {
                        secret: cached.token.clone(),
                        source: CredentialSource::File,
                    });
                }
                // Check if file changed
                if let Some(mtime) = self.current_mtime() {
                    if mtime == cached.mtime {
                        // File unchanged — update check timestamp and return cached
                        drop(guard);
                        if let Ok(mut w) = self.cached.write() {
                            if let Some(ref mut c) = *w {
                                c.checked_at = Instant::now();
                            }
                        }
                        if let Ok(g) = self.cached.read() {
                            if let Some(c) = g.as_ref() {
                                return Some(Credential {
                                    secret: c.token.clone(),
                                    source: CredentialSource::File,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Cache miss or stale — reload
        self.reload().map(|token| Credential {
            secret: token,
            source: CredentialSource::File,
        })
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str return")]
    fn name(&self) -> &str {
        "file"
    }
}

// ---------------------------------------------------------------------------
// RefreshingCredentialProvider
// ---------------------------------------------------------------------------

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
            expires_at_ms: cred.expires_at.unwrap_or(0),
        })));

        let shutdown = Arc::new(AtomicBool::new(false));

        let task_state = Arc::clone(&state);
        let task_shutdown = Arc::clone(&shutdown);
        let task_path = path.clone();

        let task = tokio::spawn(async move {
            refresh_loop(task_state, task_shutdown, task_path).await;
        });

        Some(Self {
            state,
            file_provider: FileCredentialProvider::new(path),
            shutdown,
            task: Some(task),
        })
    }

    /// Signal the background refresh task to stop.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl CredentialProvider for RefreshingCredentialProvider {
    fn get_credential(&self) -> Option<Credential> {
        // Try in-memory refreshed token first
        if let Ok(guard) = self.state.read() {
            if let Some(ref s) = *guard {
                if !s.current_token.is_empty() {
                    return Some(Credential {
                        secret: s.current_token.clone(),
                        source: CredentialSource::OAuth,
                    });
                }
            }
        }
        // Fall back to file read
        self.file_provider.get_credential()
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str return")]
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

        // Check if refresh is needed
        let (refresh_token, needs_refresh) = {
            let Ok(guard) = state.read() else {
                continue;
            };
            let Some(s) = guard.as_ref() else {
                continue;
            };
            #[expect(clippy::cast_possible_truncation, reason = "ms timestamps fit in u64")]
            let now_ms = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            #[expect(clippy::cast_possible_wrap, reason = "ms timestamps fit in i64")]
            let remaining_secs =
                (s.expires_at_ms as i64 - now_ms as i64) / 1000;
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
                #[expect(clippy::cast_possible_truncation, reason = "ms timestamps fit in u64")]
                let now_ms = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let expires_at_ms = now_ms + resp.expires_in * 1000;

                // Update in-memory state
                if let Ok(mut guard) = state.write() {
                    *guard = Some(RefreshState {
                        current_token: resp.access_token.clone(),
                        refresh_token: resp.refresh_token.clone(),
                        expires_at_ms,
                    });
                }

                // Update file
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

                info!(
                    expires_in_secs = resp.expires_in,
                    "OAuth token refreshed"
                );
            }
            None => {
                warn!("OAuth token refresh failed — will retry next cycle");
            }
        }
    }
}

async fn do_refresh(client: &reqwest::Client, refresh_token: &str) -> Option<OAuthResponse> {
    let payload = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": OAUTH_CLIENT_ID,
    });

    let resp = client
        .post(OAUTH_TOKEN_URL)
        .json(&payload)
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
        let body = resp.text().await.unwrap_or_default();
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

    #[expect(clippy::cast_possible_truncation, reason = "ms timestamps fit in u64")]
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
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

// ---------------------------------------------------------------------------
// CredentialChain
// ---------------------------------------------------------------------------

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

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str return")]
    fn name(&self) -> &str {
        "chain"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- CredentialFile ---

    #[test]
    fn credential_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");

        let cred = CredentialFile {
            token: "sk-test-123".to_owned(),
            refresh_token: Some("rt-test-456".to_owned()),
            expires_at: Some(1_700_000_000_000),
            scopes: Some(vec!["user:inference".to_owned()]),
            subscription_type: Some("max".to_owned()),
        };
        cred.save(&path).unwrap();

        let loaded = CredentialFile::load(&path).unwrap();
        assert_eq!(loaded.token, "sk-test-123");
        assert_eq!(loaded.refresh_token.as_deref(), Some("rt-test-456"));
        assert_eq!(loaded.expires_at, Some(1_700_000_000_000));
    }

    #[test]
    fn credential_file_missing_returns_none() {
        assert!(CredentialFile::load(Path::new("/nonexistent/path.json")).is_none());
    }

    #[test]
    fn credential_file_malformed_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(CredentialFile::load(&path).is_none());
    }

    #[test]
    fn has_refresh_token() {
        let with = CredentialFile {
            token: "t".to_owned(),
            refresh_token: Some("rt".to_owned()),
            expires_at: None,
            scopes: None,
            subscription_type: None,
        };
        assert!(with.has_refresh_token());

        let without = CredentialFile {
            refresh_token: None,
            ..with.clone()
        };
        assert!(!without.has_refresh_token());

        let empty = CredentialFile {
            refresh_token: Some(String::new()),
            ..without
        };
        assert!(!empty.has_refresh_token());
    }

    // --- EnvCredentialProvider ---

    // EnvCredentialProvider tests use ANTHROPIC_API_KEY which is typically
    // set in CI/dev. We test the missing case with a guaranteed-absent var.

    #[test]
    fn env_provider_missing_returns_none() {
        let provider = EnvCredentialProvider::new("ALETHEIA_TEST_NONEXISTENT_49_XYZ");
        assert!(provider.get_credential().is_none());
    }

    #[test]
    fn env_provider_name() {
        let provider = EnvCredentialProvider::new("MY_VAR");
        assert_eq!(provider.name(), "MY_VAR");
    }

    // --- FileCredentialProvider ---

    #[test]
    fn file_provider_reads_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("anthropic.json");
        let cred = CredentialFile {
            token: "sk-file-token".to_owned(),
            refresh_token: None,
            expires_at: None,
            scopes: None,
            subscription_type: None,
        };
        cred.save(&path).unwrap();

        let provider = FileCredentialProvider::new(path);
        let result = provider.get_credential().unwrap();
        assert_eq!(result.secret, "sk-file-token");
        assert_eq!(result.source, CredentialSource::File);
    }

    #[test]
    fn file_provider_missing_file_returns_none() {
        let provider = FileCredentialProvider::new(PathBuf::from("/nonexistent/cred.json"));
        assert!(provider.get_credential().is_none());
    }

    #[test]
    fn file_provider_detects_file_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("anthropic.json");

        let cred1 = CredentialFile {
            token: "token-v1".to_owned(),
            refresh_token: None,
            expires_at: None,
            scopes: None,
            subscription_type: None,
        };
        cred1.save(&path).unwrap();

        let provider = FileCredentialProvider::new(path.clone());
        let r1 = provider.get_credential().unwrap();
        assert_eq!(r1.secret, "token-v1");

        // Invalidate cache timestamp to force mtime check
        if let Ok(mut guard) = provider.cached.write() {
            if let Some(ref mut c) = *guard {
                c.checked_at = Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or(Instant::now());
                // Also change mtime to force reload
                c.mtime = SystemTime::UNIX_EPOCH;
            }
        }

        let cred2 = CredentialFile {
            token: "token-v2".to_owned(),
            ..cred1
        };
        cred2.save(&path).unwrap();

        let r2 = provider.get_credential().unwrap();
        assert_eq!(r2.secret, "token-v2");
    }

    // --- CredentialChain ---

    struct StaticProvider {
        token: Option<String>,
        name: &'static str,
    }

    impl CredentialProvider for StaticProvider {
        fn get_credential(&self) -> Option<Credential> {
            self.token.as_ref().map(|t| Credential {
                secret: t.clone(),
                source: CredentialSource::Environment,
            })
        }
        fn name(&self) -> &str {
            self.name
        }
    }

    #[test]
    fn chain_first_wins() {
        let chain = CredentialChain::new(vec![
            Box::new(StaticProvider {
                token: Some("first".to_owned()),
                name: "a",
            }),
            Box::new(StaticProvider {
                token: Some("second".to_owned()),
                name: "b",
            }),
        ]);
        let cred = chain.get_credential().unwrap();
        assert_eq!(cred.secret, "first");
    }

    #[test]
    fn chain_skips_empty() {
        let chain = CredentialChain::new(vec![
            Box::new(StaticProvider {
                token: None,
                name: "empty",
            }),
            Box::new(StaticProvider {
                token: Some("fallback".to_owned()),
                name: "fb",
            }),
        ]);
        let cred = chain.get_credential().unwrap();
        assert_eq!(cred.secret, "fallback");
    }

    #[test]
    fn chain_all_empty_returns_none() {
        let chain = CredentialChain::new(vec![
            Box::new(StaticProvider {
                token: None,
                name: "a",
            }),
            Box::new(StaticProvider {
                token: None,
                name: "b",
            }),
        ]);
        assert!(chain.get_credential().is_none());
    }

    #[test]
    fn chain_empty_providers_returns_none() {
        let chain = CredentialChain::new(vec![]);
        assert!(chain.get_credential().is_none());
    }
}
