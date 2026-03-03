//! Unified auth facade composing JWT, API keys, password auth, and RBAC.
//!
//! [`AuthService`] is the primary entry point for all authentication operations.
//! It composes [`crate::jwt::JwtManager`], [`crate::store::AuthStore`],
//! [`crate::api_key`], and [`crate::password`] into a single API.

use std::path::Path;
use std::time::Duration;

use secrecy::SecretString;
use tracing::instrument;

use crate::api_key;
use crate::error::{self, Result};
use crate::jwt::{JwtConfig, JwtManager};
use crate::password;
use crate::store::AuthStore;
use crate::types::{Action, ApiKeyRecord, Claims, Role, TokenKind, TokenPair};

/// Configuration for the auth service.
#[derive(Default)]
pub struct AuthConfig {
    /// JWT configuration.
    pub jwt: JwtConfig,
}

/// The main auth service — wraps JWT, API keys, and password auth.
pub struct AuthService {
    jwt: JwtManager,
    store: AuthStore,
}

impl AuthService {
    /// Create a new auth service backed by a `SQLite` database.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Database`] if the database cannot be opened or
    /// schema initialization fails.
    pub fn new(config: AuthConfig, db_path: &Path) -> Result<Self> {
        let store = AuthStore::open(db_path)?;
        let jwt = JwtManager::new(config.jwt);
        Ok(Self { jwt, store })
    }

    /// Create an auth service with an in-memory database (for testing).
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Database`] if `SQLite` in-memory initialization fails.
    pub fn in_memory(config: AuthConfig) -> Result<Self> {
        let store = AuthStore::open_in_memory()?;
        let jwt = JwtManager::new(config.jwt);
        Ok(Self { jwt, store })
    }

    /// Get a reference to the underlying store.
    #[must_use]
    pub fn store(&self) -> &AuthStore {
        &self.store
    }

    // --- User management ---

    /// Register a new user with a hashed password.
    #[instrument(skip(self, password))]
    pub fn register_user(
        &self,
        username: &str,
        password: &SecretString,
        role: Role,
    ) -> Result<crate::types::User> {
        let hash = password::hash_password(password)?;
        let id = ulid::Ulid::new().to_string();
        self.store.create_user(&id, username, &hash, role)
    }

    // --- Authentication ---

    /// Authenticate via username + password. Returns a JWT pair.
    ///
    /// # Errors
    ///
    /// - [`crate::error::Error::InvalidCredentials`] if the username is not found or
    ///   the password does not match.
    /// - [`crate::error::Error::Hash`] if password verification fails (malformed stored hash).
    /// - [`crate::error::Error::Database`] on `SQLite` access failure.
    #[instrument(skip(self, password))]
    pub fn login(&self, username: &str, password: &SecretString) -> Result<TokenPair> {
        let user = self
            .store
            .find_user_by_username(username)?
            .ok_or_else(|| error::InvalidCredentialsSnafu.build())?;

        let valid = password::verify_password(password, &user.password_hash)?;
        if !valid {
            return Err(error::InvalidCredentialsSnafu.build());
        }

        let access = self.jwt.issue_access(&user.id, user.role, None)?;
        let refresh = self.jwt.issue_refresh(&user.id, user.role)?;

        Ok(TokenPair {
            access_token: access,
            refresh_token: refresh,
        })
    }

    /// Authenticate via API key. Returns claims if the key is valid and not revoked.
    ///
    /// # Errors
    ///
    /// - [`crate::error::Error::InvalidApiKey`] if the key format is malformed.
    /// - [`crate::error::Error::InvalidCredentials`] if the key is not found or has been revoked.
    /// - [`crate::error::Error::ExpiredToken`] if the key has expired.
    /// - [`crate::error::Error::Database`] on `SQLite` access failure.
    pub fn authenticate_api_key(&self, raw_key: &str) -> Result<Claims> {
        api_key::validate(&self.store, raw_key)
    }

    /// Validate a JWT token. Checks signature, expiry, and revocation.
    ///
    /// # Errors
    ///
    /// - [`crate::error::Error::TokenDecode`] if the token is malformed or has an invalid signature.
    /// - [`crate::error::Error::ExpiredToken`] if the token has expired.
    /// - [`crate::error::Error::InvalidToken`] if the token appears in the revocation list.
    /// - [`crate::error::Error::Database`] on `SQLite` access failure.
    pub fn validate_token(&self, token: &str) -> Result<Claims> {
        let claims = self.jwt.validate(token)?;

        if self.store.is_token_revoked(&claims.jti)? {
            return Err(error::InvalidTokenSnafu {
                message: "token has been revoked".to_owned(),
            }
            .build());
        }

        Ok(claims)
    }

    /// Refresh a JWT pair using a refresh token. Issues a new access + refresh pair.
    ///
    /// # Errors
    ///
    /// - [`crate::error::Error::InvalidToken`] if the token is not a refresh token, or
    ///   has been revoked.
    /// - [`crate::error::Error::TokenDecode`] if the token is malformed or has an invalid signature.
    /// - [`crate::error::Error::ExpiredToken`] if the refresh token has expired.
    /// - [`crate::error::Error::Database`] on `SQLite` access failure.
    #[instrument(skip(self, refresh_token))]
    pub fn refresh_token(&self, refresh_token: &str) -> Result<TokenPair> {
        let claims = self.jwt.validate(refresh_token)?;

        if claims.kind != TokenKind::Refresh {
            return Err(error::InvalidTokenSnafu {
                message: "expected refresh token".to_owned(),
            }
            .build());
        }

        if self.store.is_token_revoked(&claims.jti)? {
            return Err(error::InvalidTokenSnafu {
                message: "refresh token has been revoked".to_owned(),
            }
            .build());
        }

        let access = self
            .jwt
            .issue_access(&claims.sub, claims.role, claims.nous_id.as_deref())?;
        let refresh = self.jwt.issue_refresh(&claims.sub, claims.role)?;

        Ok(TokenPair {
            access_token: access,
            refresh_token: refresh,
        })
    }

    /// Logout by revoking a JWT (adds its `jti` to the revocation list).
    ///
    /// The token must still be valid (not yet expired and correctly signed). Revocation
    /// entries are cleaned up by [`crate::store::AuthStore::cleanup_expired_revocations`].
    ///
    /// # Errors
    ///
    /// - [`crate::error::Error::TokenDecode`] if the token is malformed or has an invalid signature.
    /// - [`crate::error::Error::ExpiredToken`] if the token has already expired.
    /// - [`crate::error::Error::Database`] on `SQLite` access failure.
    pub fn logout(&self, token: &str) -> Result<()> {
        let claims = self.jwt.validate(token)?;
        let expires_at = format_unix_iso(claims.exp);
        self.store.revoke_token(&claims.jti, &expires_at)
    }

    // --- API key management ---

    /// Generate a new API key. Returns `(full_key_string, metadata_record)`.
    ///
    /// The full key string (`ale_{prefix}_{secret}`) is returned exactly once and must
    /// be delivered to the caller immediately — only the blake3 hash is stored.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Database`] if the key cannot be persisted.
    pub fn generate_api_key(
        &self,
        prefix: &str,
        role: Role,
        nous_id: Option<&str>,
        expires_in: Option<Duration>,
    ) -> Result<(String, ApiKeyRecord)> {
        api_key::generate(&self.store, prefix, role, nous_id, expires_in)
    }

    /// Revoke an API key by its record ID (not the key string).
    ///
    /// # Errors
    ///
    /// - [`crate::error::Error::NotFound`] if no key with that ID exists.
    /// - [`crate::error::Error::Database`] on `SQLite` access failure.
    pub fn revoke_api_key(&self, key_id: &str) -> Result<()> {
        api_key::revoke(&self.store, key_id)
    }

    /// List all API keys (metadata only — never the secret).
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::Database`] on `SQLite` access failure.
    pub fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>> {
        api_key::list(&self.store)
    }

    // --- Authorization ---

    /// Check if claims authorize the given action. Returns `Ok(())` if allowed.
    ///
    /// Authorization matrix: Operator can do everything. Agent can access only its own
    /// sessions (scoped by [`crate::types::Claims::nous_id`]). Readonly can only read the dashboard.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::PermissionDenied`] if the role is insufficient for the action.
    pub fn authorize(&self, claims: &Claims, action: &Action) -> Result<()> {
        if is_authorized(claims, action) {
            Ok(())
        } else {
            Err(error::PermissionDeniedSnafu {
                action: action.to_string(),
                role: claims.role.to_string(),
            }
            .build())
        }
    }
}

/// RBAC authorization logic.
fn is_authorized(claims: &Claims, action: &Action) -> bool {
    match claims.role {
        Role::Operator => true,
        Role::Agent => match action {
            Action::ReadSession { nous_id } | Action::WriteSession { nous_id } => {
                claims.nous_id.as_ref().is_some_and(|own| own == nous_id)
            }
            Action::ReadDashboard => true,
            Action::ManageAgents | Action::ManageUsers => false,
        },
        Role::Readonly => matches!(action, Action::ReadDashboard),
    }
}

fn format_unix_iso(unix_secs: i64) -> String {
    let secs = u64::try_from(unix_secs).unwrap_or(0);
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_service() -> AuthService {
        AuthService::in_memory(AuthConfig {
            jwt: JwtConfig {
                signing_key: SecretString::from("test-jwt-secret".to_owned()),
                access_ttl: Duration::from_secs(3600),
                refresh_ttl: Duration::from_secs(86400),
                issuer: "aletheia-test".to_owned(),
            },
        })
        .unwrap()
    }

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    // --- Login flow ---

    #[test]
    fn register_and_login() {
        let svc = test_service();
        svc.register_user("alice", &secret("hunter2"), Role::Operator)
            .unwrap();

        let pair = svc.login("alice", &secret("hunter2")).unwrap();
        let claims = svc.validate_token(&pair.access_token).unwrap();
        assert_eq!(claims.role, Role::Operator);
        assert_eq!(claims.kind, TokenKind::Access);
    }

    #[test]
    fn login_wrong_password() {
        let svc = test_service();
        svc.register_user("alice", &secret("hunter2"), Role::Operator)
            .unwrap();

        let result = svc.login("alice", &secret("wrong"));
        assert!(result.is_err());
    }

    #[test]
    fn login_nonexistent_user() {
        let svc = test_service();
        let result = svc.login("nobody", &secret("password"));
        assert!(result.is_err());
    }

    // --- Token lifecycle ---

    #[test]
    fn validate_then_logout_then_reject() {
        let svc = test_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();

        // Token is valid
        assert!(svc.validate_token(&pair.access_token).is_ok());

        // Logout (revoke)
        svc.logout(&pair.access_token).unwrap();

        // Token is now rejected
        let result = svc.validate_token(&pair.access_token);
        assert!(result.is_err());
    }

    #[test]
    fn refresh_token_flow() {
        let svc = test_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();

        let new_pair = svc.refresh_token(&pair.refresh_token).unwrap();
        let claims = svc.validate_token(&new_pair.access_token).unwrap();
        assert_eq!(claims.role, Role::Operator);
    }

    #[test]
    fn refresh_with_access_token_rejected() {
        let svc = test_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();

        let result = svc.refresh_token(&pair.access_token);
        assert!(result.is_err());
    }

    // --- API key flow ---

    #[test]
    fn api_key_generate_authenticate_revoke() {
        let svc = test_service();
        let (key, record) = svc
            .generate_api_key("test", Role::Operator, None, None)
            .unwrap();

        let claims = svc.authenticate_api_key(&key).unwrap();
        assert_eq!(claims.role, Role::Operator);

        svc.revoke_api_key(&record.id).unwrap();
        assert!(svc.authenticate_api_key(&key).is_err());
    }

    // --- RBAC ---

    #[test]
    fn operator_can_do_everything() {
        let claims = Claims {
            sub: "op-1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            iss: "test".to_owned(),
            iat: 0,
            exp: 0,
            jti: "j1".to_owned(),
            kind: TokenKind::Access,
        };

        let svc = test_service();
        svc.authorize(
            &claims,
            &Action::ReadSession {
                nous_id: "syn".to_owned(),
            },
        )
        .unwrap();
        svc.authorize(
            &claims,
            &Action::WriteSession {
                nous_id: "syn".to_owned(),
            },
        )
        .unwrap();
        svc.authorize(&claims, &Action::ManageAgents).unwrap();
        svc.authorize(&claims, &Action::ManageUsers).unwrap();
        svc.authorize(&claims, &Action::ReadDashboard).unwrap();
    }

    #[test]
    fn agent_scoped_to_own_nous() {
        let claims = Claims {
            sub: "agent-syn".to_owned(),
            role: Role::Agent,
            nous_id: Some("syn".to_owned()),
            iss: "test".to_owned(),
            iat: 0,
            exp: 0,
            jti: "j2".to_owned(),
            kind: TokenKind::Access,
        };

        let svc = test_service();

        // Can access own sessions
        svc.authorize(
            &claims,
            &Action::ReadSession {
                nous_id: "syn".to_owned(),
            },
        )
        .unwrap();
        svc.authorize(
            &claims,
            &Action::WriteSession {
                nous_id: "syn".to_owned(),
            },
        )
        .unwrap();

        // Cannot access other agent's sessions
        assert!(
            svc.authorize(
                &claims,
                &Action::ReadSession {
                    nous_id: "demiurge".to_owned()
                }
            )
            .is_err()
        );
        assert!(
            svc.authorize(
                &claims,
                &Action::WriteSession {
                    nous_id: "demiurge".to_owned()
                }
            )
            .is_err()
        );

        // Cannot manage
        assert!(svc.authorize(&claims, &Action::ManageAgents).is_err());
        assert!(svc.authorize(&claims, &Action::ManageUsers).is_err());

        // Can read dashboard
        svc.authorize(&claims, &Action::ReadDashboard).unwrap();
    }

    #[test]
    fn readonly_can_only_read_dashboard() {
        let claims = Claims {
            sub: "viewer-1".to_owned(),
            role: Role::Readonly,
            nous_id: None,
            iss: "test".to_owned(),
            iat: 0,
            exp: 0,
            jti: "j3".to_owned(),
            kind: TokenKind::Access,
        };

        let svc = test_service();

        svc.authorize(&claims, &Action::ReadDashboard).unwrap();

        assert!(
            svc.authorize(
                &claims,
                &Action::ReadSession {
                    nous_id: "syn".to_owned()
                }
            )
            .is_err()
        );
        assert!(
            svc.authorize(
                &claims,
                &Action::WriteSession {
                    nous_id: "syn".to_owned()
                }
            )
            .is_err()
        );
        assert!(svc.authorize(&claims, &Action::ManageAgents).is_err());
        assert!(svc.authorize(&claims, &Action::ManageUsers).is_err());
    }

    #[test]
    fn agent_without_nous_id_cannot_access_sessions() {
        let claims = Claims {
            sub: "agent-orphan".to_owned(),
            role: Role::Agent,
            nous_id: None,
            iss: "test".to_owned(),
            iat: 0,
            exp: 0,
            jti: "j4".to_owned(),
            kind: TokenKind::Access,
        };

        let svc = test_service();
        assert!(
            svc.authorize(
                &claims,
                &Action::ReadSession {
                    nous_id: "syn".to_owned()
                }
            )
            .is_err()
        );
    }

    // --- SQL injection safety ---

    #[test]
    fn sql_injection_in_username_parameterized() {
        let svc = test_service();
        // This should not cause SQL injection — parameterized queries handle it
        let result = svc.login("'; DROP TABLE users; --", &secret("pw"));
        assert!(result.is_err()); // Just a "not found" error, no SQL injection
    }

    // --- Duplicate registration ---

    #[test]
    fn duplicate_user_registration_rejected() {
        let svc = test_service();
        svc.register_user("alice", &secret("pw1"), Role::Operator)
            .unwrap();
        let result = svc.register_user("alice", &secret("pw2"), Role::Readonly);
        assert!(result.is_err());
    }
}
