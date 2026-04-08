//! AES-256-GCM encryption for credential files at rest.
//!
//! Encrypted files are prefixed with [`ENCRYPTED_SENTINEL`] so that
//! plaintext files (written by older versions) can still be read.
//! All new writes are encrypted.
//!
//! Key management: a 32-byte random encryption key is stored in a sidecar
//! file (`<credential-path>.key`, mode 0600). This means the key and the
//! ciphertext live in separate files, so copying one without the other
//! leaves the credential inaccessible.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, AeadCore, OsRng};
use aes_gcm::{Aes256Gcm, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

/// Magic prefix that marks an encrypted credential file.
pub(crate) const ENCRYPTED_SENTINEL: &str = "ALETHEIA_ENC_V1:";

/// AES-256-GCM nonce length (96 bits / 12 bytes per NIST recommendation).
const NONCE_LEN: usize = 12;

/// AES-256-GCM key length (256 bits / 32 bytes).
const KEY_LEN: usize = 32;

/// Derive the key-file path from the credential file path.
///
/// The key file sits alongside the credential file with a `.key` extension
/// appended: `/path/to/.credentials.json` → `/path/to/.credentials.json.key`.
#[must_use]
pub(crate) fn key_file_path(credential_path: &Path) -> PathBuf {
    let mut key_path = credential_path.as_os_str().to_owned();
    key_path.push(".key");
    PathBuf::from(key_path)
}

/// Load or generate the encryption key for the given credential file.
///
/// If the key file does not yet exist, generates a new 32-byte random key,
/// writes it (mode 0600 on Unix), and returns it.
///
/// # Errors
///
/// Returns an `io::Error` if the key file cannot be read or written.
pub(crate) fn load_or_create_key(credential_path: &Path) -> std::io::Result<[u8; KEY_LEN]> {
    let key_path = key_file_path(credential_path);

    if key_path.exists() {
        #[expect(
            clippy::disallowed_methods,
            reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
        )]
        let bytes = std::fs::read(&key_path)?;
        let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_ignored| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "encryption key file has wrong length (expected 32 bytes)",
            )
        })?;
        return Ok(key);
    }

    let key = generate_key()?;
    write_key_file(&key_path, &key)?;
    Ok(key)
}

/// Load an existing key or generate one without persisting.
///
/// Returns `(key, needs_persist)` -- the caller must call [`write_key_file_atomic`]
/// after both the key and credential temp files are ready.
pub(crate) fn load_or_generate_key(
    credential_path: &Path,
) -> std::io::Result<([u8; KEY_LEN], bool)> {
    let key_path = key_file_path(credential_path);
    if key_path.exists() {
        #[expect(
            clippy::disallowed_methods,
            reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
        )]
        let bytes = std::fs::read(&key_path)?;
        let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_ignored| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "encryption key file has wrong length (expected 32 bytes)",
            )
        })?;
        Ok((key, false))
    } else {
        let key = generate_key()?;
        Ok((key, true))
    }
}

/// Write a key to a temp file, fsync, and return the temp path for later rename.
///
/// # Errors
///
/// Returns an `io::Error` if temp file creation or write fails.
pub(crate) fn prepare_key_file(
    credential_path: &Path,
    key: &[u8; KEY_LEN],
) -> std::io::Result<TempFileGuard> {
    let key_path = key_file_path(credential_path);
    let tmp = key_path.with_extension("key.tmp");
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(key)?;
    f.flush()?;
    f.sync_all()?;
    Ok(TempFileGuard::new(tmp))
}

/// Drop guard that deletes a temp file if not defused.
///
/// WHY: prevents temp file leaks if the caller panics between
/// `prepare_key_file` and `commit_key_file`. Call [`defuse`](Self::defuse)
/// after the commit succeeds to prevent deletion. Closes #2745.
pub(crate) struct TempFileGuard {
    path: Option<PathBuf>,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// The temp file path.
    pub(crate) fn path(&self) -> &Path {
        self.path.as_deref().expect("guard already defused")
    }

    /// Prevent the guard from deleting the file on drop.
    pub(crate) fn defuse(&mut self) {
        self.path = None;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(ref path) = self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Rename a prepared key temp file to its final path with mode 0600.
///
/// # Errors
///
/// Returns an `io::Error` if the rename or permission set fails.
pub(crate) fn commit_key_file(credential_path: &Path, tmp: &Path) -> std::io::Result<()> {
    let key_path = key_file_path(credential_path);
    std::fs::rename(tmp, &key_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Encrypt `plaintext` using AES-256-GCM with a fresh random nonce.
///
/// Returns base64-encoded `nonce || ciphertext_with_tag`.
///
/// # Errors
///
/// Returns an `io::Error` if the AEAD primitive fails.
pub(crate) fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> std::io::Result<String> {
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext_with_tag = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_ignored| std::io::Error::other("AES-256-GCM: encryption failed"))?;

    // Prepend nonce so the decryptor can recover it.
    let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext_with_tag.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext_with_tag);

    Ok(BASE64.encode(&combined))
}

/// Decrypt a base64-encoded `nonce || ciphertext_with_tag` produced by [`encrypt`].
///
/// # Errors
///
/// Returns an `io::Error` if base64 decoding or AES-GCM authentication fails.
pub(crate) fn decrypt(key: &[u8; KEY_LEN], encoded: &str) -> std::io::Result<Vec<u8>> {
    let combined = BASE64.decode(encoded).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("base64 decode failed: {e}"),
        )
    })?;

    if combined.len() < NONCE_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "encrypted credential too short to contain nonce",
        ));
    }

    let (nonce_bytes, ciphertext_with_tag) = combined.split_at(NONCE_LEN);

    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let nonce = GenericArray::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext_with_tag)
        .map_err(|_ignored| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "AES-256-GCM authentication failed (wrong key or corrupted ciphertext)",
            )
        })?;

    Ok(plaintext)
}

/// Generate a fresh random 32-byte AES-256-GCM key using the system CSPRNG.
fn generate_key() -> std::io::Result<[u8; KEY_LEN]> {
    let mut key = [0u8; KEY_LEN];
    rand::fill(&mut key);
    Ok(key)
}

/// Write the encryption key to disk with restrictive permissions.
fn write_key_file(path: &Path, key: &[u8; KEY_LEN]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension("key.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(key)?;
    f.flush()?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn fixture_key() -> [u8; KEY_LEN] {
        let mut k = [0u8; KEY_LEN];
        for (i, b) in k.iter_mut().enumerate() {
            // NOTE: i is in 0..KEY_LEN (32), which fits in u8.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::as_conversions,
                reason = "i < KEY_LEN = 32, fits in u8"
            )]
            {
                *b = i as u8;
            }
        }
        k
    }

    #[test]
    fn encrypt_then_decrypt_roundtrip() {
        let key = fixture_key();
        let plaintext = b"hello, credential world";
        let encoded = encrypt(&key, plaintext).unwrap();
        let decoded = decrypt(&key, &encoded).unwrap();
        assert_eq!(
            decoded, plaintext,
            "roundtrip must recover original plaintext"
        );
    }

    #[test]
    fn different_nonces_produce_different_ciphertexts() {
        let key = fixture_key();
        let plaintext = b"same plaintext";
        let enc1 = encrypt(&key, plaintext).unwrap();
        let enc2 = encrypt(&key, plaintext).unwrap();
        assert_ne!(enc1, enc2, "each encrypt call must use a fresh nonce");
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let key = fixture_key();
        let mut bad_key = fixture_key();
        bad_key[0] ^= 0xFF;
        let encoded = encrypt(&key, b"secret").unwrap();
        let result = decrypt(&bad_key, &encoded);
        assert!(result.is_err(), "wrong key must not decrypt successfully");
    }

    #[test]
    fn tampered_ciphertext_fails_decryption() {
        let key = fixture_key();
        let mut encoded = encrypt(&key, b"secret data").unwrap();
        // NOTE: flip the last character to corrupt the GCM authentication tag.
        if let Some(c) = encoded.pop() {
            let flipped = if c == 'A' { 'B' } else { 'A' };
            encoded.push(flipped);
        }
        let result = decrypt(&key, &encoded);
        assert!(
            result.is_err(),
            "tampered ciphertext must fail authentication"
        );
    }

    #[test]
    fn key_file_path_appends_dot_key() {
        let cred = Path::new("/home/alice/.claude/.credentials.json");
        let key = key_file_path(cred);
        assert_eq!(key, Path::new("/home/alice/.claude/.credentials.json.key"));
    }

    #[test]
    fn load_or_create_key_is_stable_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let cred_path = dir.path().join("creds.json");
        let key1 = load_or_create_key(&cred_path).unwrap();
        let key2 = load_or_create_key(&cred_path).unwrap();
        assert_eq!(key1, key2, "same key file must yield the same key twice");
    }
}
