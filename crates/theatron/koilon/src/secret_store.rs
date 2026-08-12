//! TUI bearer-token secret storage (#5321).
//!
//! `tui.toml` previously stored the connection bearer token as plaintext.
//! Tokens are now written to the OS keyring first; if that backend is
//! unavailable, the TUI falls back to an AES-256-GCM encrypted file under
//! the `aletheia` config directory. `ConfigFile` stores only the stable
//! non-secret `token_ref` marker — never the raw token.
//!
//! koilon manages exactly one connection at a time (unlike proskenion's
//! multi-server desktop store), so there is a single fixed keyring account
//! and fallback file pair rather than a per-server reference.
//!
//! WARNING: the fallback's AES key is stored beside its ciphertext, both
//! `0o600` inside a `0o700` directory. Confidentiality therefore rests on
//! filesystem permissions, not on the cipher — anyone who can read the
//! ciphertext can read the key. What the fallback buys over the previous
//! plaintext field is that the token no longer lives in `tui.toml`, the file
//! operators paste into bug reports and copy between hosts. The keyring path
//! is the one that offers real at-rest protection; the fallback exists so a
//! headless box without a keyring backend still avoids plaintext-in-config.

use std::path::{Path, PathBuf};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead as _, AeadCore as _, OsRng};
use aes_gcm::{Aes256Gcm, KeyInit as _};
use snafu::{ResultExt as _, Snafu};

/// Errors from TUI secret storage.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub(crate) enum SecretStoreError {
    /// Failed to create a secret-storage directory.
    #[snafu(display("failed to create secret directory {}: {source}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to read a fallback secret file.
    #[snafu(display("failed to read secret file {}: {source}", path.display()))]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to write a fallback secret file.
    #[snafu(display("failed to write secret file {}: {source}", path.display()))]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to remove a fallback secret file.
    #[snafu(display("failed to delete secret file {}: {source}", path.display()))]
    DeleteFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Fallback encrypted secret file has an invalid format.
    #[snafu(display("invalid encrypted secret in {}: {message}", path.display()))]
    InvalidEncryptedSecret {
        path: PathBuf,
        message: &'static str,
    },

    /// Decrypted secret bytes were not valid UTF-8.
    #[snafu(display("decrypted secret in {} is not valid UTF-8: {source}", path.display()))]
    Utf8 {
        path: PathBuf,
        source: std::string::FromUtf8Error,
    },
}

const KEYRING_SERVICE: &str = "aletheia-tui";

/// The keyring account name and fallback-file stem for the single TUI
/// connection token. Also the value written into `ConfigFile.token_ref` to
/// mark that a token is stored via this module rather than in plaintext.
pub(crate) const TOKEN_REF: &str = "tui-default";

const FALLBACK_DIR: &str = "secrets";
const FALLBACK_SENTINEL: &str = "ALETHEIA_TUI_TOKEN_V1:";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Store the TUI bearer token in the OS keyring or encrypted fallback.
///
/// # Errors
///
/// Returns an error if the OS keyring is unavailable and encrypted fallback
/// storage cannot be written.
pub(crate) fn store_token(base: &Path, token: &str) -> Result<(), SecretStoreError> {
    if try_store_keyring(token) {
        if let Err(err) = delete_fallback(base) {
            tracing::warn!(error = %err, "failed to remove stale encrypted fallback TUI token");
        }
        return Ok(());
    }

    write_fallback(base, token)
}

/// Load the TUI bearer token.
///
/// # Errors
///
/// Returns an error if encrypted fallback data exists but cannot be read or
/// decrypted.
pub(crate) fn load_token(base: &Path) -> Result<Option<String>, SecretStoreError> {
    match try_load_keyring() {
        KeyringLoad::Found(token) => return Ok(Some(token)),
        KeyringLoad::Missing => {}
        KeyringLoad::Unavailable => {
            tracing::debug!("TUI token keyring unavailable, trying encrypted fallback");
        }
    }

    read_fallback(base)
}

/// Delete the TUI bearer token from keyring and fallback storage.
///
/// # Errors
///
/// Returns an error only when removing local encrypted fallback files fails.
pub(crate) fn delete_token(base: &Path) -> Result<(), SecretStoreError> {
    if let Err(err) = delete_keyring() {
        tracing::debug!(error = %err, "TUI token keyring delete skipped");
    }
    delete_fallback(base)
}

enum KeyringLoad {
    Found(String),
    Missing,
    Unavailable,
}

fn keyring_enabled() -> bool {
    !cfg!(test)
}

fn keyring_entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, TOKEN_REF)
}

fn try_store_keyring(token: &str) -> bool {
    if !keyring_enabled() {
        return false;
    }

    match keyring_entry().and_then(|entry| entry.set_password(token)) {
        Ok(()) => true,
        Err(err) => {
            tracing::debug!(error = %err, "TUI token keyring write failed, using encrypted fallback");
            false
        }
    }
}

fn try_load_keyring() -> KeyringLoad {
    if !keyring_enabled() {
        return KeyringLoad::Unavailable;
    }

    let entry = match keyring_entry() {
        Ok(entry) => entry,
        Err(err) => {
            tracing::debug!(error = %err, "TUI token keyring entry unavailable");
            return KeyringLoad::Unavailable;
        }
    };

    match entry.get_password() {
        Ok(token) if token.is_empty() => KeyringLoad::Missing,
        Ok(token) => KeyringLoad::Found(token),
        Err(keyring::Error::NoEntry) => KeyringLoad::Missing,
        Err(err) => {
            tracing::debug!(error = %err, "TUI token keyring read failed");
            KeyringLoad::Unavailable
        }
    }
}

fn delete_keyring() -> Result<(), keyring::Error> {
    if !keyring_enabled() {
        return Ok(());
    }

    match keyring_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err),
    }
}

fn fallback_dir(base: &Path) -> PathBuf {
    base.join("aletheia").join(FALLBACK_DIR)
}

fn fallback_file(base: &Path) -> PathBuf {
    fallback_dir(base).join(format!("{TOKEN_REF}.token"))
}

fn fallback_key_file(base: &Path) -> PathBuf {
    fallback_dir(base).join(format!("{TOKEN_REF}.key"))
}

fn write_fallback(base: &Path, token: &str) -> Result<(), SecretStoreError> {
    let key_path = fallback_key_file(base);
    let token_path = fallback_file(base);
    let key = load_or_create_key(&key_path)?;
    let encrypted =
        encrypt(&key, token.as_bytes()).map_err(|source| SecretStoreError::WriteFile {
            path: token_path.clone(),
            source,
        })?;
    let payload = format!("{FALLBACK_SENTINEL}{encrypted}");
    write_secure_file(&token_path, payload.as_bytes())
}

fn read_fallback(base: &Path) -> Result<Option<String>, SecretStoreError> {
    let token_path = fallback_file(base);
    if !token_path.exists() {
        return Ok(None);
    }

    let key_path = fallback_key_file(base);
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

fn delete_fallback(base: &Path) -> Result<(), SecretStoreError> {
    for path in [fallback_file(base), fallback_key_file(base)] {
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

// WHY: koilon's clippy policy pushes file I/O to `tokio::fs` or behind a
// trait. Secret loading is synchronous startup work on a fixed 32-byte file,
// before any runtime exists to await on, and the fallback path is already
// covered by tests through a tempdir. The sibling write goes through
// `koina::fs::write_restricted`; `koina::fs` exposes no read counterpart, and
// a wrapper that only forwards to `std::fs::read` would add indirection
// without adding a seam. `read_to_string` is not usable here — the key is
// raw random bytes, not UTF-8.
#[expect(
    clippy::disallowed_methods,
    reason = "synchronous startup read of a fixed-size non-UTF-8 key file"
)]
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

    // WHY: 0o700 on the parent is this module's to keep — koina::fs::write_restricted
    // deliberately only guarantees 0o600 on the file itself.
    koina::fs::write_restricted(path, bytes).map_err(|source| SecretStoreError::WriteFile {
        path: path.to_path_buf(),
        source,
    })
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
        let token = "bearer-secret-4491";

        store_token(base, token).unwrap();
        let restored = load_token(base).unwrap();

        assert_eq!(restored.as_deref(), Some(token));
        let raw = std::fs::read_to_string(fallback_file(base)).unwrap();
        assert!(!raw.contains(token));
        assert!(raw.starts_with(FALLBACK_SENTINEL));
    }

    #[cfg(unix)]
    #[test]
    fn stored_secret_is_owner_only_on_every_write() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        store_token(base, "first").unwrap();

        let path = fallback_file(base);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        store_token(base, "second").unwrap();

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

        store_token(base, "secret").unwrap();
        assert!(fallback_file(base).exists());
        assert!(fallback_key_file(base).exists());

        delete_token(base).unwrap();

        assert!(!fallback_file(base).exists());
        assert!(!fallback_key_file(base).exists());
    }

    #[test]
    fn load_token_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_token(dir.path()).unwrap().is_none());
    }
}
