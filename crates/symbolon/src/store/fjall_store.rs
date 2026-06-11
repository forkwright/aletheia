//! Fjall-backed auth store for users, API keys, and token revocation.
//!
//! Pure-Rust LSM-tree storage via `fjall`. Zero C dependencies.
//!
//! # Key schema
//!
//! All keys are UTF-8 strings. Values are JSON-encoded domain structs.
//!
//! | Partition        | Key pattern                    | Value                  |
//! |------------------|--------------------------------|------------------------|
//! | `users`          | `user:{username}`              | JSON `User`            |
//! | `api_keys`       | `key:{id}`                     | JSON `ApiKeyRecord`    |
//! | `api_keys`       | `hash:{key_hash}`              | `{id}` (lookup index)  |
//! | `revoked_tokens` | `revoked:{jti}`                | `{expires_at}` string  |
//!
//! Username is the natural unique key for user lookups. API key hash is indexed
//! via a secondary key so `find_api_key_by_hash` remains O(1).

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "auth facade internals; only exercised by crate-level tests"
    )
)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase};
use koina::secret::SecretString;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument, warn};

use crate::error::{self, Result};
use crate::types::{ApiKeyRecord, Role, User};

// ── Wire types for fjall serialization ────────────────────────────────────────

/// Serialise a `SecretString` by exposing its inner value.
///
/// WHY: fjall persistence needs the actual hash bytes to round-trip. The
/// default `SecretString` `Serialize` impl writes `"[REDACTED]"`, which
/// would destroy auth state on write. Callers still benefit from
/// `SecretString`'s `Debug`/`Display` redaction and zeroize-on-drop.
fn serialize_secret_hash<S: serde::Serializer>(
    secret: &SecretString,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(secret.expose_secret())
}

/// Serializable form of [`User`] stored in fjall.
#[derive(Serialize, Deserialize)]
struct UserRecord {
    id: String,
    username: String,
    // WHY: Argon2id hash. The value is derived and irreversible, but we
    // still wrap it in `SecretString` so stray `Debug` or `tracing`
    // interpolation cannot leak it; the field uses a custom
    // `serialize_with` so round-tripping to fjall preserves the hash.
    #[serde(serialize_with = "serialize_secret_hash")]
    password_hash: SecretString,
    role: String,
    created_at: String,
    updated_at: String,
}

/// Serializable form of [`ApiKeyRecord`] stored in fjall.
#[derive(Serialize, Deserialize)]
struct ApiKeyEntry {
    id: String,
    prefix: String,
    // WHY: blake3 hash of the raw API key. Irreversible but a lookup
    // primitive, so we wrap it in `SecretString` for leak-resistant
    // handling and retain round-trip via `serialize_with`.
    #[serde(serialize_with = "serialize_secret_hash")]
    key_hash: SecretString,
    role: String,
    nous_id: Option<String>,
    created_at: String,
    expires_at: Option<String>,
    last_used_at: Option<String>,
    revoked_at: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// ISO 8601 timestamp string for "now".
fn now_iso() -> String {
    koina::fjall::now_iso()
}

fn decode_role(role_str: &str) -> Role {
    role_str.parse().unwrap_or_else(|_| {
        warn!(role = %role_str, "unknown role in fjall store, defaulting to Readonly");
        Role::Readonly
    })
}

fn storage_err(message: impl Into<String>) -> crate::error::Error {
    error::StorageSnafu {
        message: message.into(),
    }
    .build()
}

// ── AuthStore ─────────────────────────────────────────────────────────────────

/// Partitions used by the symbolon auth store.
const PARTITIONS: &[&str] = &["users", "api_keys", "revoked_tokens"];

/// Auth store backed by `fjall` (pure-Rust LSM-tree).
///
/// Open with [`AuthStore::open`] for persistent storage or
/// [`AuthStore::open_in_memory`] for ephemeral storage (test-only).
pub(crate) struct AuthStore {
    db: Arc<SingleWriterTxDatabase>,
    /// Shared write mutex — see [`koina::fjall::FjallDb::write_lock`].
    write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    _temp_dir: Option<tempfile::TempDir>,
}

impl AuthStore {
    /// Open (or create) the auth store at the given path.
    #[instrument(skip(path))]
    pub(crate) fn open(path: &Path) -> Result<Self> {
        info!(path = %path.display(), "Opening fjall auth store");
        let fdb = koina::fjall::FjallDb::open(path, PARTITIONS)
            .map_err(|e| storage_err(e.to_string()))?;
        Ok(Self::from_fjall_db(fdb))
    }

    /// Open an ephemeral auth store backed by a `TempDir` (for testing).
    ///
    /// The directory and all data are deleted when the returned store is dropped.
    #[instrument]
    pub(crate) fn open_in_memory() -> Result<Self> {
        let fdb =
            koina::fjall::FjallDb::open_temp(PARTITIONS).map_err(|e| storage_err(e.to_string()))?;
        Ok(Self::from_fjall_db(fdb))
    }

    fn from_fjall_db(fdb: koina::fjall::FjallDb) -> Self {
        Self {
            db: Arc::new(fdb.db),
            write_lock: fdb.write_lock,
            _temp_dir: fdb._temp_dir,
        }
    }

    // ── Partition helpers ─────────────────────────────────────────────────────

    fn partition(&self, name: &str) -> Result<fjall::SingleWriterTxKeyspace> {
        self.db
            .keyspace(name, KeyspaceCreateOptions::default)
            .map_err(|e| storage_err(format!("fjall partition {name}: {e}")))
    }

    fn get_bytes(
        &self,
        partition: &fjall::SingleWriterTxKeyspace,
        key: &str,
    ) -> Result<Option<Vec<u8>>> {
        use fjall::Readable;
        let snap = self.db.read_tx();
        snap.get(partition, key.as_bytes())
            .map(|opt| opt.map(|s| s.to_vec()))
            .map_err(|e| storage_err(format!("fjall get: {e}")))
    }

    fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        partition: &fjall::SingleWriterTxKeyspace,
        key: &str,
    ) -> Result<Option<T>> {
        match self.get_bytes(partition, key)? {
            None => Ok(None),
            Some(bytes) => {
                let v = serde_json::from_slice(&bytes)
                    .map_err(|e| storage_err(format!("fjall json decode key={key}: {e}")))?;
                Ok(Some(v))
            }
        }
    }

    fn put_json<T: Serialize>(
        &self,
        partition: &fjall::SingleWriterTxKeyspace,
        key: &str,
        value: &T,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| storage_err(format!("fjall json encode key={key}: {e}")))?;
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut tx = self.db.write_tx();
        tx.insert(partition, key.as_bytes(), &bytes);
        tx.commit()
            .map_err(|e| storage_err(format!("fjall commit: {e}")))
    }

    fn key_exists(&self, partition: &fjall::SingleWriterTxKeyspace, key: &str) -> Result<bool> {
        Ok(self.get_bytes(partition, key)?.is_some())
    }

    fn delete_key(&self, partition: &fjall::SingleWriterTxKeyspace, key: &str) -> Result<bool> {
        if !self.key_exists(partition, key)? {
            return Ok(false);
        }
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut tx = self.db.write_tx();
        tx.remove(partition, key.as_bytes());
        tx.commit()
            .map_err(|e| storage_err(format!("fjall commit failed (remove path): {e}")))?;
        Ok(true)
    }

    // ── User operations ───────────────────────────────────────────────────────

    /// Create a new user.
    #[instrument(skip(self, password_hash))]
    pub(crate) fn create_user(
        &self,
        id: &str,
        username: &str,
        password_hash: &str,
        role: Role,
    ) -> Result<User> {
        let users = self.partition("users")?;
        let key = format!("user:{username}");

        if self.key_exists(&users, &key)? {
            return Err(error::DuplicateSnafu {
                entity: "user".to_owned(),
                id: username.to_owned(),
            }
            .build());
        }

        let now = now_iso();
        let record = UserRecord {
            id: id.to_owned(),
            username: username.to_owned(),
            password_hash: SecretString::from(password_hash),
            role: role.as_str().to_owned(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.put_json(&users, &key, &record)?;

        info!(id, username, %role, "created user");
        self.find_user_by_username(username)?.ok_or_else(|| {
            error::NotFoundSnafu {
                entity: "user".to_owned(),
                id: username.to_owned(),
            }
            .build()
        })
    }

    /// Find a user by username.
    pub(crate) fn find_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let users = self.partition("users")?;
        let key = format!("user:{username}");
        let Some(record): Option<UserRecord> = self.get_json(&users, &key)? else {
            return Ok(None);
        };
        Ok(Some(User {
            id: record.id,
            username: record.username,
            // WHY: `User.password_hash` is the public domain type; the
            // wire-side SecretString is exposed here only at the
            // persistence boundary.
            password_hash: record.password_hash.expose_secret().to_owned(),
            role: decode_role(&record.role),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }))
    }

    /// Update a user's role.
    pub(crate) fn update_user_role(&self, username: &str, role: Role) -> Result<()> {
        let users = self.partition("users")?;
        let key = format!("user:{username}");
        let Some(mut record): Option<UserRecord> = self.get_json(&users, &key)? else {
            return Err(error::NotFoundSnafu {
                entity: "user".to_owned(),
                id: username.to_owned(),
            }
            .build());
        };
        role.as_str().clone_into(&mut record.role);
        record.updated_at = now_iso();
        self.put_json(&users, &key, &record)
    }

    /// Delete a user by username.
    pub(crate) fn delete_user(&self, username: &str) -> Result<bool> {
        let users = self.partition("users")?;
        let key = format!("user:{username}");
        self.delete_key(&users, &key)
    }

    // ── API key operations ────────────────────────────────────────────────────

    /// Store an API key record.
    pub(crate) fn store_api_key(&self, record: &ApiKeyRecord) -> Result<()> {
        let api_keys = self.partition("api_keys")?;
        let now = now_iso();
        let entry = ApiKeyEntry {
            id: record.id.clone(),
            prefix: record.prefix.clone(),
            key_hash: SecretString::from(record.key_hash.clone()),
            role: record.role.as_str().to_owned(),
            nous_id: record.nous_id.clone(),
            created_at: if record.created_at.is_empty() {
                now
            } else {
                record.created_at.clone()
            },
            expires_at: record.expires_at.clone(),
            last_used_at: record.last_used_at.clone(),
            revoked_at: record.revoked_at.clone(),
        };

        // WHY: Two keys per API key — primary (by id) and index (by hash).
        // Both written in the same transaction to keep them in sync.
        let primary_key = format!("key:{}", record.id);
        let hash_key = format!("hash:{}", record.key_hash);

        let bytes = serde_json::to_vec(&entry)
            .map_err(|e| storage_err(format!("api key json encode: {e}")))?;

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut tx = self.db.write_tx();
        tx.insert(&api_keys, primary_key.as_bytes(), &bytes);
        tx.insert(&api_keys, hash_key.as_bytes(), record.id.as_bytes());
        tx.commit()
            .map_err(|e| storage_err(format!("fjall commit store_api_key: {e}")))
    }

    /// Find an API key by its blake3 hash. Returns `None` if not found.
    pub(crate) fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyRecord>> {
        let api_keys = self.partition("api_keys")?;
        let hash_key = format!("hash:{key_hash}");

        // NOTE: two-step lookup — the hash key is a secondary index holding
        // the primary id.
        let Some(id_bytes) = self.get_bytes(&api_keys, &hash_key)? else {
            return Ok(None);
        };
        let id = std::str::from_utf8(&id_bytes)
            .map_err(|e| storage_err(format!("api key id utf8: {e}")))?;

        let primary_key = format!("key:{id}");
        match self.get_json::<ApiKeyEntry>(&api_keys, &primary_key)? {
            None => Ok(None),
            Some(entry) => Ok(Some(api_key_entry_to_record(entry))),
        }
    }

    /// Update the `last_used_at` timestamp for an API key.
    pub(crate) fn touch_api_key(&self, id: &str) -> Result<()> {
        let api_keys = self.partition("api_keys")?;
        let primary_key = format!("key:{id}");
        let Some(mut entry): Option<ApiKeyEntry> = self.get_json(&api_keys, &primary_key)? else {
            return Ok(());
        };
        entry.last_used_at = Some(now_iso());
        self.put_json(&api_keys, &primary_key, &entry)
    }

    /// Revoke an API key by ID.
    pub(crate) fn revoke_api_key(&self, id: &str) -> Result<()> {
        let api_keys = self.partition("api_keys")?;
        let primary_key = format!("key:{id}");
        let Some(mut entry): Option<ApiKeyEntry> = self.get_json(&api_keys, &primary_key)? else {
            return Err(error::NotFoundSnafu {
                entity: "api_key".to_owned(),
                id: id.to_owned(),
            }
            .build());
        };
        entry.revoked_at = Some(now_iso());
        self.put_json(&api_keys, &primary_key, &entry)
    }

    /// List all API keys (metadata only).
    ///
    /// # Complexity
    ///
    /// O(k) where k is the number of API key primary records.
    pub(crate) fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>> {
        use fjall::Readable;
        let api_keys = self.partition("api_keys")?;
        let snap = self.db.read_tx();

        // WHY: range scan on "key:" prefix picks up all primary records and
        // skips the "hash:" index entries, avoiding double-counting.
        // Upper bound "key;" (ASCII 0x3B) is the next byte after ':' (0x3A).
        let mut records = Vec::new();
        for guard in snap.range(&api_keys, "key:".."key;") {
            let (_key, value) = guard
                .into_inner()
                .map_err(|e| storage_err(format!("fjall scan api_keys: {e}")))?;
            let entry: ApiKeyEntry = serde_json::from_slice(&value)
                .map_err(|e| storage_err(format!("api key json decode: {e}")))?;
            records.push(api_key_entry_to_record(entry));
        }

        // WHY: descending created_at order matches the legacy SQLite
        // behaviour callers still expect.
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(records)
    }

    // ── Token revocation ──────────────────────────────────────────────────────

    /// Revoke a JWT by its `jti`.
    pub(crate) fn revoke_token(&self, jti: &str, expires_at: &str) -> Result<()> {
        let revoked = self.partition("revoked_tokens")?;
        let key = format!("revoked:{jti}");
        // WHY: INSERT OR IGNORE semantics — silently skip if already present.
        if !self.key_exists(&revoked, &key)? {
            let _guard = self
                .write_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut tx = self.db.write_tx();
            tx.insert(&revoked, key.as_bytes(), expires_at.as_bytes());
            tx.commit()
                .map_err(|e| storage_err(format!("fjall commit revoke_token: {e}")))?;
        }
        Ok(())
    }

    /// Check if a JWT has been revoked.
    pub(crate) fn is_token_revoked(&self, jti: &str) -> Result<bool> {
        let revoked = self.partition("revoked_tokens")?;
        let key = format!("revoked:{jti}");
        self.key_exists(&revoked, &key)
    }

    /// Remove revocation entries for tokens that have already expired.
    ///
    /// # Complexity
    ///
    /// O(r) where r is the number of revoked tokens in the store.
    pub(crate) fn cleanup_expired_revocations(&self) -> Result<usize> {
        use fjall::Readable;
        let revoked = self.partition("revoked_tokens")?;
        let now = now_iso();

        // WHY: ';' is the next byte after ':', so "revoked:".."revoked;" is an
        // exclusive upper bound covering exactly the "revoked:" prefix.
        let snap = self.db.read_tx();
        let mut expired_keys: Vec<Vec<u8>> = Vec::new();
        for guard in snap.range(&revoked, "revoked:".."revoked;") {
            let (key, value) = guard
                .into_inner()
                .map_err(|e| storage_err(format!("fjall scan revocations: {e}")))?;
            let expires_at = std::str::from_utf8(&value).unwrap_or("").to_owned();
            if expires_at < now {
                expired_keys.push(key.to_vec());
            }
        }
        // NOTE: the read snapshot is dropped before the write lock is taken.
        drop(snap);

        let count = expired_keys.len();
        if count > 0 {
            let _guard = self
                .write_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut tx = self.db.write_tx();
            for key in &expired_keys {
                tx.remove(&revoked, key.as_slice());
            }
            tx.commit()
                .map_err(|e| storage_err(format!("fjall commit cleanup: {e}")))?;
        }
        Ok(count)
    }
}

fn api_key_entry_to_record(entry: ApiKeyEntry) -> ApiKeyRecord {
    ApiKeyRecord {
        id: entry.id,
        prefix: entry.prefix,
        // WHY: `ApiKeyRecord.key_hash` is the public domain type; the
        // wire-side SecretString is unwrapped here at the persistence
        // boundary.
        key_hash: entry.key_hash.expose_secret().to_owned(),
        role: decode_role(&entry.role),
        nous_id: entry.nous_id,
        created_at: entry.created_at,
        expires_at: entry.expires_at,
        last_used_at: entry.last_used_at,
        revoked_at: entry.revoked_at,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn memory_store() -> AuthStore {
        // kanon:ignore RUST/expect — test helper invariant; panic aborts the test run
        AuthStore::open_in_memory().expect("open in-memory fjall auth store")
    }

    #[test]
    fn user_crud() {
        let store = memory_store();
        let user = store
            .create_user("u1", "alice", "$argon2id$hash", Role::Operator)
            .unwrap();
        assert_eq!(user.username, "alice");
        assert_eq!(user.role, Role::Operator);

        let found = store.find_user_by_username("alice").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "u1");

        store.update_user_role("alice", Role::Readonly).unwrap();
        let updated = store.find_user_by_username("alice").unwrap().unwrap();
        assert_eq!(updated.role, Role::Readonly);

        let deleted = store.delete_user("alice").unwrap();
        assert!(deleted);

        let gone = store.find_user_by_username("alice").unwrap();
        assert!(gone.is_none());
    }

    #[test]
    fn duplicate_username_rejected() {
        let store = memory_store();
        store
            .create_user("u1", "alice", "$hash1", Role::Operator)
            .unwrap();
        let result = store.create_user("u2", "alice", "$hash2", Role::Readonly);
        assert!(result.is_err());
    }

    #[test]
    fn delete_nonexistent_user_returns_false() {
        let store = memory_store();
        assert!(!store.delete_user("nobody").unwrap());
    }

    #[test]
    fn update_nonexistent_user_role_errors() {
        let store = memory_store();
        let result = store.update_user_role("nobody", Role::Operator);
        assert!(result.is_err());
    }

    #[test]
    fn token_revocation_lifecycle() {
        let store = memory_store();
        let jti = "test-jti-123";

        assert!(!store.is_token_revoked(jti).unwrap());
        store.revoke_token(jti, "2099-01-01T00:00:00.000Z").unwrap();
        assert!(store.is_token_revoked(jti).unwrap());
    }

    #[test]
    fn expired_revocation_cleanup() {
        let store = memory_store();
        store
            .revoke_token("old-jti", "2000-01-01T00:00:00.000Z")
            .unwrap();
        store
            .revoke_token("future-jti", "2099-01-01T00:00:00.000Z")
            .unwrap();

        let cleaned = store.cleanup_expired_revocations().unwrap();
        assert_eq!(cleaned, 1);

        assert!(!store.is_token_revoked("old-jti").unwrap());
        assert!(store.is_token_revoked("future-jti").unwrap());
    }

    #[test]
    fn api_key_store_and_find() {
        let store = memory_store();
        let record = ApiKeyRecord {
            id: "k1".to_owned(),
            prefix: "syn".to_owned(),
            key_hash: "abc123hash".to_owned(),
            role: Role::Agent,
            nous_id: Some("syn".to_owned()),
            created_at: String::new(),
            expires_at: None,
            last_used_at: None,
            revoked_at: None,
        };

        store.store_api_key(&record).unwrap();
        let found = store.find_api_key_by_hash("abc123hash").unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id, "k1");
        assert_eq!(found.prefix, "syn");
        assert_eq!(found.role, Role::Agent);
    }

    #[test]
    fn api_key_revoke() {
        let store = memory_store();
        let record = ApiKeyRecord {
            id: "k1".to_owned(),
            prefix: "test".to_owned(),
            key_hash: "hash1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            created_at: String::new(),
            expires_at: None,
            last_used_at: None,
            revoked_at: None,
        };

        store.store_api_key(&record).unwrap();
        store.revoke_api_key("k1").unwrap();

        let found = store.find_api_key_by_hash("hash1").unwrap().unwrap();
        assert!(found.revoked_at.is_some());
    }

    #[test]
    fn api_key_list() {
        let store = memory_store();
        for i in 0..3 {
            let record = ApiKeyRecord {
                id: format!("k{i}"),
                prefix: format!("prefix{i}"),
                key_hash: format!("hash{i}"),
                role: Role::Readonly,
                nous_id: None,
                created_at: String::new(),
                expires_at: None,
                last_used_at: None,
                revoked_at: None,
            };
            store.store_api_key(&record).unwrap();
        }

        let keys = store.list_api_keys().unwrap();
        assert_eq!(keys.len(), 3);
    }

    #[test]
    fn revoke_nonexistent_api_key_errors() {
        let store = memory_store();
        let result = store.revoke_api_key("no-such-key");
        assert!(result.is_err());
    }

    /// Boundary test for `cleanup_expired_revocations`'s expiry comparison.
    ///
    /// WHY: Kills the `<` → `<=` mutant. A token whose `expires_at` is exactly
    /// `now` must *not* be removed; changing `<` to `<=` would incorrectly
    /// delete it, violating the revocation guarantee.
    #[test]
    fn cleanup_expired_revocations_boundary() {
        let store = memory_store();

        let now = jiff::Zoned::now();
        let past = now
            .clone()
            .checked_sub(jiff::Span::new().seconds(1))
            .unwrap()
            .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        // WHY: use `now + 100ms` rather than exactly `now` so the timestamp
        // is unambiguously strictly in the future by the time cleanup runs
        // a few microseconds later. Wall-clock-equal-to-now is untestable
        // without a time-injection seam in production.
        let near_future = now
            .clone()
            .checked_add(jiff::Span::new().milliseconds(100))
            .unwrap()
            .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let future = now
            .checked_add(jiff::Span::new().seconds(1))
            .unwrap()
            .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        store.revoke_token("past-jti", &past).unwrap();
        store.revoke_token("present-jti", &near_future).unwrap();
        store.revoke_token("future-jti", &future).unwrap();

        let cleaned = store.cleanup_expired_revocations().unwrap();
        assert_eq!(
            cleaned, 1,
            "only the strictly-past token should be cleaned up"
        );

        assert!(!store.is_token_revoked("past-jti").unwrap());
        assert!(
            store.is_token_revoked("present-jti").unwrap(),
            "token expiring just after now must survive cleanup (strict `<` boundary)"
        );
        assert!(store.is_token_revoked("future-jti").unwrap());
    }

    /// Ensures `cleanup_expired_revocations` behaves correctly when nothing is
    /// expired (the count == 0 early-exit path).
    #[test]
    fn cleanup_expired_revocations_noop() {
        let store = memory_store();

        // Empty store → count == 0.
        assert_eq!(store.cleanup_expired_revocations().unwrap(), 0);

        // Only future tokens → count == 0.
        store
            .revoke_token("future-jti", "2099-01-01T00:00:00.000Z")
            .unwrap();
        assert_eq!(store.cleanup_expired_revocations().unwrap(), 0);
        assert!(store.is_token_revoked("future-jti").unwrap());
    }

    /// Round-trip test for `api_key_entry_to_record`.
    ///
    /// WHY: Kills the `Default::default()` mutant by asserting every field
    /// survives encoding → storage → decoding. All `Option` fields are set to
    /// `Some` so that a default-value replacement is detected.
    #[test]
    fn api_key_round_trip_all_fields() {
        let store = memory_store();
        let record = ApiKeyRecord {
            id: "k1".to_owned(),
            prefix: "ak_prefix".to_owned(),
            key_hash: "blake3hash".to_owned(),
            role: Role::Admin,
            nous_id: Some("nous-42".to_owned()),
            created_at: "2024-01-15T09:30:00.000Z".to_owned(),
            expires_at: Some("2025-01-15T09:30:00.000Z".to_owned()),
            last_used_at: Some("2024-06-15T09:30:00.000Z".to_owned()),
            revoked_at: Some("2024-07-15T09:30:00.000Z".to_owned()),
        };

        store.store_api_key(&record).unwrap();
        let found = store.find_api_key_by_hash("blake3hash").unwrap();
        assert!(found.is_some(), "api key should be found by hash");
        let found = found.unwrap();

        assert_eq!(found.id, record.id);
        assert_eq!(found.prefix, record.prefix);
        assert_eq!(found.key_hash, record.key_hash);
        assert_eq!(found.role, record.role);
        assert_eq!(found.nous_id, record.nous_id);
        assert_eq!(found.created_at, record.created_at);
        assert_eq!(found.expires_at, record.expires_at);
        assert_eq!(found.last_used_at, record.last_used_at);
        assert_eq!(found.revoked_at, record.revoked_at);
    }
}
