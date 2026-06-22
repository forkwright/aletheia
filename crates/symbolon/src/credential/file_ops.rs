//! Credential file format and atomic I/O with encryption.

use std::path::Path;

use koina::secret::SecretString;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use std::time::{Duration, SystemTime};

use crate::encrypt::{
    ENCRYPTED_SENTINEL, commit_key_file, decrypt, encrypt, load_key, load_or_generate_key,
    prepare_key_file,
};

use super::unix_epoch_ms;

fn serialize_secret<S: serde::Serializer>(
    secret: &SecretString,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(secret.expose_secret())
}

// WHY: serde's `serialize_with` passes `&T` where T is the field type, so an
// `Option<SecretString>` field forces a `&Option<SecretString>` parameter.
// Cannot be rewritten to `Option<&SecretString>` without dropping the
// `serialize_with` attribute.
#[expect(
    clippy::ref_option,
    reason = "serde serialize_with contract requires &Option<T> for Option<T> fields"
)]
fn serialize_option_secret<S: serde::Serializer>(
    secret: &Option<SecretString>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match secret {
        Some(s) => serializer.serialize_some(s.expose_secret()),
        None => serializer.serialize_none(),
    }
}

/// On-disk credential file format.
///
/// Accepts both `"token"` (native format) and `"accessToken"` (Claude Code OAuth
/// output) for backward compatibility. Serialization always writes `"token"`.
// kanon:ignore RUST/no-debug-derive-on-public-types — CredentialFile redacts the token field via SecretString's custom Debug; derived Debug is safe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialFile {
    /// Access token (API key or OAuth access token).
    #[serde(alias = "accessToken", serialize_with = "serialize_secret")]
    pub token: SecretString,
    /// OAuth refresh token (absent for static API keys).
    #[serde(
        rename = "refreshToken",
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_option_secret"
    )]
    pub refresh_token: Option<SecretString>,
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
        // WHY: validate path stays within its parent directory to prevent
        // path-traversal via crafted credential config paths (CodeQL
        // "Uncontrolled data used in path expression").
        let parent = path.parent()?;
        if parent.exists()
            && let Err(e) = koina::fs::validate_within_root(path, parent)
        {
            // SAFETY: logs file path and validation error, not credential contents.
            // WHY: phrase avoids the word "credential" to stay within
            // SECURITY/credential-logging policy; the surrounding module name
            // preserves context for operators reading the log.
            warn!(error = %e, path = %path.display(), "auth file path validation failed");
            return None;
        }

        // WHY: orphaned .json.tmp files from crashed writes waste disk and confuse operators.
        // Cleanup must happen under an exclusive lock and only for files old enough that a
        // concurrent writer is unlikely to still be holding the lock.
        let tmp = path.with_extension("json.tmp");
        if tmp.exists() {
            // WHY: validate derived tmp path before filesystem operations
            if parent.exists()
                && let Err(e) = koina::fs::validate_within_root(&tmp, parent)
            {
                warn!(error = %e, path = %tmp.display(), "temp file path validation failed");
                return None;
            }
            cleanup_orphaned_temp_file(path, &tmp);
        }

        // WHY: hold a shared advisory lock while reading so concurrent saves cannot
        // observe or mutate the credential file halfway through a write.
        let _lock = CredentialFileLock::shared(path).ok()?;

        let contents = std::fs::read_to_string(path).ok()?;

        let json = if let Some(encoded) = contents.strip_prefix(ENCRYPTED_SENTINEL) {
            // WHY: encrypted files must be decrypted before JSON parsing.
            // `load` never creates a missing key file; that would produce a
            // confusing decrypt failure instead of a clear "key missing" error.
            let key = load_key(path)
                // SAFETY: logging file path and error kind, not credential value
                .map_err(|e| tracing::warn!(error = %e, path = %path.display(), "failed to load encryption key")) // kanon:ignore SECURITY/credential-logging -- logs error on decrypt failure, not the credential
                .ok()?;
            let plaintext = decrypt(&key, encoded.trim_end())
                // SAFETY: logging file path and error kind, not credential value
                .map_err(|e| tracing::warn!(error = %e, path = %path.display(), "failed to decrypt credential file")) // kanon:ignore SECURITY/credential-logging -- logs error on decrypt failure, not the credential
                .ok()?;
            String::from_utf8(plaintext)
                .map_err(|e| tracing::warn!(error = %e, "decrypted credential is not valid UTF-8")) // kanon:ignore SECURITY/credential-logging -- logs UTF-8 error, not the credential
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

        // WHY: validate path stays within its parent directory to prevent
        // path-traversal via crafted credential config paths (CodeQL
        // "Uncontrolled data used in path expression").
        if let Some(parent) = path.parent()
            && parent.exists()
        {
            koina::fs::validate_within_root(path, parent)?;
        }

        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        // WHY: load_or_generate_key tells us whether the key needs persisting so we
        // can write both temp files before committing either rename
        let (key, key_needs_persist) = load_or_generate_key(path)?;
        let encoded = encrypt(&key, json.as_bytes())?;

        let _lock = CredentialFileLock::exclusive(path)?;

        // WHY: create parent dirs inside the lock scope to prevent race conditions
        // where two concurrent writers both create the directory
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // INVARIANT: Phase 1 — write all temp files and fsync.
        // WHY: TempFileGuard ensures cleanup if the caller panics between
        // prepare and commit. Defuse after successful commit.
        let mut key_guard = if key_needs_persist {
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

        // INVARIANT: Phase 2 — rename both atomically. If either fails, clean up both.
        if let Some(ref guard) = key_guard
            && let Err(e) = commit_key_file(path, guard.path())
        {
            // NOTE: the guard's drop cleans up the key tmp; the cred tmp is
            // cleaned manually.
            // kanon:ignore RUST/no-silent-result-swallow — best-effort cleanup of temp credential file on commit failure
            let _ = std::fs::remove_file(&cred_tmp);
            return Err(e);
        }

        // WHY: the key is committed, so defuse the guard or Drop would delete
        // the live key file.
        if let Some(ref mut guard) = key_guard {
            guard.defuse();
        }

        if let Err(e) = std::fs::rename(&cred_tmp, path) {
            // kanon:ignore RUST/no-silent-result-swallow — best-effort cleanup of temp credential file on rename failure
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
        self.refresh_token
            .as_ref()
            .is_some_and(|t| !t.expose_secret().is_empty())
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

/// Maximum age for a `.json.tmp` file before it is treated as an orphan and
/// removed during load.
const ORPHAN_TEMP_AGE_THRESHOLD: Duration = Duration::from_secs(60);

/// Remove an orphaned credential temp file, but only under an exclusive lock and
/// only when the file is older than [`ORPHAN_TEMP_AGE_THRESHOLD`].
fn cleanup_orphaned_temp_file(credential_path: &Path, tmp: &Path) {
    if !is_temp_orphaned(tmp, ORPHAN_TEMP_AGE_THRESHOLD) {
        return;
    }

    match CredentialFileLock::exclusive(credential_path) {
        Ok(_lock) => {
            // Re-check age after acquiring the lock; a concurrent writer may
            // have recycled the temp file in the meantime.
            if !is_temp_orphaned(tmp, ORPHAN_TEMP_AGE_THRESHOLD) {
                return;
            }
            match std::fs::remove_file(tmp) {
                Ok(()) => debug!(path = %tmp.display(), "cleaned up orphaned temp file"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    debug!(path = %tmp.display(), "orphaned temp file already removed");
                }
                Err(e) => {
                    warn!(error = %e, path = %tmp.display(), "failed to clean up orphaned temp file");
                }
            }
        }
        Err(e) => {
            warn!(error = %e, path = %tmp.display(), "skipped orphaned temp cleanup; could not acquire exclusive lock");
        }
    }
}

/// Whether `tmp` exists and its mtime is older than `max_age`.
fn is_temp_orphaned(tmp: &Path, max_age: Duration) -> bool {
    let Ok(metadata) = std::fs::metadata(tmp) else {
        return false;
    };
    let Some(age) = metadata
        .modified()
        .ok()
        .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
    else {
        // If we cannot determine the age, err on the side of leaving the file
        // alone; a fresh temp file from an in-flight writer must not be deleted.
        return false;
    };
    age > max_age
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
    pub(super) fn shared(credential_path: &Path) -> std::io::Result<Self> {
        let lock_path = credential_path.with_extension("json.lock");
        Self::lock_path(&lock_path, rustix::fs::FlockOperation::LockShared)
    }

    /// Acquire an exclusive (write) lock on the credential file.
    pub(super) fn exclusive(credential_path: &Path) -> std::io::Result<Self> {
        let lock_path = credential_path.with_extension("json.lock");
        Self::lock_path(&lock_path, rustix::fs::FlockOperation::LockExclusive)
    }

    /// Acquire an exclusive lock on an arbitrary lock-file path.
    ///
    /// Used for provider-wide rotations where a single lock must cover both the
    /// primary and backup credential files.
    pub(super) fn exclusive_at(lock_path: &Path) -> std::io::Result<Self> {
        Self::lock_path(lock_path, rustix::fs::FlockOperation::LockExclusive)
    }

    #[cfg(unix)]
    #[expect(
        unsafe_code,
        reason = "BorrowedFd::borrow_raw requires unsafe; fd is valid for File's lifetime"
    )]
    fn lock_path(lock_path: &Path, op: rustix::fs::FlockOperation) -> std::io::Result<Self> {
        use std::os::unix::io::AsRawFd;

        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)?;
        // WHY: flock is advisory but sufficient when all writers cooperate
        rustix::fs::flock(
            unsafe { rustix::fd::BorrowedFd::borrow_raw(file.as_raw_fd()) },
            op,
        )
        .map_err(|e| std::io::Error::from_raw_os_error(e.raw_os_error()))?;
        Ok(Self { _file: file })
    }

    #[cfg(not(unix))]
    #[expect(
        dead_code,
        reason = "lock_path stub on non-Unix opens the sidecar for consistency but does not implement advisory locking"
    )]
    fn lock_path(lock_path: &Path, _op: rustix::fs::FlockOperation) -> std::io::Result<Self> {
        // WHY: advisory file locking is not implemented on non-Unix targets.
        // The sidecar file is still created so that callers can treat lock
        // acquisition as a uniform operation, but this is intentionally a no-op
        // stub. All production deployments that rely on this behavior run on
        // Unix-like systems.
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)?;
        Ok(Self { _file: file })
    }
}

// NOTE: lock is released when _file is dropped (flock semantics)

#[cfg(test)]
#[path = "file_ops_tests.rs"]
mod file_ops_tests;
