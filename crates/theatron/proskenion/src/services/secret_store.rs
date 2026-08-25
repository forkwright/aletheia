//! Desktop bearer-token secret storage.
//!
//! Tokens are written to the OS keyring first. If that backend is unavailable,
//! the desktop falls back to AES-256-GCM encrypted files under the desktop
//! config directory. TOML settings store only stable non-secret references.
//!
//! NOTE: the fallback AES key and the decrypted token are held in
//! [`zeroize::Zeroizing`] / [`koina::secret::SecretString`], both of which
//! wipe their backing buffer on drop. This narrows, but does not close, the
//! residual-memory window — a buffer that was copied or reallocated before
//! wrapping (e.g. an intermediate `Vec` from a resize) leaves copies the
//! wrapper never touches, and the OS may still have paged either buffer to
//! swap before it was wiped.

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead as _, Generate as _, Key, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit as _};
use koina::secret::SecretString;
use snafu::{ResultExt as _, Snafu};
use zeroize::{Zeroize as _, Zeroizing};

/// Errors from desktop secret storage.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub(crate) enum SecretStoreError {
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

const KEYRING_SERVICE: &str = "aletheia-desktop";
const FALLBACK_DIR: &str = "secrets";
const FALLBACK_SENTINEL: &str = "ALETHEIA_DESKTOP_TOKEN_V1:";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Store a bearer token in the OS keyring or encrypted fallback.
///
/// # Errors
///
/// Returns an error if the OS keyring is unavailable and encrypted fallback
/// storage cannot be written.
pub(crate) fn store_token(
    base: &Path,
    token_ref: &str,
    token: &str,
) -> Result<(), SecretStoreError> {
    if try_store_keyring(token_ref, token) {
        if let Err(err) = delete_fallback(base, token_ref) {
            tracing::warn!(error = %err, token_ref, "failed to remove stale encrypted fallback token");
        }
        return Ok(());
    }

    write_fallback(base, token_ref, token)
}

/// Load a bearer token by reference.
///
/// # Errors
///
/// Returns an error if encrypted fallback data exists but cannot be read or
/// decrypted.
pub(crate) fn load_token(
    base: &Path,
    token_ref: &str,
) -> Result<Option<SecretString>, SecretStoreError> {
    match try_load_keyring(token_ref) {
        KeyringLoad::Found(token) => return Ok(Some(SecretString::from(token))),
        KeyringLoad::Missing => {}
        KeyringLoad::Unavailable => {
            tracing::debug!(
                token_ref,
                "desktop token keyring unavailable, trying encrypted fallback"
            );
        }
    }

    read_fallback(base, token_ref)
}

/// Delete a bearer token from keyring and fallback storage.
///
/// # Errors
///
/// Returns an error only when removing local encrypted fallback files fails.
pub(crate) fn delete_token(base: &Path, token_ref: &str) -> Result<(), SecretStoreError> {
    if let Err(err) = delete_keyring(token_ref) {
        tracing::debug!(error = %err, token_ref, "desktop token keyring delete skipped");
    }
    delete_fallback(base, token_ref)
}

enum KeyringLoad {
    Found(String),
    Missing,
    Unavailable,
}

fn keyring_enabled() -> bool {
    !cfg!(test)
}

fn keyring_entry(token_ref: &str) -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, token_ref)
}

fn try_store_keyring(token_ref: &str, token: &str) -> bool {
    if !keyring_enabled() {
        return false;
    }

    match keyring_entry(token_ref).and_then(|entry| entry.set_password(token)) {
        Ok(()) => true,
        Err(err) => {
            tracing::debug!(error = %err, token_ref, "desktop token keyring write failed, using encrypted fallback");
            false
        }
    }
}

fn try_load_keyring(token_ref: &str) -> KeyringLoad {
    if !keyring_enabled() {
        return KeyringLoad::Unavailable;
    }

    let entry = match keyring_entry(token_ref) {
        Ok(entry) => entry,
        Err(err) => {
            tracing::debug!(error = %err, token_ref, "desktop token keyring entry unavailable");
            return KeyringLoad::Unavailable;
        }
    };

    match entry.get_password() {
        Ok(token) if token.is_empty() => KeyringLoad::Missing,
        Ok(token) => KeyringLoad::Found(token),
        Err(keyring::Error::NoEntry) => KeyringLoad::Missing,
        Err(err) => {
            tracing::debug!(error = %err, token_ref, "desktop token keyring read failed");
            KeyringLoad::Unavailable
        }
    }
}

fn delete_keyring(token_ref: &str) -> Result<(), keyring::Error> {
    if !keyring_enabled() {
        return Ok(());
    }

    match keyring_entry(token_ref)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err),
    }
}

fn fallback_dir(base: &Path) -> PathBuf {
    base.join("aletheia-desktop").join(FALLBACK_DIR)
}

fn fallback_file(base: &Path, token_ref: &str) -> PathBuf {
    fallback_dir(base).join(format!("{}.token", safe_file_stem(token_ref)))
}

fn fallback_key_file(base: &Path, token_ref: &str) -> PathBuf {
    fallback_dir(base).join(format!("{}.key", safe_file_stem(token_ref)))
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

fn write_fallback(base: &Path, token_ref: &str, token: &str) -> Result<(), SecretStoreError> {
    let key_path = fallback_key_file(base, token_ref);
    let token_path = fallback_file(base, token_ref);
    let key = load_or_create_key(&key_path)?;
    let encrypted =
        encrypt(&key, token.as_bytes()).map_err(|source| SecretStoreError::WriteFile {
            path: token_path.clone(),
            source,
        })?;
    let payload = format!("{FALLBACK_SENTINEL}{encrypted}");
    write_secure_file(&token_path, payload.as_bytes())
}

fn read_fallback(base: &Path, token_ref: &str) -> Result<Option<SecretString>, SecretStoreError> {
    let token_path = fallback_file(base, token_ref);
    if !token_path.exists() {
        return Ok(None);
    }

    let key_path = fallback_key_file(base, token_ref);
    let key = read_key(&key_path)?;
    let content =
        std::fs::read_to_string(&token_path).context(ReadFileSnafu { path: &token_path })?;
    let encoded = content.strip_prefix(FALLBACK_SENTINEL).ok_or_else(|| {
        SecretStoreError::InvalidEncryptedSecret {
            path: token_path.clone(),
            message: "missing sentinel",
        }
    })?;
    let plaintext = decrypt(&key, encoded).map_err(|source| SecretStoreError::ReadFile {
        path: token_path.clone(),
        source,
    })?;
    String::from_utf8(plaintext)
        .context(Utf8Snafu { path: token_path })
        .map(|token| Some(SecretString::from(token)))
}

fn delete_fallback(base: &Path, token_ref: &str) -> Result<(), SecretStoreError> {
    for path in [
        fallback_file(base, token_ref),
        fallback_key_file(base, token_ref),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(SecretStoreError::DeleteFile { path, source }),
        }
    }
    Ok(())
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

    // WHY: 0o600 on the secret file itself; the 0o700 on its parent above stays
    // this function's decision — `write_atomic` deliberately neither creates the
    // parent nor sets its mode, so a caller writing secret material keeps that.
    bathron::atomic::write_atomic(path, bytes, Some(0o600)).map_err(|source| {
        SecretStoreError::WriteFile {
            path: path.to_path_buf(),
            source: std::io::Error::other(source),
        }
    })?;
    Ok(())
}

fn generate_key() -> Zeroizing<[u8; KEY_LEN]> {
    Zeroizing::new(<[u8; KEY_LEN]>::generate())
}

fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> std::io::Result<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
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
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::<Aes256Gcm>::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).map_err(|_err| {
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

    #[test]
    fn fallback_round_trip_does_not_store_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_test";
        let token = "bearer-secret-4491";

        store_token(base, token_ref, token).unwrap();
        let restored = load_token(base, token_ref).unwrap();

        assert_eq!(
            restored.as_ref().map(SecretString::expose_secret),
            Some(token)
        );
        let raw = std::fs::read_to_string(fallback_file(base, token_ref)).unwrap();
        assert!(!raw.contains(token));
        assert!(raw.starts_with(FALLBACK_SENTINEL));
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

    /// The secret file is owner-only and its directory owner-only-traversable,
    /// on a replacement as well as a first write. `write_atomic` deliberately
    /// does not touch the parent's mode, so the 0o700 is this module's to keep.
    #[cfg(unix)]
    #[test]
    fn stored_secret_is_owner_only_on_every_write() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_modes";

        store_token(base, token_ref, "first").unwrap();

        let path = fallback_file(base, token_ref);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        store_token(base, token_ref, "second").unwrap();

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
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let token_ref = "server-srv_delete";

        store_token(base, token_ref, "secret").unwrap();
        assert!(fallback_file(base, token_ref).exists());
        assert!(fallback_key_file(base, token_ref).exists());

        delete_token(base, token_ref).unwrap();

        assert!(!fallback_file(base, token_ref).exists());
        assert!(!fallback_key_file(base, token_ref).exists());
    }
}
