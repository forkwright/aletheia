//! Shared bearer-token secret storage core (#7027).
//!
//! `tui.toml` and desktop settings previously stored connection bearer
//! tokens as plaintext (#5321). Tokens are now written to the OS keyring
//! first; if that backend is unavailable, storage falls back to an
//! AES-256-GCM encrypted file under a caller-supplied config directory.
//! Callers store only the stable non-secret `token_ref` marker in their own
//! config — never the raw token.
//!
//! Koilon (the TUI) manages exactly one connection at a time and addresses
//! it with a single fixed `token_ref`; proskenion (the desktop app)
//! addresses multiple server token references. That is the one place the
//! two clients legitimately differ. [`TokenStore`] takes each client's
//! keyring service name, fallback directory name, fallback file sentinel,
//! and test-mode predicate, and owns everything else: the error taxonomy,
//! the keyring state machine, cipher framing/versioning, key
//! generation/read/zeroization, and secure atomic writes. Adapters in
//! `koilon::secret_store` and `proskenion::services::secret_store` are thin
//! wrappers that fix those parameters for their own client.
//!
//! WARNING: the fallback's AES key is stored beside its ciphertext, both
//! `0o600` inside a `0o700` directory. Confidentiality therefore rests on
//! filesystem permissions, not on the cipher — anyone who can read the
//! ciphertext can read the key. What the fallback buys over plaintext
//! config is that the token no longer lives in a file operators paste into
//! bug reports and copy between hosts. The keyring path is the one that
//! offers real at-rest protection; the fallback exists so a headless box
//! without a keyring backend still avoids plaintext-in-config.
//!
//! NOTE: the fallback AES key and the decrypted token are held in
//! [`zeroize::Zeroizing`] / [`koina::secret::SecretString`], both of which
//! wipe their backing buffer on drop. This narrows, but does not close, the
//! residual-memory window — a buffer that was copied or reallocated before
//! wrapping (e.g. an intermediate `Vec` from a resize) leaves copies the
//! wrapper never touches, and the OS may still have paged either buffer to
//! swap before it was wiped.

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead as _, Generate as _, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit as _};
use koina::secret::SecretString;
use snafu::{ResultExt as _, Snafu};
use zeroize::{Zeroize as _, Zeroizing};

/// Errors from bearer-token secret storage.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum SecretStoreError {
    /// Failed to create a secret-storage directory.
    #[snafu(display("failed to create secret directory {}: {source}", path.display()))]
    CreateDir {
        /// Directory path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to read a fallback secret file.
    #[snafu(display("failed to read secret file {}: {source}", path.display()))]
    ReadFile {
        /// File path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to write a fallback secret file.
    #[snafu(display("failed to write secret file {}: {source}", path.display()))]
    WriteFile {
        /// File path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to remove a fallback secret file.
    #[snafu(display("failed to delete secret file {}: {source}", path.display()))]
    DeleteFile {
        /// File path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Fallback encrypted secret file has an invalid format.
    #[snafu(display("invalid encrypted secret in {}: {message}", path.display()))]
    InvalidEncryptedSecret {
        /// File path.
        path: PathBuf,
        /// Validation message.
        message: &'static str,
    },

    /// Decrypted secret bytes were not valid UTF-8.
    #[snafu(display("decrypted secret in {} is not valid UTF-8: {source}", path.display()))]
    Utf8 {
        /// File path.
        path: PathBuf,
        /// Underlying UTF-8 error.
        source: std::string::FromUtf8Error,
    },
}

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

enum KeyringLoad {
    Found(String),
    Missing,
    Unavailable,
}

/// A namespaced bearer-token store: OS keyring first, AES-256-GCM encrypted
/// file fallback.
///
/// Construct one `const` instance per client (see `koilon::secret_store` and
/// `proskenion::services::secret_store`) and call its methods with that
/// client's config base directory and token reference.
pub struct TokenStore {
    keyring_service: &'static str,
    fallback_dir_name: &'static str,
    fallback_sentinel: &'static str,
    keyring_enabled: fn() -> bool,
}

impl TokenStore {
    /// Construct a token store scoped to one client.
    ///
    /// `keyring_service` names the OS keyring service (e.g.
    /// `"aletheia-tui"`). `fallback_dir_name` names the directory created
    /// under the caller-supplied base directory for encrypted fallback
    /// files (e.g. `"aletheia"`). `fallback_sentinel` prefixes a fallback
    /// file's content to mark its format; it is part of that client's
    /// on-disk contract and must stay stable once tokens have been written
    /// under it. `keyring_enabled` lets a caller disable keyring lookups —
    /// callers pass `|| !cfg!(test)` so their own test builds deterministically
    /// exercise the fallback path rather than depending on whatever keyring
    /// backend (if any) happens to be present on the machine running tests.
    #[must_use]
    pub const fn new(
        keyring_service: &'static str,
        fallback_dir_name: &'static str,
        fallback_sentinel: &'static str,
        keyring_enabled: fn() -> bool,
    ) -> Self {
        Self {
            keyring_service,
            fallback_dir_name,
            fallback_sentinel,
            keyring_enabled,
        }
    }

    /// Store a bearer token in the OS keyring or encrypted fallback.
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keyring is unavailable and encrypted
    /// fallback storage cannot be written.
    pub fn store_token(
        &self,
        base: &Path,
        token_ref: &str,
        token: &str,
    ) -> Result<(), SecretStoreError> {
        if self.try_store_keyring(token_ref, token) {
            if let Err(err) = self.delete_fallback(base, token_ref) {
                tracing::warn!(error = %err, token_ref, "failed to remove stale encrypted fallback token");
            }
            return Ok(());
        }

        self.write_fallback(base, token_ref, token)
    }

    /// Load a bearer token by reference.
    ///
    /// # Errors
    ///
    /// Returns an error if encrypted fallback data exists but cannot be
    /// read or decrypted.
    pub fn load_token(
        &self,
        base: &Path,
        token_ref: &str,
    ) -> Result<Option<SecretString>, SecretStoreError> {
        match self.try_load_keyring(token_ref) {
            KeyringLoad::Found(token) => return Ok(Some(SecretString::from(token))),
            KeyringLoad::Missing => {}
            KeyringLoad::Unavailable => {
                tracing::debug!(
                    token_ref,
                    "token keyring unavailable, trying encrypted fallback"
                );
            }
        }

        self.read_fallback(base, token_ref)
    }

    /// Delete a bearer token from keyring and fallback storage.
    ///
    /// # Errors
    ///
    /// Returns an error only when removing local encrypted fallback files
    /// fails.
    pub fn delete_token(&self, base: &Path, token_ref: &str) -> Result<(), SecretStoreError> {
        if let Err(err) = self.delete_keyring(token_ref) {
            tracing::debug!(error = %err, token_ref, "token keyring delete skipped");
        }
        self.delete_fallback(base, token_ref)
    }

    fn keyring_entry(&self, token_ref: &str) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(self.keyring_service, token_ref)
    }

    fn try_store_keyring(&self, token_ref: &str, token: &str) -> bool {
        if !(self.keyring_enabled)() {
            return false;
        }

        match self
            .keyring_entry(token_ref)
            .and_then(|entry| entry.set_password(token))
        {
            Ok(()) => true,
            Err(err) => {
                tracing::debug!(error = %err, token_ref, "token keyring write failed, using encrypted fallback");
                false
            }
        }
    }

    fn try_load_keyring(&self, token_ref: &str) -> KeyringLoad {
        if !(self.keyring_enabled)() {
            return KeyringLoad::Unavailable;
        }

        let entry = match self.keyring_entry(token_ref) {
            Ok(entry) => entry,
            Err(err) => {
                tracing::debug!(error = %err, token_ref, "token keyring entry unavailable");
                return KeyringLoad::Unavailable;
            }
        };

        match entry.get_password() {
            Ok(token) if token.is_empty() => KeyringLoad::Missing,
            Ok(token) => KeyringLoad::Found(token),
            Err(keyring::Error::NoEntry) => KeyringLoad::Missing,
            Err(err) => {
                tracing::debug!(error = %err, token_ref, "token keyring read failed");
                KeyringLoad::Unavailable
            }
        }
    }

    fn delete_keyring(&self, token_ref: &str) -> Result<(), keyring::Error> {
        if !(self.keyring_enabled)() {
            return Ok(());
        }

        match self.keyring_entry(token_ref)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn fallback_dir(&self, base: &Path) -> PathBuf {
        base.join(self.fallback_dir_name).join("secrets")
    }

    fn fallback_file(&self, base: &Path, token_ref: &str) -> PathBuf {
        self.fallback_dir(base)
            .join(format!("{}.token", safe_file_stem(token_ref)))
    }

    fn fallback_key_file(&self, base: &Path, token_ref: &str) -> PathBuf {
        self.fallback_dir(base)
            .join(format!("{}.key", safe_file_stem(token_ref)))
    }

    fn write_fallback(
        &self,
        base: &Path,
        token_ref: &str,
        token: &str,
    ) -> Result<(), SecretStoreError> {
        let key_path = self.fallback_key_file(base, token_ref);
        let token_path = self.fallback_file(base, token_ref);
        let key = load_or_create_key(&key_path)?;
        let encrypted =
            encrypt(&key, token.as_bytes()).map_err(|source| SecretStoreError::WriteFile {
                path: token_path.clone(),
                source,
            })?;
        let payload = format!("{}{encrypted}", self.fallback_sentinel);
        write_secure_file(&token_path, payload.as_bytes())
    }

    fn read_fallback(
        &self,
        base: &Path,
        token_ref: &str,
    ) -> Result<Option<SecretString>, SecretStoreError> {
        let token_path = self.fallback_file(base, token_ref);
        if !token_path.exists() {
            return Ok(None);
        }

        let key_path = self.fallback_key_file(base, token_ref);
        let key = read_key(&key_path)?;
        let content =
            std::fs::read_to_string(&token_path).context(ReadFileSnafu { path: &token_path })?;
        let encoded = content
            .strip_prefix(self.fallback_sentinel)
            .ok_or_else(|| SecretStoreError::InvalidEncryptedSecret {
                path: token_path.clone(),
                message: "missing sentinel",
            })?;
        let plaintext = decrypt(&key, encoded).map_err(|source| SecretStoreError::ReadFile {
            path: token_path.clone(),
            source,
        })?;
        String::from_utf8(plaintext)
            .context(Utf8Snafu { path: token_path })
            .map(|token| Some(SecretString::from(token)))
    }

    fn delete_fallback(&self, base: &Path, token_ref: &str) -> Result<(), SecretStoreError> {
        for path in [
            self.fallback_file(base, token_ref),
            self.fallback_key_file(base, token_ref),
        ] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(SecretStoreError::DeleteFile { path, source }),
            }
        }
        Ok(())
    }
}

fn safe_file_stem(token_ref: &str) -> String {
    token_ref
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn load_or_create_key(path: &Path) -> Result<Zeroizing<[u8; KEY_LEN]>, SecretStoreError> {
    match read_key(path) {
        Ok(key) => Ok(key),
        Err(SecretStoreError::ReadFile { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            let key = generate_key();
            write_secure_file(path, key.as_slice())?;
            Ok(key)
        }
        Err(err) => Err(err),
    }
}

// WHY: koilon's clippy policy pushes file I/O to `tokio::fs` or behind a
// trait, and this shared core carries that constraint since koilon depends
// on it. Secret loading is synchronous startup work on a fixed 32-byte
// file, before any runtime exists to await on, and the fallback path is
// already covered by tests through a tempdir. The sibling write goes
// through `bathron::atomic::write_atomic`; there is no atomic-read
// counterpart, and a wrapper that only forwards to `std::fs::read` would
// add indirection without adding a seam. `read_to_string` is not usable
// here — the key is raw random bytes, not UTF-8.
#[expect(
    clippy::disallowed_methods,
    reason = "synchronous startup read of a fixed-size non-UTF-8 key file"
)]
fn read_key(path: &Path) -> Result<Zeroizing<[u8; KEY_LEN]>, SecretStoreError> {
    let mut bytes = std::fs::read(path).context(ReadFileSnafu { path })?;
    // WHY Option rather than Result: the only failure `try_into` can report
    // here is a length mismatch, and `TryFromSliceError` carries no payload
    // describing it. Discarding it through `map_err(|_| ..)` is what
    // `clippy::map_err_ignore` objects to, and it is right that the shape
    // looks lossy — so drop to a form that has nothing to lose.
    let key = <[u8; KEY_LEN]>::try_from(bytes.as_slice()).ok();
    // WARNING: zeroize the read buffer regardless of outcome — it held a
    // copy of the key even on the length-check failure path below.
    bytes.zeroize();
    key.map(Zeroizing::new)
        .ok_or_else(|| SecretStoreError::InvalidEncryptedSecret {
            path: path.to_path_buf(),
            message: "encryption key file has wrong length",
        })
}

fn write_secure_file(path: &Path, bytes: &[u8]) -> Result<(), SecretStoreError> {
    let parent = path.parent().ok_or_else(|| SecretStoreError::CreateDir {
        path: path.to_path_buf(),
        source: std::io::Error::other("secret file has no parent directory"),
    })?;
    std::fs::create_dir_all(parent).context(CreateDirSnafu { path: parent })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .context(CreateDirSnafu { path: parent })?;
    }

    // WHY: 0o700 on the parent is this module's to keep — write_atomic
    // deliberately neither creates the parent nor sets its mode, so a
    // caller writing secret material keeps that; it guarantees 0o600 on
    // the file itself and replaces it atomically, which is the stronger of
    // the two write policies koilon and proskenion had independently
    // arrived at before #7027.
    bathron::atomic::write_atomic(path, bytes, Some(0o600)).map_err(|source| {
        SecretStoreError::WriteFile {
            path: path.to_path_buf(),
            source: std::io::Error::other(source),
        }
    })
}

fn generate_key() -> Zeroizing<[u8; KEY_LEN]> {
    Zeroizing::new(<[u8; KEY_LEN]>::generate())
}

fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> std::io::Result<String> {
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::<Aes256Gcm>::generate();

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_err| std::io::Error::other("AES-256-GCM encryption failed"))?;
    let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);
    Ok(koina::base64::encode(&combined))
}

fn decrypt(key: &[u8; KEY_LEN], encoded: &str) -> std::io::Result<Vec<u8>> {
    let combined = koina::base64::decode(encoded)
        .map_err(|_err| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid base64"))?;
    if combined.len() < NONCE_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "encrypted token is too short",
        ));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::<Aes256Gcm>::try_from(nonce_bytes).map_err(|_err| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid nonce length")
    })?;
    cipher.decrypt(&nonce, ciphertext).map_err(|_err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "AES-256-GCM authentication failed",
        )
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn test_store() -> TokenStore {
        // WHY false: these tests must deterministically exercise the
        // fallback path rather than depending on whatever keyring backend
        // (if any) happens to be present on the machine running them.
        TokenStore::new(
            "aletheia-secret-store-tests",
            "aletheia-test",
            "ALETHEIA_TEST_TOKEN_V1:",
            || false,
        )
    }

    #[test]
    fn fallback_round_trip_does_not_store_plaintext() {
        let store = test_store();
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_test";
        let token = "bearer-secret-4491";

        store.store_token(base, token_ref, token).unwrap();
        let restored = store.load_token(base, token_ref).unwrap();

        assert_eq!(
            restored.as_ref().map(SecretString::expose_secret),
            Some(token)
        );
        let raw = std::fs::read_to_string(store.fallback_file(base, token_ref)).unwrap();
        assert!(!raw.contains(token));
        assert!(raw.starts_with("ALETHEIA_TEST_TOKEN_V1:"));
    }

    /// `generate_key`/`read_key`/`load_or_create_key` are typed to return
    /// `Zeroizing<[u8; KEY_LEN]>` — this test would fail to compile if that
    /// regressed to a bare array. We cannot safely inspect memory after
    /// drop (that's UB), so — matching `koina::secret::SecretString`'s own
    /// test idiom — the runtime half proves the delegated `zeroize()` call
    /// (exactly what `Zeroizing::drop` invokes) actually clears the bytes.
    #[test]
    fn fallback_key_is_wrapped_for_zeroize_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test.key");

        let key: Zeroizing<[u8; KEY_LEN]> = load_or_create_key(&key_path).unwrap();
        assert!(
            key.iter().any(|&b| b != 0),
            "precondition: generated key is non-zero"
        );

        let mut copy = *key;
        copy.zeroize();
        assert!(
            copy.iter().all(|&b| b == 0),
            "zeroize should clear key bytes"
        );

        let reread: Zeroizing<[u8; KEY_LEN]> = read_key(&key_path).unwrap();
        assert_eq!(*reread, *key, "read_key must round-trip the stored key");
    }

    #[cfg(unix)]
    #[test]
    fn stored_secret_is_owner_only_on_every_write() {
        use std::os::unix::fs::PermissionsExt as _;

        let store = test_store();
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_modes";

        store.store_token(base, token_ref, "first").unwrap();

        let path = store.fallback_file(base, token_ref);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        store.store_token(base, token_ref, "second").unwrap();

        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            file_mode & 0o777,
            0o600,
            "secret file expected 0o600, got {:o}",
            file_mode & 0o777
        );

        let parent = path.parent().unwrap();
        let dir_mode = std::fs::metadata(parent).unwrap().permissions().mode();
        assert_eq!(
            dir_mode & 0o777,
            0o700,
            "secret dir expected 0o700, got {:o}",
            dir_mode & 0o777
        );
    }

    #[test]
    fn delete_token_removes_fallback_files() {
        let store = test_store();
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_delete";

        store.store_token(base, token_ref, "secret").unwrap();
        assert!(store.fallback_file(base, token_ref).exists());
        assert!(store.fallback_key_file(base, token_ref).exists());

        store.delete_token(base, token_ref).unwrap();

        assert!(!store.fallback_file(base, token_ref).exists());
        assert!(!store.fallback_key_file(base, token_ref).exists());
    }

    #[test]
    fn load_token_missing_returns_none() {
        let store = test_store();
        let dir = tempfile::tempdir().unwrap();
        assert!(
            store
                .load_token(dir.path(), "missing-ref")
                .unwrap()
                .is_none()
        );
    }
}
