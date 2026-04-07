//! API key generation and validation.
//!
//! Key format: `ale_{prefix}_{secret}` where secret is 32 random bytes hex-encoded.
//! The prefix identifies the key holder (for logs/audit).
//! The secret is hashed with blake3 before storage: never stored in plaintext.

use std::time::Duration;

use aletheia_koina::hex;
use rand::Rng;
use tracing::instrument;

use crate::error::{self, Result};
use crate::store::AuthStore;
use crate::types::{ApiKeyRecord, Claims, Role, TokenKind};

/// Prefix for all Aletheia API keys.
const KEY_PREFIX: &str = "ale";

/// Generate a new API key. Returns `(full_key_string, metadata_record)`.
///
/// The full key string is shown to the user exactly once. Only the hash is stored.
#[instrument(skip(store))]
pub(crate) fn generate(
    store: &AuthStore,
    prefix: &str,
    role: Role,
    nous_id: Option<&str>,
    expires_in: Option<Duration>,
) -> Result<(String, ApiKeyRecord)> {
    let secret_bytes: [u8; 32] = rand::rng().random();
    let secret_hex = hex::encode(&secret_bytes);
    let full_key = format!("{KEY_PREFIX}_{prefix}_{secret_hex}");

    let key_hash = blake3::hash(full_key.as_bytes()).to_hex().to_string();
    let id = ulid::Ulid::new().to_string();

    let expires_at = expires_in.map(|d| {
        let signed = jiff::SignedDuration::try_from(d).unwrap_or_else(|_| {
            tracing::warn!("expiry duration overflows signed duration, clamping");
            jiff::SignedDuration::MAX
        });
        jiff::Timestamp::now()
            .checked_add(signed)
            .unwrap_or(jiff::Timestamp::MAX)
            .to_string()
    });

    let record = ApiKeyRecord {
        id: id.clone(),
        prefix: prefix.to_owned(),
        key_hash,
        role,
        nous_id: nous_id.map(str::to_owned),
        created_at: String::new(), // NOTE: DB default handles this
        expires_at,
        last_used_at: None,
        revoked_at: None,
    };

    store.store_api_key(&record)?;

    let stored = store
        .find_api_key_by_hash(&record.key_hash)?
        .unwrap_or_else(|| {
            tracing::warn!("could not re-read API key after storage, using unsaved record");
            record
        });

    Ok((full_key, stored))
}

/// Validate an API key string and return claims if valid.
///
/// Parses the key format, hashes with blake3, looks up in the store,
/// checks revocation and expiry, and updates `last_used_at`.
pub(crate) fn validate(store: &AuthStore, raw_key: &str) -> Result<Claims> {
    let _parts = parse_key(raw_key)?;
    let key_hash = blake3::hash(raw_key.as_bytes()).to_hex().to_string();

    let record = store
        .find_api_key_by_hash(&key_hash)?
        .ok_or_else(|| error::InvalidCredentialsSnafu.build())?;

    if record.revoked_at.is_some() {
        return Err(error::InvalidCredentialsSnafu.build());
    }

    if let Some(ref expires_at) = record.expires_at {
        let now = now_iso();
        if *expires_at < now {
            return Err(error::ExpiredTokenSnafu.build());
        }
    }

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

/// Revoke an API key by its ID, preventing further use.
pub(crate) fn revoke(store: &AuthStore, key_id: &str) -> Result<()> {
    store.revoke_api_key(key_id)
}

/// List all API key records (metadata only, never the secret).
pub(crate) fn list(store: &AuthStore) -> Result<Vec<ApiKeyRecord>> {
    store.list_api_keys()
}

/// Parse an API key into `(global_prefix, holder_prefix, secret)`.
fn parse_key(raw: &str) -> Result<(&str, &str, &str)> {
    let mut iter = raw.splitn(3, '_');
    let prefix = iter
        .next()
        .ok_or_else(|| error::InvalidApiKeySnafu.build())?;
    let holder = iter
        .next()
        .ok_or_else(|| error::InvalidApiKeySnafu.build())?;
    let secret = iter
        .next()
        .ok_or_else(|| error::InvalidApiKeySnafu.build())?;

    if prefix != KEY_PREFIX || holder.is_empty() || secret.is_empty() {
        return Err(error::InvalidApiKeySnafu.build());
    }
    Ok((prefix, holder, secret))
}

fn now_iso() -> String {
    aletheia_koina::time::now_iso8601()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: parts[2] is valid after splitn(3) produces 3 parts"
)]
mod tests {
    use super::*;

    fn memory_store() -> AuthStore {
        AuthStore::open_in_memory().unwrap()
    }

    #[test]
    fn generate_and_validate_roundtrip() {
        let store = memory_store();
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
        let store = memory_store();
        let (key, _) = generate(&store, "syn", Role::Agent, Some("syn"), None).unwrap();
        let claims = validate(&store, &key).unwrap();
        assert_eq!(claims.role, Role::Agent);
        assert_eq!(claims.nous_id.as_deref(), Some("syn"));
    }

    #[test]
    fn revoked_key_rejected() {
        let store = memory_store();
        let (key, record) = generate(&store, "test", Role::Operator, None, None).unwrap();

        revoke(&store, &record.id).unwrap();
        let result = validate(&store, &key);
        assert!(result.is_err());
    }

    #[test]
    fn malformed_key_rejected() {
        let store = memory_store();
        assert!(validate(&store, "not-a-key").is_err());
        assert!(validate(&store, "ale_").is_err());
        assert!(validate(&store, "ale__secret").is_err());
        assert!(validate(&store, "xyz_test_secret").is_err());
    }

    #[test]
    fn nonexistent_key_rejected() {
        let store = memory_store();
        assert!(validate(&store, "ale_test_nonexistent").is_err());
    }

    #[test]
    fn list_returns_all_keys() {
        let store = memory_store();
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
        let store = memory_store();
        let (key, _) = generate(&store, "test", Role::Operator, None, None).unwrap();
        let parts: Vec<&str> = key.splitn(3, '_').collect();
        assert_eq!(parts[2].len(), 64); // NOTE: 32 bytes * 2 hex chars
    }
}
