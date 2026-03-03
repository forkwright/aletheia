//! API key generation and validation.
//!
//! Key format: `ale_{prefix}_{secret}` where secret is 32 random bytes hex-encoded.
//! The prefix identifies the key holder (for logs/audit).
//! The secret is hashed with blake3 before storage — never stored in plaintext.

use std::time::Duration;

use rand::Rng;
use tracing::instrument;

use crate::error::{self, Result};
use crate::store::AuthStore;
use crate::types::{ApiKeyRecord, Claims, Role, TokenKind};

/// Prefix for all Aletheia API keys.
const KEY_PREFIX: &str = "ale";

/// Generate a new API key. Returns `(full_key_string, metadata_record)`.
///
/// The full key string (`ale_{prefix}_{secret}`) is returned exactly once and must
/// be delivered to the caller immediately — only the blake3 hash is stored.
/// Use `prefix` as a human-readable identifier for logs and audit trails.
///
/// # Errors
///
/// Returns [`crate::error::Error::Database`] if the key cannot be persisted to the store.
#[instrument(skip(store))]
pub fn generate(
    store: &AuthStore,
    prefix: &str,
    role: Role,
    nous_id: Option<&str>,
    expires_in: Option<Duration>,
) -> Result<(String, ApiKeyRecord)> {
    let secret_bytes: [u8; 32] = rand::thread_rng().r#gen();
    let secret_hex = hex::encode(&secret_bytes);
    let full_key = format!("{KEY_PREFIX}_{prefix}_{secret_hex}");

    let key_hash = blake3::hash(full_key.as_bytes()).to_hex().to_string();
    let id = ulid::Ulid::new().to_string();

    let expires_at = expires_in.map(|d| {
        let expiry = std::time::SystemTime::now() + d;
        let secs = expiry
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        time_from_unix(secs)
    });

    let record = ApiKeyRecord {
        id: id.clone(),
        prefix: prefix.to_owned(),
        key_hash,
        role,
        nous_id: nous_id.map(str::to_owned),
        created_at: String::new(), // DB default handles this
        expires_at,
        last_used_at: None,
        revoked_at: None,
    };

    store.store_api_key(&record)?;

    // Re-read from DB to get populated timestamps
    let stored = store
        .find_api_key_by_hash(&record.key_hash)?
        .unwrap_or(record);

    Ok((full_key, stored))
}

/// Validate an API key string and return claims if valid.
///
/// Parses the `ale_{prefix}_{secret}` format, hashes the full string with blake3,
/// looks up the hash in the store, checks revocation and expiry, and updates `last_used_at`.
///
/// # Errors
///
/// - [`crate::error::Error::InvalidApiKey`] if the key format is malformed.
/// - [`crate::error::Error::InvalidCredentials`] if the hash is not found or the key is revoked.
/// - [`crate::error::Error::ExpiredToken`] if the key has passed its expiry timestamp.
/// - [`crate::error::Error::Database`] on `SQLite` access failure.
pub fn validate(store: &AuthStore, raw_key: &str) -> Result<Claims> {
    let _parts = parse_key(raw_key)?;
    let key_hash = blake3::hash(raw_key.as_bytes()).to_hex().to_string();

    let record = store
        .find_api_key_by_hash(&key_hash)?
        .ok_or_else(|| error::InvalidCredentialsSnafu.build())?;

    // Check revocation
    if record.revoked_at.is_some() {
        return Err(error::InvalidCredentialsSnafu.build());
    }

    // Check expiry
    if let Some(ref expires_at) = record.expires_at {
        let now = now_iso();
        if *expires_at < now {
            return Err(error::ExpiredTokenSnafu.build());
        }
    }

    // Update last_used_at
    store.touch_api_key(&record.id)?;

    Ok(Claims {
        sub: format!("apikey:{}", record.prefix),
        role: record.role,
        nous_id: record.nous_id,
        iss: "aletheia".to_owned(),
        iat: 0,
        exp: 0,
        jti: record.id,
        kind: TokenKind::Access,
    })
}

/// Revoke an API key by its record ID (not the key string itself).
///
/// # Errors
///
/// - [`crate::error::Error::NotFound`] if no key with that ID exists.
/// - [`crate::error::Error::Database`] on `SQLite` access failure.
pub fn revoke(store: &AuthStore, key_id: &str) -> Result<()> {
    store.revoke_api_key(key_id)
}

/// List all API keys, ordered by creation time descending (metadata only, never the secret).
///
/// # Errors
///
/// Returns [`crate::error::Error::Database`] on `SQLite` access failure.
pub fn list(store: &AuthStore) -> Result<Vec<ApiKeyRecord>> {
    store.list_api_keys()
}

/// Parse an API key into `(global_prefix, holder_prefix, secret)`.
fn parse_key(raw: &str) -> Result<(&str, &str, &str)> {
    let parts: Vec<&str> = raw.splitn(3, '_').collect();
    if parts.len() != 3 || parts[0] != KEY_PREFIX {
        return Err(error::InvalidApiKeySnafu.build());
    }
    if parts[1].is_empty() || parts[2].is_empty() {
        return Err(error::InvalidApiKeySnafu.build());
    }
    Ok((parts[0], parts[1], parts[2]))
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    time_from_unix(secs)
}

fn time_from_unix(secs: u64) -> String {
    // Simple ISO 8601 formatting without external dependency
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Convert days since epoch to Y-M-D (simplified)
    let (year, month, day) = days_to_date(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.000Z")
}

fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let y = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let d = day_of_year - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// We need hex encoding for the secret bytes
mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            s.push(HEX_CHARS[(b >> 4) as usize] as char);
            s.push(HEX_CHARS[(b & 0x0f) as usize] as char);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> AuthStore {
        AuthStore::open_in_memory().unwrap()
    }

    #[test]
    fn generate_and_validate_roundtrip() {
        let store = test_store();
        let (key, record) = generate(&store, "test", Role::Operator, None, None).unwrap();

        assert!(key.starts_with("ale_test_"));
        assert_eq!(record.prefix, "test");
        assert_eq!(record.role, Role::Operator);

        let claims = validate(&store, &key).unwrap();
        assert_eq!(claims.sub, "apikey:test");
        assert_eq!(claims.role, Role::Operator);
    }

    #[test]
    fn generate_agent_key_with_nous_id() {
        let store = test_store();
        let (key, _) = generate(&store, "syn", Role::Agent, Some("syn"), None).unwrap();
        let claims = validate(&store, &key).unwrap();
        assert_eq!(claims.role, Role::Agent);
        assert_eq!(claims.nous_id.as_deref(), Some("syn"));
    }

    #[test]
    fn revoked_key_rejected() {
        let store = test_store();
        let (key, record) = generate(&store, "test", Role::Operator, None, None).unwrap();

        revoke(&store, &record.id).unwrap();
        let result = validate(&store, &key);
        assert!(result.is_err());
    }

    #[test]
    fn malformed_key_rejected() {
        let store = test_store();
        assert!(validate(&store, "not-a-key").is_err());
        assert!(validate(&store, "ale_").is_err());
        assert!(validate(&store, "ale__secret").is_err());
        assert!(validate(&store, "xyz_test_secret").is_err());
    }

    #[test]
    fn nonexistent_key_rejected() {
        let store = test_store();
        assert!(validate(&store, "ale_test_nonexistent").is_err());
    }

    #[test]
    fn list_returns_all_keys() {
        let store = test_store();
        generate(&store, "a", Role::Operator, None, None).unwrap();
        generate(&store, "b", Role::Agent, Some("syn"), None).unwrap();

        let keys = list(&store).unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn parse_key_format() {
        let (prefix, holder, secret) = parse_key("ale_syn_abc123").unwrap();
        assert_eq!(prefix, "ale");
        assert_eq!(holder, "syn");
        assert_eq!(secret, "abc123");
    }

    #[test]
    fn key_secret_is_64_hex_chars() {
        let store = test_store();
        let (key, _) = generate(&store, "test", Role::Operator, None, None).unwrap();
        let parts: Vec<&str> = key.splitn(3, '_').collect();
        assert_eq!(parts[2].len(), 64); // 32 bytes * 2 hex chars
    }
}
