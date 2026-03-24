//! Static credential providers: environment variables, files, and chains.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Instant, SystemTime};

use tracing::warn;

use aletheia_koina::credential::{Credential, CredentialProvider, CredentialSource};
use aletheia_koina::secret::SecretString;

use super::file_ops::CredentialFile;
use super::{CLOCK_SKEW_LEEWAY_SECS, FILE_MTIME_CHECK_INTERVAL, OAUTH_TOKEN_PREFIX, unix_epoch_ms};
use crate::util::decode_jwt_exp_secs;

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
    /// Create a provider that reads credentials from the given environment variable.
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

            // WHY: static env var tokens cannot be refreshed; a refreshable file
            // provider downstream must get a chance to supply a valid credential.
            if v.starts_with(OAUTH_TOKEN_PREFIX)
                && let Some(exp_secs) = decode_jwt_exp_secs(&v)
            {
                let now_secs = unix_epoch_ms() / 1000;
                if exp_secs + CLOCK_SKEW_LEEWAY_SECS < now_secs {
                    warn!(
                        var = %self.var_name,
                        exp_secs,
                        now_secs,
                        leeway_secs = CLOCK_SKEW_LEEWAY_SECS,
                        "OAuth token from environment variable expired \
                         (exp + leeway < now), falling through to next provider"
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
            Some(Credential {
                secret: SecretString::from(v),
                source,
            })
        })
    }

    fn name(&self) -> &str {
        &self.var_name
    }
}

// NOTE: pub(crate) for test access after credential.rs → credential/mod.rs split
pub(crate) struct CachedFile {
    pub(crate) token: SecretString,
    pub(crate) mtime: SystemTime,
    pub(crate) checked_at: Instant,
}

/// Reads a credential from a JSON file on disk.
pub struct FileCredentialProvider {
    path: PathBuf,
    // NOTE: pub(crate) for test access after credential.rs → credential/mod.rs split
    pub(crate) cached: RwLock<Option<CachedFile>>,
}

impl FileCredentialProvider {
    /// Create a provider that reads credentials from the given JSON file path.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            cached: RwLock::new(None),
        }
    }

    pub(super) fn current_mtime(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.path).ok()?.modified().ok()
    }

    fn reload(&self) -> Option<SecretString> {
        let cred = CredentialFile::load(&self.path)?;
        let mtime = self.current_mtime().unwrap_or_else(|| {
            tracing::debug!(path = %self.path.display(), "could not read file mtime, using epoch");
            SystemTime::UNIX_EPOCH
        });

        let token = SecretString::from(cred.token);
        if let Ok(mut guard) = self.cached.write() {
            *guard = Some(CachedFile {
                token: token.clone(),
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

        self.reload().map(|secret| Credential {
            secret,
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

/// Ordered list of credential providers. First to return `Some` wins.
pub struct CredentialChain {
    providers: Vec<Box<dyn CredentialProvider>>,
    resolved_name: RwLock<String>,
}

impl CredentialChain {
    /// Create a credential chain that tries each provider in order until one succeeds.
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
