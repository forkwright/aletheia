//! `SQLite` auth store for users, API keys, and token revocation.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "auth facade internals; only exercised by crate-level tests"
    )
)]

use std::path::Path;

use rusqlite::Connection;
use snafu::{IntoError, ResultExt};
use tracing::{info, instrument, warn};

use crate::error::{self, Result};
use crate::types::{ApiKeyRecord, Role, User};

/// Current schema version.
pub(crate) const SCHEMA_VERSION: u32 = 1;

/// Base DDL for the auth database.
pub(crate) const DDL: &str = r"
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'readonly',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    nous_id TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT,
    last_used_at TEXT,
    revoked_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_prefix ON api_keys(prefix);

CREATE TABLE IF NOT EXISTS revoked_tokens (
    jti TEXT PRIMARY KEY,
    revoked_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
";

/// Auth store backed by `SQLite`.
pub(crate) struct AuthStore {
    conn: Connection,
}

impl AuthStore {
    /// Open (or create) the auth store at the given path.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        info!("Opening auth store at {}", path.display());
        let conn = Connection::open(path).context(error::DatabaseSnafu)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .context(error::DatabaseSnafu)?;

        initialize(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory auth store (for testing).
    pub(crate) fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context(error::DatabaseSnafu)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .context(error::DatabaseSnafu)?;
        initialize(&conn)?;
        Ok(Self { conn })
    }

    /// Get a reference to the underlying connection.
    #[must_use]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Create a new user.
    #[instrument(skip(self, password_hash))]
    pub(crate) fn create_user(
        &self,
        id: &str,
        username: &str,
        password_hash: &str,
        role: Role,
    ) -> Result<User> {
        self.conn
            .execute(
                "INSERT INTO users (id, username, password_hash, role) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, username, password_hash, role.as_str()],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(ref err, _)
                    if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
                {
                    error::DuplicateSnafu {
                        entity: "user".to_owned(),
                        id: username.to_owned(),
                    }
                    .build()
                }
                other => error::DatabaseSnafu.into_error(other),
            })?;

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
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, username, password_hash, role, created_at, updated_at FROM users WHERE username = ?1",
            )
            .context(error::DatabaseSnafu)?;

        stmt.query_row([username], map_user)
            .optional()
            .context(error::DatabaseSnafu)
    }

    /// Update a user's role.
    pub(crate) fn update_user_role(&self, username: &str, role: Role) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE users SET role = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE username = ?2",
                rusqlite::params![role.as_str(), username],
            )
            .context(error::DatabaseSnafu)?;

        if rows == 0 {
            return Err(error::NotFoundSnafu {
                entity: "user".to_owned(),
                id: username.to_owned(),
            }
            .build());
        }
        Ok(())
    }

    /// Delete a user by username.
    pub(crate) fn delete_user(&self, username: &str) -> Result<bool> {
        let rows = self
            .conn
            .execute("DELETE FROM users WHERE username = ?1", [username])
            .context(error::DatabaseSnafu)?;
        Ok(rows > 0)
    }

    /// Store an API key record.
    pub(crate) fn store_api_key(&self, record: &ApiKeyRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO api_keys (id, prefix, key_hash, role, nous_id, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    record.id,
                    record.prefix,
                    record.key_hash,
                    record.role.as_str(),
                    record.nous_id,
                    record.expires_at,
                ],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Find an API key by its blake3 hash. Returns `None` if not found.
    pub(crate) fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyRecord>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, prefix, key_hash, role, nous_id, created_at, expires_at, last_used_at, revoked_at
                 FROM api_keys WHERE key_hash = ?1",
            )
            .context(error::DatabaseSnafu)?;

        stmt.query_row([key_hash], map_api_key)
            .optional()
            .context(error::DatabaseSnafu)
    }

    /// Update the `last_used_at` timestamp for an API key.
    pub(crate) fn touch_api_key(&self, id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE api_keys SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                [id],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Revoke an API key by ID.
    pub(crate) fn revoke_api_key(&self, id: &str) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE api_keys SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                [id],
            )
            .context(error::DatabaseSnafu)?;

        if rows == 0 {
            return Err(error::NotFoundSnafu {
                entity: "api_key".to_owned(),
                id: id.to_owned(),
            }
            .build());
        }
        Ok(())
    }

    /// List all API keys (metadata only).
    pub(crate) fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, prefix, key_hash, role, nous_id, created_at, expires_at, last_used_at, revoked_at
                 FROM api_keys ORDER BY created_at DESC",
            )
            .context(error::DatabaseSnafu)?;

        let rows = stmt
            .query_map([], map_api_key)
            .context(error::DatabaseSnafu)?;

        let mut keys = Vec::new();
        for row in rows {
            keys.push(row.context(error::DatabaseSnafu)?);
        }
        Ok(keys)
    }

    /// Revoke a JWT by its `jti`.
    pub(crate) fn revoke_token(&self, jti: &str, expires_at: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO revoked_tokens (jti, expires_at) VALUES (?1, ?2)",
                [jti, expires_at],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Check if a JWT has been revoked.
    pub(crate) fn is_token_revoked(&self, jti: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT 1 FROM revoked_tokens WHERE jti = ?1")
            .context(error::DatabaseSnafu)?;

        let exists = stmt
            .query_row([jti], |_| Ok(()))
            .optional()
            .context(error::DatabaseSnafu)?;

        Ok(exists.is_some())
    }

    /// Remove revocation entries for tokens that have already expired.
    pub(crate) fn cleanup_expired_revocations(&self) -> Result<usize> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM revoked_tokens WHERE expires_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                [],
            )
            .context(error::DatabaseSnafu)?;
        Ok(rows)
    }
}

fn initialize(conn: &Connection) -> Result<()> {
    let version = get_schema_version(conn)?;

    if version == 0 {
        info!("Initializing fresh auth database with schema v{SCHEMA_VERSION}");
        conn.execute_batch(DDL).context(error::DatabaseSnafu)?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION],
        )
        .context(error::DatabaseSnafu)?;
    }

    // WHY: revoked tokens only need to be tracked until their natural expiry;
    // startup cleanup is sufficient for production workloads.
    let cleaned = conn
        .execute(
            "DELETE FROM revoked_tokens WHERE expires_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            [],
        )
        .context(error::DatabaseSnafu)?;
    if cleaned > 0 {
        info!(
            count = cleaned,
            "cleaned up expired token revocations on startup"
        );
    }

    Ok(())
}

fn get_schema_version(conn: &Connection) -> Result<u32> {
    // NOTE: if the schema_version table does not yet exist, this is a fresh database;
    // return 0 to signal that all DDL should be applied.
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |row| row.get(0),
        )
        .context(error::DatabaseSnafu)?;

    if !table_exists {
        return Ok(0);
    }

    // NOTE: the table exists — any failure to read the version signals corruption.
    conn.query_row(
        "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .context(error::SchemaCorruptedSnafu)
}

fn map_user(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    let role_str: String = row.get("role")?;
    let role = role_str.parse().unwrap_or_else(|_| {
        warn!(role = %role_str, "unknown role in database, defaulting to Readonly");
        Role::Readonly
    });
    Ok(User {
        id: row.get("id")?,
        username: row.get("username")?,
        password_hash: row.get("password_hash")?,
        role,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn map_api_key(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApiKeyRecord> {
    let role_str: String = row.get("role")?;
    let role = role_str.parse().unwrap_or_else(|_| {
        warn!(role = %role_str, "unknown role in database, defaulting to Readonly");
        Role::Readonly
    });
    Ok(ApiKeyRecord {
        id: row.get("id")?,
        prefix: row.get("prefix")?,
        key_hash: row.get("key_hash")?,
        role,
        nous_id: row.get("nous_id")?,
        created_at: row.get("created_at")?,
        expires_at: row.get("expires_at")?,
        last_used_at: row.get("last_used_at")?,
        revoked_at: row.get("revoked_at")?,
    })
}

/// Extension trait for optional query results.
trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn test_store() -> AuthStore {
        AuthStore::open_in_memory().expect("open in-memory auth store")
    }

    #[test]
    fn fresh_database_initializes() {
        let store = test_store();
        let version = get_schema_version(store.conn()).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn idempotent_initialization() {
        let store = test_store();
        initialize(store.conn()).unwrap();
        let version = get_schema_version(store.conn()).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn schema_corruption_returns_error() {
        let store = test_store();
        // NOTE: simulate corruption: drop and recreate schema_version empty to verify error path
        store
            .conn()
            .execute_batch("DELETE FROM schema_version;")
            .unwrap();
        let result = get_schema_version(store.conn());
        assert!(
            matches!(result, Err(error::Error::SchemaCorrupted { .. })),
            "expected SchemaCorrupted, got: {result:?}"
        );
    }

    #[test]
    fn tables_exist_after_init() {
        let store = test_store();
        for table in &["users", "api_keys", "revoked_tokens"] {
            let exists: bool = store
                .conn()
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "table {table} should exist");
        }
    }

    #[test]
    fn user_crud() {
        let store = test_store();
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
        let store = test_store();
        store
            .create_user("u1", "alice", "$hash1", Role::Operator)
            .unwrap();
        let result = store.create_user("u2", "alice", "$hash2", Role::Readonly);
        assert!(result.is_err());
    }

    #[test]
    fn delete_nonexistent_user_returns_false() {
        let store = test_store();
        assert!(!store.delete_user("nobody").unwrap());
    }

    #[test]
    fn token_revocation_lifecycle() {
        let store = test_store();
        let jti = "test-jti-123";

        assert!(!store.is_token_revoked(jti).unwrap());
        store.revoke_token(jti, "2099-01-01T00:00:00.000Z").unwrap();
        assert!(store.is_token_revoked(jti).unwrap());
    }

    #[test]
    fn expired_revocation_cleanup() {
        let store = test_store();
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
        let store = test_store();
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
        let store = test_store();
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
        let store = test_store();
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
    fn update_nonexistent_user_role_errors() {
        let store = test_store();
        let result = store.update_user_role("nobody", Role::Operator);
        assert!(result.is_err());
    }

    #[test]
    fn revoke_nonexistent_api_key_errors() {
        let store = test_store();
        let result = store.revoke_api_key("no-such-key");
        assert!(result.is_err());
    }
}
