//! Credential file format and atomic I/O with encryption.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::encrypt::{
    ENCRYPTED_SENTINEL, commit_key_file, decrypt, encrypt, load_or_create_key,
    load_or_generate_key, prepare_key_file,
};

use super::unix_epoch_ms;

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
    /// Accepts three on-disk layouts:
    ///
    /// * **Encrypted**: prefixed with `ALETHEIA_ENC_V1:` — decrypted using the
    ///   sidecar key file (`<path>.key`) before parsing.
    /// * **Flat**: `{"token": "...", "refreshToken": "..."}` (native) or with the
    ///   `"accessToken"` alias produced by older Claude Code versions.
    /// * **Wrapped**: `{"claudeAiOauth": {"accessToken": "...", ...}}`: the nested
    ///   format written by current Claude Code releases.
    ///
    /// WHY: Claude Code changed its `.credentials.json` layout to nest all OAuth fields
    /// under a `claudeAiOauth` top-level key. Without unwrapping it, fresh credentials
    /// are invisible and the chain falls back to a stale env-var token.
    #[must_use]
    pub fn load(path: &Path) -> Option<Self> {
        // WHY: orphaned .json.tmp files from crashed writes waste disk and confuse operators
        let tmp = path.with_extension("json.tmp");
        if tmp.exists() {
            if let Err(e) = std::fs::remove_file(&tmp) {
                warn!(error = %e, path = %tmp.display(), "failed to clean up orphaned temp file");
            } else {
                debug!(path = %tmp.display(), "cleaned up orphaned temp file");
            }
        }

        let contents = std::fs::read_to_string(path).ok()?;

        let json = if let Some(encoded) = contents.strip_prefix(ENCRYPTED_SENTINEL) {
            // WHY: encrypted files must be decrypted before JSON parsing.
            let key = load_or_create_key(path)
                .map_err(|e| tracing::warn!(error = %e, path = %path.display(), "failed to load encryption key"))
                .ok()?;
            let plaintext = decrypt(&key, encoded.trim_end())
                .map_err(|e| tracing::warn!(error = %e, path = %path.display(), "failed to decrypt credential file"))
                .ok()?;
            String::from_utf8(plaintext)
                .map_err(|e| tracing::warn!(error = %e, "decrypted credential is not valid UTF-8"))
                .ok()?
        } else {
            contents
        };

        if let Ok(cred) = serde_json::from_str::<Self>(&json) {
            return Some(cred);
        }

        let outer: serde_json::Value = serde_json::from_str(&json).ok()?;
        serde_json::from_value(outer.get("claudeAiOauth")?.clone()).ok()
    }

    /// Write the credential file atomically (write to temp, fsync, rename).
    ///
    /// The file is always encrypted with AES-256-GCM using a per-file key
    /// stored in a sidecar `.key` file (mode 0600).
    ///
    /// Both the key file and credential file are written to temp files, fsynced,
    /// and renamed atomically as a pair. An advisory write lock (`flock`) is held
    /// for the duration to prevent races with Claude Code.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write as _;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        // WHY: load_or_generate_key tells us whether the key needs persisting so we
        // can write both temp files before committing either rename
        let (key, key_needs_persist) = load_or_generate_key(path)?;
        let encoded = encrypt(&key, json.as_bytes())?;

        let _lock = CredentialFileLock::exclusive(path)?;

        // Phase 1: write all temp files and fsync
        let key_tmp = if key_needs_persist {
            Some(prepare_key_file(path, &key)?)
        } else {
            None
        };

        let cred_tmp = path.with_extension("json.tmp");
        let mut file = std::fs::File::create(&cred_tmp)?;
        file.write_all(ENCRYPTED_SENTINEL.as_bytes())?;
        file.write_all(encoded.as_bytes())?;
        file.flush()?;
        file.sync_all()?;

        // Phase 2: rename both atomically. If either fails, clean up both.
        if let Some(ref ktmp) = key_tmp
            && let Err(e) = commit_key_file(path, ktmp)
        {
            let _ = std::fs::remove_file(ktmp);
            let _ = std::fs::remove_file(&cred_tmp);
            return Err(e);
        }

        if let Err(e) = std::fs::rename(&cred_tmp, path) {
            let _ = std::fs::remove_file(&cred_tmp);
            // NOTE: key file was already renamed; it's valid on its own
            return Err(e);
        }

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
    pub fn seconds_remaining(&self) -> Option<i64> {
        let expires_at_ms = self.expires_at?;
        let now_ms = unix_epoch_ms();
        // WHY: ms timestamps fit in i64 until year 292M; subtraction gives signed delta
        let expires_i64 = i64::try_from(expires_at_ms).unwrap_or(i64::MAX);
        let now_i64 = i64::try_from(now_ms).unwrap_or(i64::MAX);
        Some((expires_i64 - now_i64) / 1000)
    }

    /// Whether the token needs refresh (expired or within threshold).
    #[must_use]
    #[expect(
        dead_code,
        reason = "refresh logic inlined in refresh_loop; kept as public API"
    )]
    pub(crate) fn needs_refresh(&self) -> bool {
        match self.seconds_remaining() {
            // WHY: REFRESH_THRESHOLD_SECS is a small constant that fits in i64
            Some(remaining) => {
                remaining < i64::try_from(super::REFRESH_THRESHOLD_SECS).unwrap_or(i64::MAX)
            }
            None => false,
        }
    }
}

/// Advisory file lock for credential read-modify-write cycles.
///
/// Uses `flock()` via `rustix` on a `.lock` sidecar file. The lock file is
/// created alongside the credential file and never deleted (harmless).
pub(super) struct CredentialFileLock {
    _file: std::fs::File,
}

impl CredentialFileLock {
    /// Acquire a shared (read) lock on the credential file.
    #[expect(
        dead_code,
        reason = "available for load() callers that need consistency"
    )]
    pub(super) fn shared(credential_path: &Path) -> std::io::Result<Self> {
        Self::lock(credential_path, rustix::fs::FlockOperation::LockShared)
    }

    /// Acquire an exclusive (write) lock on the credential file.
    pub(super) fn exclusive(credential_path: &Path) -> std::io::Result<Self> {
        Self::lock(credential_path, rustix::fs::FlockOperation::LockExclusive)
    }

    #[cfg(unix)]
    #[expect(
        unsafe_code,
        reason = "BorrowedFd::borrow_raw requires unsafe; fd is valid for File's lifetime"
    )]
    fn lock(credential_path: &Path, op: rustix::fs::FlockOperation) -> std::io::Result<Self> {
        use std::os::unix::io::AsRawFd;

        let lock_path = credential_path.with_extension("json.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        // WHY: flock is advisory but sufficient when all writers cooperate
        rustix::fs::flock(
            unsafe { rustix::fd::BorrowedFd::borrow_raw(file.as_raw_fd()) },
            op,
        )
        .map_err(|e| std::io::Error::from_raw_os_error(e.raw_os_error()))?;
        Ok(Self { _file: file })
    }

    #[cfg(not(unix))]
    fn lock(credential_path: &Path, _op: rustix::fs::FlockOperation) -> std::io::Result<Self> {
        let lock_path = credential_path.with_extension("json.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        Ok(Self { _file: file })
    }
}

// NOTE: lock is released when _file is dropped (flock semantics)
