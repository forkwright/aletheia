//! Desktop bearer-token secret storage.
//!
//! Tokens are written to the OS keyring first. If that backend is unavailable,
//! the desktop falls back to AES-256-GCM encrypted files under the desktop
//! config directory. TOML settings store only stable non-secret references.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead as _, AeadCore as _, OsRng};
use aes_gcm::{Aes256Gcm, KeyInit as _};
use snafu::{ResultExt as _, Snafu};

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
pub(crate) fn load_token(base: &Path, token_ref: &str) -> Result<Option<String>, SecretStoreError> {
    match try_load_keyring(token_ref) {
        KeyringLoad::Found(token) => return Ok(Some(token)),
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

fn read_fallback(base: &Path, token_ref: &str) -> Result<Option<String>, SecretStoreError> {
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
        .map(Some)
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

fn load_or_create_key(path: &Path) -> Result<[u8; KEY_LEN], SecretStoreError> {
    match read_key(path) {
        Ok(key) => Ok(key),
        Err(SecretStoreError::ReadFile { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            let key = generate_key();
            write_secure_file(path, &key)?;
            Ok(key)
        }
        Err(err) => Err(err),
    }
}

fn read_key(path: &Path) -> Result<[u8; KEY_LEN], SecretStoreError> {
    let bytes = std::fs::read(path).context(ReadFileSnafu { path })?;
    bytes
        .try_into()
        .map_err(|_bytes: Vec<u8>| SecretStoreError::InvalidEncryptedSecret {
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

    #[cfg(unix)]
    let mut tmp = {
        use std::os::unix::fs::PermissionsExt as _;
        let perms = std::fs::Permissions::from_mode(0o600);
        tempfile::Builder::new()
            .permissions(perms)
            .tempfile_in(parent)
            .context(WriteFileSnafu { path })?
    };
    #[cfg(not(unix))]
    let mut tmp = tempfile::Builder::new()
        .tempfile_in(parent)
        .context(WriteFileSnafu { path })?;

    tmp.write_all(bytes).context(WriteFileSnafu { path })?;
    tmp.as_file().sync_all().context(WriteFileSnafu { path })?;
    tmp.persist(path)
        .map_err(|err| err.error)
        .context(WriteFileSnafu { path })?;
    Ok(())
}

fn generate_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut OsRng, &mut key);
    key
}

fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> std::io::Result<String> {
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

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
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let nonce = GenericArray::from_slice(nonce_bytes);
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

        assert_eq!(restored.as_deref(), Some(token));
        let raw = std::fs::read_to_string(fallback_file(base, token_ref)).unwrap();
        assert!(!raw.contains(token));
        assert!(raw.starts_with(FALLBACK_SENTINEL));
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
