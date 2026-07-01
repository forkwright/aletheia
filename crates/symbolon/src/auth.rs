// kanon:ignore STORAGE/no-migration-checksum -- auth facade has no schema migrations; AuthStore uses a static fjall partition list without versioned migrations
//! Unified auth facade composing JWT, API keys, password auth, and RBAC.

use std::path::Path;
use std::time::Duration;

use tracing::instrument;

use koina::secret::SecretString;

use crate::api_key;
use crate::error::{self, Result};
use crate::jwt::{JwtConfig, JwtManager};
use crate::password;
use crate::store::AuthStore;
use crate::types::{
    Action, ApiKeyRecord, Claims, ManagedCredential, ManagedCredentialRole, Role, TokenKind,
    TokenPair,
};
use crate::util::days_to_date;

/// Configuration for the auth service.
#[derive(Clone, Default)]
pub struct AuthConfig {
    /// JWT configuration.
    pub jwt: JwtConfig,
}

/// The main auth service: wraps JWT, API keys, and password auth.
pub struct AuthService {
    jwt: JwtManager,
    store: AuthStore,
}

/// Canonical production auth facade.
pub type AuthFacade = AuthService;

/// Claims returned after a token is verified as an administrator token.
// kanon:ignore RUST/no-debug-derive-on-public-types — AdminClaims carries only non-secret identity metadata (sub, role, nous_id); Debug is safe for observability
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminClaims {
    /// Subject identifier for the authenticated administrator.
    pub sub: String,
    /// Authorization role. This is always [`Role::Admin`].
    pub role: Role,
    /// Optional nous scope carried by the token.
    pub nous_id: Option<String>,
}

impl AdminClaims {
    /// Returns `true` if the admin token is scoped to a specific nous instance.
    pub fn is_nous_scoped(&self) -> bool {
        self.nous_id.is_some()
    }
}

impl AuthService {
    /// Create a new auth service backed by the configured auth store.
    ///
    /// # Errors
    ///
    /// Returns an error if the auth store cannot be opened.
    pub fn new(config: AuthConfig, db_path: &Path) -> Result<Self> {
        let store = AuthStore::open(db_path)?;
        let jwt = JwtManager::new(config.jwt);
        Ok(Self { jwt, store })
    }

    /// Create an auth service with an in-memory auth store.
    ///
    /// # Errors
    ///
    /// Returns an error if the temporary auth store cannot be opened.
    pub fn in_memory(config: AuthConfig) -> Result<Self> {
        let store = AuthStore::open_in_memory()?;
        let jwt = JwtManager::new(config.jwt);
        Ok(Self { jwt, store })
    }

    /// Register a new user with a hashed password.
    #[instrument(skip(self, password))]
    pub fn register_user(
        &self,
        username: &str,
        password: &SecretString,
        role: Role,
    ) -> Result<crate::types::User> {
        let hash = password::hash_password(password)?;
        let id = koina::ulid::Ulid::new().to_string();
        self.store.create_user(&id, username, &hash, role)
    }

    /// Authenticate via username + password. Returns a JWT pair.
    #[instrument(skip(self, password))]
    pub fn login(&self, username: &str, password: &SecretString) -> Result<TokenPair> {
        let user = self.store.find_user_by_username(username)?.ok_or_else(|| {
            crate::metrics::record_auth_attempt("password", false);
            error::InvalidCredentialsSnafu.build()
        })?;

        let valid = password::verify_password(password, &user.password_hash)?;
        if !valid {
            crate::metrics::record_auth_attempt("password", false);
            return Err(error::InvalidCredentialsSnafu.build());
        }

        let access = self.jwt.issue_access(&user.id, user.role, None)?;
        let refresh = self.jwt.issue_refresh(&user.id, user.role)?;

        crate::metrics::record_auth_attempt("password", true);
        Ok(TokenPair {
            access_token: SecretString::from(access),
            refresh_token: SecretString::from(refresh),
        })
    }

    /// Authenticate via API key. Returns claims.
    pub fn authenticate_api_key(&self, raw_key: &str) -> Result<Claims> {
        let result = api_key::validate(&self.store, raw_key, self.jwt.issuer());
        crate::metrics::record_auth_attempt("api_key", result.is_ok());
        result
    }

    /// Validate a JWT token. Checks signature, expiry, and revocation.
    pub fn validate_token(&self, token: &str) -> Result<Claims> {
        let claims = self.jwt.validate(token)?;

        if claims.kind != TokenKind::Access {
            return Err(error::InvalidTokenSnafu {
                message: "expected access token".to_owned(),
            }
            .build());
        }

        if self.store.is_token_revoked(&claims.jti)? {
            return Err(error::InvalidTokenSnafu {
                message: "token has been revoked".to_owned(),
            }
            .build());
        }

        Ok(claims)
    }

    /// Validate a JWT access token and require an administrator role.
    ///
    /// # Errors
    ///
    /// Returns an error if the token is invalid, expired, revoked, or not an
    /// administrator token.
    pub fn verify_admin(&self, token: &str) -> Result<AdminClaims> {
        let claims = self.validate_token(token)?;
        if claims.role != Role::Admin {
            return Err(error::PermissionDeniedSnafu {
                action: "verify admin".to_owned(),
                role: claims.role.to_string(),
            }
            .build());
        }
        Ok(AdminClaims {
            sub: claims.sub,
            role: claims.role,
            nous_id: claims.nous_id,
        })
    }

    /// Revoke a JWT access or refresh token.
    ///
    /// # Errors
    ///
    /// Returns an error if the token cannot be validated or persisted to the
    /// revocation store.
    pub fn revoke(&self, token: &str) -> Result<()> {
        self.logout(token)
    }

    /// Refresh a JWT pair using a refresh token.
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
            // WARNING: a revoked refresh token presented again indicates possible
            // token reuse — log at error level for operator visibility.
            // Full session revocation (revoke all tokens for subject) is a
            // future hardening step pending a subject-indexed revocation store.
            tracing::error!(
                sub = %claims.sub,
                "refresh token reuse detected — revoked token presented again"
            );
            return Err(error::InvalidTokenSnafu {
                message: "refresh token has been revoked".to_owned(),
            }
            .build());
        }

        // WHY: revoke the consumed refresh token before issuing the new pair so
        // a stolen token cannot be replayed for the full refresh_ttl window.
        // This enforces the single-use rotation guarantee documented in jwt.rs.
        let consumed_jti = claims.jti.clone();
        let consumed_exp = format_unix_iso(claims.exp);

        self.store.revoke_token(&consumed_jti, &consumed_exp)?;

        // WHY: reuse `JwtManager::refresh` for issuance so the token-rotation
        // logic lives in exactly one place in the production path.
        let pair = self.jwt.refresh(refresh_token)?;

        Ok(TokenPair {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
        })
    }

    /// Logout by revoking a JWT (adds its jti to the revocation list).
    pub fn logout(&self, token: &str) -> Result<()> {
        let claims = self.jwt.validate(token)?;
        let expires_at = format_unix_iso(claims.exp);
        self.store.revoke_token(&claims.jti, &expires_at)
    }

    /// Generate a new API key.
    pub fn generate_api_key(
        &self,
        prefix: &str,
        role: Role,
        nous_id: Option<&str>,
        expires_in: Option<Duration>,
    ) -> Result<(String, ApiKeyRecord)> {
        api_key::generate(
            &self.store,
            prefix,
            role,
            nous_id,
            expires_in,
            self.jwt.issuer(),
        )
    }

    /// Revoke an API key.
    pub fn revoke_api_key(&self, key_id: &str) -> Result<()> {
        api_key::revoke(&self.store, key_id)
    }

    /// List all API keys.
    pub fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>> {
        api_key::list(&self.store)
    }

    /// List managed provider credentials from an instance credential directory.
    ///
    /// Returned records never contain raw secret material.
    pub fn list_credentials(&self, root: &Path) -> Result<Vec<ManagedCredential>> {
        crate::credential::admin::list(root)
    }

    /// Store a managed provider credential.
    ///
    /// The raw key is written encrypted at rest and is not returned.
    pub fn add_credential(
        &self,
        root: &Path,
        provider: &str,
        key: &SecretString,
        role: ManagedCredentialRole,
    ) -> Result<ManagedCredential> {
        crate::credential::admin::add(root, provider, key, role)
    }

    /// Validate a managed provider credential using local secret-file semantics.
    ///
    /// The response never contains raw secret material.
    pub fn validate_credential(&self, root: &Path, id: &str) -> Result<ManagedCredential> {
        crate::credential::admin::validate(root, id)
    }

    /// Swap a provider's primary and backup credentials.
    ///
    /// The returned records never contain raw secret material.
    pub fn rotate_credentials(
        &self,
        root: &Path,
        provider: &str,
    ) -> Result<Vec<ManagedCredential>> {
        crate::credential::admin::rotate(root, provider)
    }

    /// Remove a managed provider credential and its sidecar files.
    pub fn remove_credential(&self, root: &Path, id: &str) -> Result<()> {
        crate::credential::admin::remove(root, id)
    }

    /// Check if claims authorize the given action. Returns `Ok(())` if allowed.
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
///
/// A pure function separate from `AuthService` so it is testable without
/// database setup. The role hierarchy is flat: Admin/Operator > Agent >
/// Readonly. Agent access is scoped to its own `nous_id`.
fn is_authorized(claims: &Claims, action: &Action) -> bool {
    match claims.role {
        Role::Admin | Role::Operator => true,
        // NOTE: Agent role is scoped to a specific nous_id. An agent token
        // without a nous_id claim cannot access any sessions (fails closed).
        Role::Agent => match action {
            Action::ReadSession { nous_id } | Action::WriteSession { nous_id } => {
                claims.nous_id.as_ref().is_some_and(|own| own == nous_id)
            }
            Action::ReadDashboard => true,
            Action::ManageAgents | Action::ManageUsers | Action::ManageCredentials => false,
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

    fn memory_service() -> AuthService {
        AuthService::in_memory(AuthConfig {
            jwt: JwtConfig {
                signing_key: SecretString::from("test-jwt-secret"),
                access_ttl: Duration::from_hours(1),
                refresh_ttl: Duration::from_hours(24),
                issuer: "aletheia-test".to_owned(),
                ..JwtConfig::default()
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
        let svc = memory_service();
        svc.register_user("alice", &secret("hunter2"), Role::Operator)
            .unwrap();

        let pair = svc.login("alice", &secret("hunter2")).unwrap();
        let claims = svc
            .validate_token(pair.access_token.expose_secret())
            .unwrap();
        assert_eq!(claims.role, Role::Operator);
        assert_eq!(claims.kind, TokenKind::Access);
    }

    #[test]
    fn login_wrong_password() {
        let svc = memory_service();
        svc.register_user("alice", &secret("hunter2"), Role::Operator)
            .unwrap();

        let result = svc.login("alice", &secret("wrong"));
        assert!(result.is_err());
    }

    #[test]
    fn login_nonexistent_user() {
        let svc = memory_service();
        let result = svc.login("nobody", &secret("password"));
        assert!(result.is_err());
    }

    #[test]
    fn validate_then_logout_then_reject() {
        let svc = memory_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();

        assert!(
            svc.validate_token(pair.access_token.expose_secret())
                .is_ok()
        );

        svc.logout(pair.access_token.expose_secret()).unwrap();

        let result = svc.validate_token(pair.access_token.expose_secret());
        assert!(result.is_err());
    }

    #[test]
    fn verify_admin_accepts_valid_admin_token() {
        let svc = memory_service();
        let token = svc.jwt.issue_access("alice", Role::Admin, None).unwrap();

        let claims = svc.verify_admin(&token).unwrap();

        assert_eq!(claims.sub, "alice");
        assert_eq!(claims.role, Role::Admin);
    }

    #[test]
    fn verify_admin_rejects_revoked_token() {
        let svc = memory_service();
        let token = svc.jwt.issue_access("alice", Role::Admin, None).unwrap();

        svc.revoke(&token).unwrap();

        assert!(svc.verify_admin(&token).is_err());
    }

    #[test]
    fn verify_admin_rejects_invalid_signature() {
        let svc = memory_service();
        let other = JwtManager::new(JwtConfig {
            signing_key: SecretString::from("different-test-secret"),
            access_ttl: Duration::from_hours(1),
            refresh_ttl: Duration::from_hours(24),
            issuer: "aletheia-test".to_owned(),
            ..JwtConfig::default()
        });
        let token = other.issue_access("alice", Role::Admin, None).unwrap();

        assert!(svc.verify_admin(&token).is_err());
    }

    #[test]
    fn verify_admin_rejects_non_admin_token() {
        let svc = memory_service();
        let token = svc.jwt.issue_access("alice", Role::Operator, None).unwrap();

        assert!(svc.verify_admin(&token).is_err());
    }

    #[test]
    fn refresh_token_flow() {
        let svc = memory_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();

        let new_pair = svc
            .refresh_token(pair.refresh_token.expose_secret())
            .unwrap();
        let claims = svc
            .validate_token(new_pair.access_token.expose_secret())
            .unwrap();
        assert_eq!(claims.role, Role::Operator);
    }

    #[test]
    fn refresh_with_access_token_rejected() {
        let svc = memory_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();

        let result = svc.refresh_token(pair.access_token.expose_secret());
        assert!(result.is_err());
    }

    // ── issue 5448: refresh token rotation (single-use enforcement) ──

    #[test]
    fn refresh_token_is_revoked_after_use() {
        let svc = memory_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();
        let original_refresh = pair.refresh_token.expose_secret().to_owned();

        // First refresh should succeed and consume the original token.
        let _new_pair = svc.refresh_token(&original_refresh).unwrap();

        // Replaying the original refresh token must be rejected.
        let replay_result = svc.refresh_token(&original_refresh);
        assert!(
            replay_result.is_err(),
            "replayed refresh token must be rejected after single use"
        );
    }

    #[test]
    fn rotated_refresh_token_is_independently_valid() {
        let svc = memory_service();
        svc.register_user("alice", &secret("pw"), Role::Operator)
            .unwrap();
        let pair = svc.login("alice", &secret("pw")).unwrap();

        let new_pair = svc
            .refresh_token(pair.refresh_token.expose_secret())
            .unwrap();

        // The new refresh token must work for a subsequent refresh.
        let newer_pair = svc
            .refresh_token(new_pair.refresh_token.expose_secret())
            .unwrap();
        let claims = svc
            .validate_token(newer_pair.access_token.expose_secret())
            .unwrap();
        assert_eq!(claims.role, Role::Operator);
    }

    #[test]
    fn api_key_generate_authenticate_revoke() {
        let svc = memory_service();
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
            nbf: None,
            exp: 0,
            jti: "j1".to_owned(),
            kind: TokenKind::Access,
        };

        let svc = memory_service();
        assert!(
            svc.authorize(
                &claims,
                &Action::ReadSession {
                    nous_id: "syn".to_owned(),
                },
            )
            .is_ok(),
            "Operator should be able to read any session"
        );
        assert!(
            svc.authorize(
                &claims,
                &Action::WriteSession {
                    nous_id: "syn".to_owned(),
                },
            )
            .is_ok(),
            "Operator should be able to write any session"
        );
        assert!(
            svc.authorize(&claims, &Action::ManageAgents).is_ok(),
            "Operator should be able to manage agents"
        );
        assert!(
            svc.authorize(&claims, &Action::ManageUsers).is_ok(),
            "Operator should be able to manage users"
        );
        assert!(
            svc.authorize(&claims, &Action::ReadDashboard).is_ok(),
            "Operator should be able to read dashboard"
        );
    }

    #[test]
    fn agent_scoped_to_own_nous() {
        let claims = Claims {
            sub: "agent-syn".to_owned(),
            role: Role::Agent,
            nous_id: Some("syn".to_owned()),
            iss: "test".to_owned(),
            iat: 0,
            nbf: None,
            exp: 0,
            jti: "j2".to_owned(),
            kind: TokenKind::Access,
        };

        let svc = memory_service();

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
            nbf: None,
            exp: 0,
            jti: "j3".to_owned(),
            kind: TokenKind::Access,
        };

        let svc = memory_service();

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
            nbf: None,
            exp: 0,
            jti: "j4".to_owned(),
            kind: TokenKind::Access,
        };

        let svc = memory_service();
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
        let svc = memory_service();
        let result = svc.login("'; DROP TABLE users; --", &secret("pw"));
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_user_registration_rejected() {
        let svc = memory_service();
        svc.register_user("alice", &secret("pw1"), Role::Operator)
            .unwrap();
        let result = svc.register_user("alice", &secret("pw2"), Role::Readonly);
        assert!(result.is_err());
    }
}
