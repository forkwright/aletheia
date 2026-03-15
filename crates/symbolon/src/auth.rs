//! Unified auth facade composing JWT, API keys, password auth, and RBAC.
#![expect(
    dead_code,
    reason = "auth facade internals; only exercised by crate-level tests"
)]

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
use crate::util::days_to_date;

/// Configuration for the auth service.
#[derive(Default)]
pub(crate) struct AuthConfig {
    /// JWT configuration.
    pub jwt: JwtConfig,
}

/// The main auth service — wraps JWT, API keys, and password auth.
pub(crate) struct AuthService {
    jwt: JwtManager,
    store: AuthStore,
}

impl AuthService {
    /// Create a new auth service backed by a `SQLite` database.
    pub(crate) fn new(config: AuthConfig, db_path: &Path) -> Result<Self> {
        let store = AuthStore::open(db_path)?;
        let jwt = JwtManager::new(config.jwt);
        Ok(Self { jwt, store })
    }

    /// Create an auth service with an in-memory database (for testing).
    pub(crate) fn in_memory(config: AuthConfig) -> Result<Self> {
        let store = AuthStore::open_in_memory()?;
        let jwt = JwtManager::new(config.jwt);
        Ok(Self { jwt, store })
    }

    /// Get a reference to the underlying store.
    #[must_use]
    pub(crate) fn store(&self) -> &AuthStore {
        &self.store
    }

    /// Register a new user with a hashed password.
    #[instrument(skip(self, password))]
    pub(crate) fn register_user(
        &self,
        username: &str,
        password: &SecretString,
        role: Role,
    ) -> Result<crate::types::User> {
        let hash = password::hash_password(password)?;
        let id = ulid::Ulid::new().to_string();
        self.store.create_user(&id, username, &hash, role)
    }

    /// Authenticate via username + password. Returns a JWT pair.
    #[instrument(skip(self, password))]
    pub(crate) fn login(&self, username: &str, password: &SecretString) -> Result<TokenPair> {
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

    /// Authenticate via API key. Returns claims.
    pub(crate) fn authenticate_api_key(&self, raw_key: &str) -> Result<Claims> {
        api_key::validate(&self.store, raw_key)
    }

    /// Validate a JWT token. Checks signature, expiry, and revocation.
    pub(crate) fn validate_token(&self, token: &str) -> Result<Claims> {
        let claims = self.jwt.validate(token)?;

        if self.store.is_token_revoked(&claims.jti)? {
            return Err(error::InvalidTokenSnafu {
                message: "token has been revoked".to_owned(),
            }
            .build());
        }

        Ok(claims)
    }

    /// Refresh a JWT pair using a refresh token.
    #[instrument(skip(self, refresh_token))]
    pub(crate) fn refresh_token(&self, refresh_token: &str) -> Result<TokenPair> {
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

    /// Logout by revoking a JWT (adds its jti to the revocation list).
    pub(crate) fn logout(&self, token: &str) -> Result<()> {
        let claims = self.jwt.validate(token)?;
        let expires_at = format_unix_iso(claims.exp);
        self.store.revoke_token(&claims.jti, &expires_at)
    }

    /// Generate a new API key.
    pub(crate) fn generate_api_key(
        &self,
        prefix: &str,
        role: Role,
        nous_id: Option<&str>,
        expires_in: Option<Duration>,
    ) -> Result<(String, ApiKeyRecord)> {
        api_key::generate(&self.store, prefix, role, nous_id, expires_in)
    }

    /// Revoke an API key.
    pub(crate) fn revoke_api_key(&self, key_id: &str) -> Result<()> {
        api_key::revoke(&self.store, key_id)
    }

    /// List all API keys.
    pub(crate) fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>> {
        api_key::list(&self.store)
    }

    /// Check if claims authorize the given action. Returns `Ok(())` if allowed.
    #[expect(
        clippy::unused_self,
        reason = "method semantically belongs to AuthService instance"
    )]
    pub(crate) fn authorize(&self, claims: &Claims, action: &Action) -> Result<()> {
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
    let secs = u64::try_from(unix_secs).unwrap_or_else(|_| {
        tracing::warn!(unix_secs, "negative unix timestamp, clamping to epoch");
        0
    });
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_date(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.000Z")
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
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

    #[test]
    fn format_unix_iso_positive_timestamp() {
        let result = format_unix_iso(1_700_000_000);
        assert!(result.contains("2023"), "expected year 2023, got {result}");
    }

    #[test]
    fn format_unix_iso_negative_timestamp_clamps_to_epoch() {
        let result = format_unix_iso(-100);
        assert_eq!(result, "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn format_unix_iso_zero_is_epoch() {
        let result = format_unix_iso(0);
        assert_eq!(result, "1970-01-01T00:00:00.000Z");
    }

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

    #[test]
    fn validate_then_logout_then_reject() {
        let svc = test_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();

        assert!(svc.validate_token(&pair.access_token).is_ok());

        svc.logout(&pair.access_token).unwrap();

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

        assert!(svc.authorize(&claims, &Action::ManageAgents).is_err());
        assert!(svc.authorize(&claims, &Action::ManageUsers).is_err());

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

    #[test]
    fn sql_injection_in_username_parameterized() {
        let svc = test_service();
        let result = svc.login("'; DROP TABLE users; --", &secret("pw"));
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_user_registration_rejected() {
        let svc = test_service();
        svc.register_user("alice", &secret("pw1"), Role::Operator)
            .unwrap();
        let result = svc.register_user("alice", &secret("pw2"), Role::Readonly);
        assert!(result.is_err());
    }
}
