//! Encryption at rest for sensitive configuration fields.
//!
//! Sensitive values in `aletheia.toml` (API keys, signing keys, secrets) can be
//! encrypted with a primary key. Encrypted values are stored with an `enc:` prefix
//! followed by base64-encoded ciphertext. On config load, `enc:` values are
//! transparently decrypted. If the primary key is missing, encrypted values pass
//! through as-is with a warning.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use snafu::ResultExt;
use tracing::warn;

use crate::error::{self, Result};

/// Prefix marking an encrypted config value.
pub(crate) const ENCRYPTED_PREFIX: &str = "enc:";

/// Length of the ChaCha20-Poly1305 key in bytes.
const KEY_LEN: usize = 32;

/// Default path for the primary key file.
fn default_key_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("aletheia")
            .join("primary.key")
    })
}

/// Resolve the primary key file path.
///
/// Checks `ALETHEIA_PRIMARY_KEY` env var first, then falls back to
/// `~/.config/aletheia/primary.key`.
#[must_use]
pub fn primary_key_path() -> Option<PathBuf> {
    std::env::var_os("ALETHEIA_PRIMARY_KEY")
        .map(PathBuf::from)
        .or_else(default_key_path)
}

/// Load the 32-byte primary key from the key file.
///
/// The file must contain exactly 64 hex characters (32 bytes).
/// Returns `None` if the file does not exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or contains
/// invalid hex data.
#[expect(
    clippy::result_large_err,
    reason = "taxis Error is inherently large due to PathBuf fields"
)]
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn load_primary_key(path: &Path) -> Result<Option<[u8; KEY_LEN]>> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(path).context(error::ReadConfigSnafu {
        path: path.to_path_buf(),
    })?;
    let hex = contents.trim();
    parse_hex_key(hex, path)
}

#[expect(
    clippy::result_large_err,
    reason = "taxis Error is inherently large due to PathBuf fields"
)]
fn parse_hex_key(hex: &str, path: &Path) -> Result<Option<[u8; KEY_LEN]>> {
    if hex.len() != KEY_LEN * 2 {
        return Err(error::InvalidPrimaryKeySnafu {
            path: path.to_path_buf(),
            reason: format!("expected {} hex characters, got {}", KEY_LEN * 2, hex.len()),
        }
        .build());
    }

    let mut key = [0u8; KEY_LEN];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        // chunks(2) on a string of even length always yields 2-element slices;
        // i is bounded by KEY_LEN because hex.len() == KEY_LEN * 2
        #[expect(
            clippy::indexing_slicing,
            reason = "chunks(2) yields 2-element slices; i < KEY_LEN"
        )]
        let hi = hex_digit(chunk[0]).ok_or_else(|| {
            error::InvalidPrimaryKeySnafu {
                path: path.to_path_buf(),
                reason: format!("invalid hex character at position {}", i * 2),
            }
            .build()
        })?;
        #[expect(clippy::indexing_slicing, reason = "chunks(2) yields 2-element slices")]
        let lo = hex_digit(chunk[1]).ok_or_else(|| {
            error::InvalidPrimaryKeySnafu {
                path: path.to_path_buf(),
                reason: format!("invalid hex character at position {}", i * 2 + 1),
            }
            .build()
        })?;
        #[expect(
            clippy::indexing_slicing,
            reason = "i < KEY_LEN because hex.len() == KEY_LEN * 2"
        )]
        {
            key[i] = (hi << 4) | lo;
        }
    }

    Ok(Some(key))
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        // b >> 4 is 0..=15 and b & 0x0f is 0..=15; the array has exactly 16 elements
        #[expect(
            clippy::indexing_slicing,
            reason = "nibble value 0..=15 always indexes into 16-element array"
        )]
        s.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
        #[expect(
            clippy::indexing_slicing,
            reason = "nibble value 0..=15 always indexes into 16-element array"
        )]
        s.push(char::from(b"0123456789abcdef"[usize::from(b & 0x0f)]));
    }
    s
}

/// Generate a new random primary key and write it to the given path.
///
/// Creates parent directories and sets file permissions to 0600.
///
/// # Errors
///
/// Returns an error if the file already exists, the directory cannot be
/// created, or the file cannot be written.
#[expect(
    clippy::result_large_err,
    reason = "taxis Error is inherently large due to PathBuf fields"
)]
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn generate_primary_key(path: &Path) -> Result<()> {
    if path.exists() {
        return Err(error::PrimaryKeyExistsSnafu {
            path: path.to_path_buf(),
        }
        .build());
    }

    let rng = SystemRandom::new();
    let mut key = [0u8; KEY_LEN];
    // WHY: ring::error::Unspecified has no useful fields to propagate
    rng.fill(&mut key).map_err(|_unspecified| {
        error::EncryptSnafu {
            reason: "failed to generate random key".to_owned(),
        }
        .build()
    })?;

    let hex = to_hex(&key);
    aletheia_koina::fs::write_restricted(path, hex.as_bytes()).context(
        error::WriteConfigSnafu {
            path: path.to_path_buf(),
        },
    )?;

    Ok(())
}

/// Whether a string value is encrypted (starts with `enc:`).
#[must_use]
pub(crate) fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENCRYPTED_PREFIX)
}

/// Encrypt a plaintext string value using ChaCha20-Poly1305.
///
/// Returns `enc:` + base64(nonce || ciphertext+tag).
///
/// # Errors
///
/// Returns an error if encryption fails.
#[expect(
    clippy::result_large_err,
    reason = "taxis Error is inherently large due to PathBuf fields"
)]
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub(crate) fn encrypt_value(plaintext: &str, primary_key: &[u8; KEY_LEN]) -> Result<String> {
    // WHY: ring::error::Unspecified has no useful fields to propagate
    let unbound = UnboundKey::new(&CHACHA20_POLY1305, primary_key)
        .map_err(|_unspecified| build_encrypt_error())?;
    let key = LessSafeKey::new(unbound);

    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_unspecified| build_encrypt_error())?;

    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let mut in_out = plaintext.as_bytes().to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_unspecified| build_encrypt_error())?;

    // NOTE: nonce || ciphertext+tag
    let mut payload = Vec::with_capacity(NONCE_LEN + in_out.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&in_out);

    let encoded = base64::engine::general_purpose::STANDARD.encode(&payload);
    Ok(format!("{ENCRYPTED_PREFIX}{encoded}"))
}

/// Decrypt an `enc:`-prefixed string value.
///
/// # Errors
///
/// Returns an error if the value is malformed or decryption fails.
#[expect(
    clippy::result_large_err,
    reason = "taxis Error is inherently large due to PathBuf fields"
)]
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub(crate) fn decrypt_value(encrypted: &str, primary_key: &[u8; KEY_LEN]) -> Result<String> {
    let encoded = encrypted
        .strip_prefix(ENCRYPTED_PREFIX)
        .ok_or_else(|| build_decrypt_error("missing enc: prefix"))?;

    // WHY: base64::DecodeError is not useful to propagate through taxis Error
    let payload = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_decode_err| build_decrypt_error("invalid base64"))?;

    if payload.len() < NONCE_LEN + CHACHA20_POLY1305.tag_len() {
        return Err(build_decrypt_error("ciphertext too short"));
    }

    let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
    // WHY: ring::error::Unspecified has no useful fields to propagate
    let nonce = Nonce::try_assume_unique_for_key(nonce_bytes)
        .map_err(|_unspecified| build_decrypt_error("invalid nonce"))?;

    let unbound = UnboundKey::new(&CHACHA20_POLY1305, primary_key)
        .map_err(|_unspecified| build_decrypt_error("invalid key"))?;
    let key = LessSafeKey::new(unbound);

    let mut in_out = ciphertext.to_vec();
    let plaintext = key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_unspecified| {
            build_decrypt_error("decryption failed (wrong key or corrupted data)")
        })?;

    String::from_utf8(plaintext.to_vec())
        .map_err(|_utf8_err| build_decrypt_error("decrypted value is not valid UTF-8"))
}

fn build_encrypt_error() -> error::Error {
    error::EncryptSnafu {
        reason: "encryption operation failed".to_owned(),
    }
    .build()
}

fn build_decrypt_error(reason: &str) -> error::Error {
    error::DecryptSnafu {
        reason: reason.to_owned(),
    }
    .build()
}

/// Encrypt sensitive plaintext values in a TOML config file in place.
///
/// Reads the file, encrypts sensitive string values that aren't already
/// `enc:`-prefixed, and writes back atomically (via tmp + rename).
/// Returns the number of values encrypted.
///
/// # Errors
///
/// Returns an error if the file cannot be read/written or encryption fails.
#[expect(
    clippy::result_large_err,
    reason = "taxis Error is inherently large due to PathBuf fields"
)]
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn encrypt_config_file(toml_path: &Path, primary_key: &[u8; KEY_LEN]) -> Result<usize> {
    let content =
        std::fs::read_to_string(toml_path).context(error::ReadConfigSnafu { path: toml_path })?;
    let mut value: toml::Value = toml::from_str(&content).map_err(|e| {
        error::DecryptSnafu {
            reason: format!("invalid TOML: {e}"),
        }
        .build()
    })?;

    let count = encrypt_sensitive_values(&mut value, primary_key)?;

    if count > 0 {
        let encrypted_toml = toml::to_string(&value).map_err(|e| {
            error::EncryptSnafu {
                reason: format!("failed to serialize TOML: {e}"),
            }
            .build()
        })?;

        aletheia_koina::fs::write_restricted(toml_path, encrypted_toml.as_bytes()).context(
            error::WriteConfigSnafu {
                path: toml_path.to_path_buf(),
            },
        )?;
    }

    Ok(count)
}

/// Keys in TOML whose string values are considered sensitive and eligible for
/// encryption. Matches the patterns used by `redact.rs`.
const SENSITIVE_TOML_KEYS: &[&str] = &[
    "signingKey",
    "signing_key",
    "token",
    "secret",
    "password",
    "apiKey",
    "api_key",
];

/// Recursively decrypt all `enc:`-prefixed string values in a TOML value tree.
///
/// If `primary_key` is `None`, logs a warning for each encrypted value found
/// and leaves it unchanged (plaintext fallback).
pub(crate) fn decrypt_toml_values(value: &mut toml::Value, primary_key: Option<&[u8; KEY_LEN]>) {
    match value {
        toml::Value::String(s) if is_encrypted(s) => {
            if let Some(key) = primary_key {
                match decrypt_value(s, key) {
                    Ok(plaintext) => *s = plaintext,
                    Err(e) => {
                        warn!(error = %e, "failed to decrypt config value, leaving encrypted");
                    }
                }
            } else {
                warn!(
                    "encrypted config value found but no primary key available \
                     -- value will remain encrypted"
                );
            }
        }
        toml::Value::Table(table) => {
            for (_key, val) in table.iter_mut() {
                decrypt_toml_values(val, primary_key);
            }
        }
        toml::Value::Array(arr) => {
            for item in arr {
                decrypt_toml_values(item, primary_key);
            }
        }
        _ => {
            // NOTE: leaf TOML values (bool, integer, float, datetime) cannot contain
            // encrypted strings and require no decryption pass
        }
    }
}

/// Recursively encrypt plaintext string values for sensitive keys in a TOML
/// value tree. Only encrypts values that are not already `enc:`-prefixed.
///
/// # Errors
///
/// Returns an error if any encryption operation fails.
#[expect(
    clippy::result_large_err,
    reason = "taxis Error is inherently large due to PathBuf fields"
)]
pub(crate) fn encrypt_sensitive_values(
    value: &mut toml::Value,
    primary_key: &[u8; KEY_LEN],
) -> Result<usize> {
    let mut count = 0;
    encrypt_recursive(value, primary_key, &mut count)?;
    Ok(count)
}

#[expect(
    clippy::result_large_err,
    reason = "taxis Error is inherently large due to PathBuf fields"
)]
fn encrypt_recursive(
    value: &mut toml::Value,
    primary_key: &[u8; KEY_LEN],
    count: &mut usize,
) -> Result<()> {
    match value {
        toml::Value::Table(table) => {
            for (key, val) in table.iter_mut() {
                if is_sensitive_key(key) {
                    if let toml::Value::String(s) = val
                        && !s.is_empty()
                        && !is_encrypted(s)
                    {
                        *s = encrypt_value(s, primary_key)?;
                        *count += 1;
                    }
                } else {
                    encrypt_recursive(val, primary_key, count)?;
                }
            }
        }
        toml::Value::Array(arr) => {
            for item in arr {
                encrypt_recursive(item, primary_key, count)?;
            }
        }
        _ => {
            // NOTE: leaf TOML values (bool, integer, float, datetime) cannot hold
            // plaintext secrets and are skipped during encryption traversal
        }
    }
    Ok(())
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    SENSITIVE_TOML_KEYS
        .iter()
        .any(|s| lower == s.to_lowercase())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: TOML string-key indexing panics only if key is absent"
)]
#[expect(
    clippy::as_conversions,
    reason = "test: wrapping cast by design in fixture_key()"
)]
mod tests {
    use super::*;

    fn fixture_key() -> [u8; KEY_LEN] {
        let mut key = [0u8; KEY_LEN];
        for (i, byte) in key.iter_mut().enumerate() {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "test key generation wraps by design"
            )]
            {
                *byte = (i as u8).wrapping_mul(7).wrapping_add(42);
            }
        }
        key
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = fixture_key();
        let plaintext = "sk-ant-api-secret-key-12345";
        let encrypted = encrypt_value(plaintext, &key).unwrap();

        assert!(
            is_encrypted(&encrypted),
            "encrypted value must start with enc:"
        );
        assert!(
            encrypted.starts_with(ENCRYPTED_PREFIX),
            "encrypted value should have enc: prefix"
        );

        let decrypted = decrypt_value(&encrypted, &key).unwrap();
        assert_eq!(
            decrypted, plaintext,
            "decrypted value should match original plaintext"
        );
    }

    #[test]
    fn encrypt_decrypt_empty_string() {
        let key = fixture_key();
        let encrypted = encrypt_value("", &key).unwrap();
        let decrypted = decrypt_value(&encrypted, &key).unwrap();
        assert_eq!(
            decrypted, "",
            "empty string should roundtrip through encryption"
        );
    }

    #[test]
    fn encrypt_decrypt_unicode() {
        let key = fixture_key();
        let plaintext = "secret-with-unicode-\u{1f512}\u{1f511}";
        let encrypted = encrypt_value(plaintext, &key).unwrap();
        let decrypted = decrypt_value(&encrypted, &key).unwrap();
        assert_eq!(
            decrypted, plaintext,
            "unicode string should roundtrip through encryption"
        );
    }

    #[test]
    fn detect_encrypted_vs_plaintext() {
        assert!(
            is_encrypted("enc:abc123"),
            "enc: prefixed value should be detected as encrypted"
        );
        assert!(
            !is_encrypted("plaintext-value"),
            "plaintext value should not be detected as encrypted"
        );
        assert!(
            !is_encrypted(""),
            "empty string should not be detected as encrypted"
        );
        assert!(
            !is_encrypted("ENC:uppercase-not-matched"),
            "uppercase ENC: should not match"
        );
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let key1 = fixture_key();
        let mut key2 = fixture_key();
        key2[0] ^= 0xff;

        let encrypted = encrypt_value("secret", &key1).unwrap();
        let result = decrypt_value(&encrypted, &key2);
        assert!(result.is_err(), "decryption with wrong key must fail");
    }

    #[test]
    fn corrupted_ciphertext_fails() {
        let key = fixture_key();
        let encrypted = encrypt_value("secret", &key).unwrap();

        let mut chars: Vec<char> = encrypted.chars().collect();
        let mid = chars.len() / 2;
        chars[mid] = if chars[mid] == 'A' { 'B' } else { 'A' };
        let corrupted: String = chars.into_iter().collect();

        let result = decrypt_value(&corrupted, &key);
        assert!(result.is_err(), "corrupted ciphertext must fail");
    }

    #[test]
    fn too_short_ciphertext_fails() {
        let key = fixture_key();
        let result = decrypt_value("enc:AAAA", &key);
        assert!(result.is_err(), "too-short ciphertext must fail");
    }

    #[test]
    fn invalid_base64_fails() {
        let key = fixture_key();
        let result = decrypt_value("enc:!!!invalid!!!", &key);
        assert!(result.is_err(), "invalid base64 must fail");
    }

    #[test]
    fn decrypt_toml_with_key() {
        let key = fixture_key();
        let secret = "my-jwt-signing-key";
        let encrypted = encrypt_value(secret, &key).unwrap();

        let toml_str = format!(
            r#"
            [gateway.auth]
            mode = "token"
            signingKey = "{encrypted}"
            "#
        );
        let mut value: toml::Value = toml::from_str(&toml_str).unwrap();
        decrypt_toml_values(&mut value, Some(&key));

        let signing_key = value["gateway"]["auth"]["signingKey"].as_str().unwrap();
        assert_eq!(
            signing_key, secret,
            "decrypted signing key should match original"
        );
    }

    #[test]
    fn decrypt_toml_without_key_leaves_encrypted() {
        let key = fixture_key();
        let encrypted = encrypt_value("secret", &key).unwrap();

        let toml_str = format!(
            r#"
            [gateway.auth]
            signingKey = "{encrypted}"
            "#
        );
        let mut value: toml::Value = toml::from_str(&toml_str).unwrap();
        decrypt_toml_values(&mut value, None);

        let signing_key = value["gateway"]["auth"]["signingKey"].as_str().unwrap();
        assert!(
            is_encrypted(signing_key),
            "without primary key, value must stay encrypted"
        );
    }

    #[test]
    fn encrypt_sensitive_values_in_toml() {
        let key = fixture_key();
        let toml_str = r#"
            [gateway]
            port = 18789

            [gateway.auth]
            mode = "token"
            signingKey = "my-secret-key"

            [gateway.tls]
            enabled = false
        "#;
        let mut value: toml::Value = toml::from_str(toml_str).unwrap();
        let count = encrypt_sensitive_values(&mut value, &key).unwrap();

        assert_eq!(count, 1, "only signingKey should be encrypted");

        let signing_key = value["gateway"]["auth"]["signingKey"].as_str().unwrap();
        assert!(is_encrypted(signing_key), "signingKey must be encrypted");

        let decrypted = decrypt_value(signing_key, &key).unwrap();
        assert_eq!(
            decrypted, "my-secret-key",
            "decrypted sensitive value should match original"
        );
    }

    #[test]
    fn encrypt_skips_already_encrypted_values() {
        let key = fixture_key();
        let already = encrypt_value("existing", &key).unwrap();

        let toml_str = format!(
            r#"
            [gateway.auth]
            signingKey = "{already}"
            "#
        );
        let mut value: toml::Value = toml::from_str(&toml_str).unwrap();
        let count = encrypt_sensitive_values(&mut value, &key).unwrap();

        assert_eq!(count, 0, "already-encrypted values must be skipped");
    }

    #[test]
    fn primary_key_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("primary.key");

        generate_primary_key(&key_path).unwrap();

        assert!(key_path.exists(), "key file should exist after generation");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&key_path).unwrap().permissions();
            assert_eq!(
                perms.mode() & 0o777,
                0o600,
                "key file must have 0600 permissions"
            );
        }

        let key = load_primary_key(&key_path).unwrap();
        assert!(key.is_some(), "primary key should be loaded");
        let key = key.unwrap();
        assert_ne!(key, [0u8; KEY_LEN], "key must not be all zeros");
    }

    #[test]
    fn load_missing_key_returns_none() {
        let result = load_primary_key(Path::new("/nonexistent/path/primary.key")).unwrap();
        assert!(result.is_none(), "missing key file should return None");
    }

    #[test]
    fn generate_key_rejects_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("primary.key");

        generate_primary_key(&key_path).unwrap();
        let result = generate_primary_key(&key_path);
        assert!(result.is_err(), "must reject if key file already exists");
    }

    #[test]
    fn hex_key_parsing_rejects_wrong_length() {
        let result = parse_hex_key("abcd", Path::new("test.key"));
        assert!(result.is_err(), "short hex string should be rejected");
    }

    #[test]
    fn hex_key_parsing_rejects_invalid_chars() {
        let bad = "zz".repeat(KEY_LEN);
        let result = parse_hex_key(&bad, Path::new("test.key"));
        assert!(result.is_err(), "invalid hex characters should be rejected");
    }

    #[test]
    fn each_encryption_produces_different_ciphertext() {
        let key = fixture_key();
        let plaintext = "same-input";
        let enc1 = encrypt_value(plaintext, &key).unwrap();
        let enc2 = encrypt_value(plaintext, &key).unwrap();
        assert_ne!(enc1, enc2, "random nonce must produce different ciphertext");

        assert_eq!(
            decrypt_value(&enc1, &key).unwrap(),
            plaintext,
            "first encryption should decrypt correctly"
        );
        assert_eq!(
            decrypt_value(&enc2, &key).unwrap(),
            plaintext,
            "second encryption should decrypt correctly"
        );
    }
}
