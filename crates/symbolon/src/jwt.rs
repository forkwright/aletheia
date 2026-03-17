//! JWT token issuance and validation.
//!
//! Implements HS256 (HMAC-SHA256) JWT encode/decode directly using `ring`,
//! eliminating the `jsonwebtoken` crate and its CVE-flagged transitive deps.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::hmac;
use secrecy::{ExposeSecret, SecretString};
use snafu::ensure;
use tracing::instrument;

use crate::error::{self, Result};
use crate::types::{Claims, Role, TokenKind, TokenPair};

/// Fixed HS256 JWT header, pre-encoded as base64url.
const HS256_HEADER_B64: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";

/// Configuration for JWT token management.
pub struct JwtConfig {
    /// HMAC-SHA256 signing key.
    pub signing_key: SecretString,
    /// Access token time-to-live (default: 1 hour).
    pub access_ttl: Duration,
    /// Refresh token time-to-live (default: 7 days).
    pub refresh_ttl: Duration,
    /// Issuer claim value.
    pub issuer: String,
}

impl std::fmt::Debug for JwtConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtConfig")
            .field("signing_key", &"[REDACTED]")
            .field("access_ttl", &self.access_ttl)
            .field("refresh_ttl", &self.refresh_ttl)
            .field("issuer", &self.issuer)
            .finish()
    }
}

/// The insecure placeholder key used when no explicit key is configured.
/// Server startup MUST reject this value when auth is enabled.
pub(crate) const INSECURE_DEFAULT_KEY: &str = "CHANGE-ME-IN-PRODUCTION";

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            signing_key: SecretString::from(INSECURE_DEFAULT_KEY.to_owned()),
            access_ttl: Duration::from_secs(3600),
            refresh_ttl: Duration::from_secs(7 * 24 * 3600),
            issuer: "aletheia".to_owned(),
        }
    }
}

impl JwtConfig {
    /// Returns `true` if the signing key is the insecure placeholder.
    #[must_use]
    pub fn has_insecure_key(&self) -> bool {
        self.signing_key.expose_secret() == INSECURE_DEFAULT_KEY
    }

    /// Reject the insecure default key when the auth mode requires JWT signing.
    ///
    /// Auth mode `"none"` is always allowed (the key is unused). Any other
    /// mode triggers an error if the key is still the default placeholder.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::InsecureKey`] if `auth_mode` is not `"none"` and the signing
    /// key is still the built-in insecure placeholder.
    pub fn validate_for_auth_mode(&self, auth_mode: &str) -> Result<()> {
        if auth_mode != "none" && self.has_insecure_key() {
            tracing::error!(
                auth_mode,
                "JWT signing key is the insecure default, refusing to start"
            );
            return Err(error::InsecureKeySnafu {
                auth_mode: auth_mode.to_owned(),
            }
            .build());
        }
        Ok(())
    }
}

/// Manages JWT issuance and validation.
pub struct JwtManager {
    hmac_key: hmac::Key,
    config: JwtConfig,
}

impl JwtManager {
    /// Create a new JWT manager from the given config.
    pub fn new(config: JwtConfig) -> Self {
        let key_bytes = config.signing_key.expose_secret().as_bytes();
        let hmac_key = hmac::Key::new(hmac::HMAC_SHA256, key_bytes);
        Self { hmac_key, config }
    }

    /// Issue an access token.
    ///
    /// # Errors
    ///
    /// Returns an error if the JWT claims cannot be encoded or signed.
    #[instrument(skip(self), fields(kind = "access"))]
    pub fn issue_access(&self, sub: &str, role: Role, nous_id: Option<&str>) -> Result<String> {
        self.issue(
            sub,
            role,
            nous_id,
            TokenKind::Access,
            self.config.access_ttl,
        )
    }

    /// Issue a refresh token.
    #[instrument(skip(self), fields(kind = "refresh"))]
    pub(crate) fn issue_refresh(&self, sub: &str, role: Role) -> Result<String> {
        self.issue(sub, role, None, TokenKind::Refresh, self.config.refresh_ttl)
    }

    /// Validate a token and return its claims.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ExpiredToken`] if the token's expiration time has passed.
    /// Returns [`crate::error::Error::TokenDecode`] if the token is malformed, has an invalid
    /// signature, or fails any other JWT validation check.
    pub fn validate(&self, token: &str) -> Result<Claims> {
        let claims = decode(token, &self.hmac_key)?;

        ensure!(
            claims.iss == self.config.issuer,
            error::TokenDecodeSnafu {
                message: format!(
                    "issuer mismatch: expected '{}', got '{}'",
                    self.config.issuer, claims.iss
                )
            }
        );

        let now = now_unix();
        if claims.exp <= now {
            return Err(error::ExpiredTokenSnafu.build());
        }

        Ok(claims)
    }

    /// Refresh a token pair: validate the refresh token, issue a new access + refresh pair.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "auth facade internals; only exercised by crate-level tests"
        )
    )]
    #[instrument(skip(self, refresh_token))]
    pub(crate) fn refresh(&self, refresh_token: &str) -> Result<TokenPair> {
        let claims = self.validate(refresh_token)?;

        if claims.kind != TokenKind::Refresh {
            return Err(error::InvalidTokenSnafu {
                message: "expected refresh token, got access token".to_owned(),
            }
            .build());
        }

        let access = self.issue_access(&claims.sub, claims.role, claims.nous_id.as_deref())?;
        let refresh = self.issue_refresh(&claims.sub, claims.role)?;

        Ok(TokenPair {
            access_token: access,
            refresh_token: refresh,
        })
    }

    fn issue(
        &self,
        sub: &str,
        role: Role,
        nous_id: Option<&str>,
        kind: TokenKind,
        ttl: Duration,
    ) -> Result<String> {
        let now = now_unix();
        let claims = Claims {
            sub: sub.to_owned(),
            role,
            nous_id: nous_id.map(str::to_owned),
            iss: self.config.issuer.clone(),
            iat: now,
            // WHY: saturate to i64::MAX: a TTL exceeding ~292 billion years is effectively infinite
            exp: now + i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX),
            jti: ulid::Ulid::new().to_string(),
            kind,
        };

        encode(&claims, &self.hmac_key)
    }
}

/// Encode claims as an HS256 JWT.
///
/// # Errors
///
/// Returns [`crate::error::Error::TokenEncode`] if the claims cannot be serialized to JSON.
pub fn encode(claims: &Claims, key: &hmac::Key) -> Result<String> {
    let payload_json = serde_json::to_vec(claims).map_err(|e| {
        error::TokenEncodeSnafu {
            message: e.to_string(),
        }
        .build()
    })?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(&payload_json);

    let signing_input = format!("{HS256_HEADER_B64}.{payload_b64}");
    let signature = hmac::sign(key, signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(signature.as_ref());

    Ok(format!("{signing_input}.{sig_b64}"))
}

/// Decode and verify an HS256 JWT, returning the claims.
///
/// # Errors
///
/// Returns [`crate::error::Error::TokenDecode`] if the token is malformed or the signature
/// is invalid.
pub fn decode(token: &str, key: &hmac::Key) -> Result<Claims> {
    let (header_payload, sig_b64) = token.rsplit_once('.').ok_or_else(|| {
        error::TokenDecodeSnafu {
            message: "missing signature segment".to_owned(),
        }
        .build()
    })?;

    // Verify there are exactly 3 segments
    if header_payload.matches('.').count() != 1 {
        return Err(error::TokenDecodeSnafu {
            message: "token must have exactly 3 segments".to_owned(),
        }
        .build());
    }

    let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).map_err(|_e| {
        error::TokenDecodeSnafu {
            message: "invalid base64url in signature".to_owned(),
        }
        .build()
    })?;

    hmac::verify(key, header_payload.as_bytes(), &sig_bytes).map_err(|_e| {
        error::TokenDecodeSnafu {
            message: "signature verification failed".to_owned(),
        }
        .build()
    })?;

    let payload_b64 = header_payload
        .split_once('.')
        .map(|(_, p)| p)
        .ok_or_else(|| {
            error::TokenDecodeSnafu {
                message: "missing payload segment".to_owned(),
            }
            .build()
        })?;

    let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).map_err(|_e| {
        error::TokenDecodeSnafu {
            message: "invalid base64url in payload".to_owned(),
        }
        .build()
    })?;

    let claims: Claims = serde_json::from_slice(&payload_bytes).map_err(|e| {
        error::TokenDecodeSnafu {
            message: format!("invalid claims JSON: {e}"),
        }
        .build()
    })?;

    Ok(claims)
}

/// Build an `hmac::Key` from raw secret bytes. Convenience for test helpers.
#[must_use]
pub fn hmac_key(secret: &[u8]) -> hmac::Key {
    hmac::Key::new(hmac::HMAC_SHA256, secret)
}

fn now_unix() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|_| {
                tracing::warn!("system clock before UNIX epoch, using epoch as fallback");
                std::time::Duration::default()
            })
            .as_secs(),
    )
    // WHY: saturate u64 seconds to i64::MAX (~year 292B) to prevent overflow
    .unwrap_or(i64::MAX)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn test_manager() -> JwtManager {
        JwtManager::new(JwtConfig {
            signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
            access_ttl: Duration::from_secs(3600),
            refresh_ttl: Duration::from_secs(86400),
            issuer: "aletheia-test".to_owned(),
        })
    }

    #[test]
    fn issue_and_validate_access_token() {
        let mgr = test_manager();
        let token = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        let claims = mgr.validate(&token).unwrap();
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.role, Role::Operator);
        assert_eq!(claims.kind, TokenKind::Access);
        assert!(claims.nous_id.is_none());
    }

    #[test]
    fn issue_and_validate_agent_token() {
        let mgr = test_manager();
        let token = mgr
            .issue_access("agent-syn", Role::Agent, Some("syn"))
            .unwrap();
        let claims = mgr.validate(&token).unwrap();
        assert_eq!(claims.sub, "agent-syn");
        assert_eq!(claims.role, Role::Agent);
        assert_eq!(claims.nous_id.as_deref(), Some("syn"));
    }

    #[test]
    fn issue_and_validate_refresh_token() {
        let mgr = test_manager();
        let token = mgr.issue_refresh("user-1", Role::Operator).unwrap();
        let claims = mgr.validate(&token).unwrap();
        assert_eq!(claims.kind, TokenKind::Refresh);
    }

    #[test]
    fn wrong_signing_key_rejected() {
        let mgr1 = test_manager();
        let mgr2 = JwtManager::new(JwtConfig {
            signing_key: SecretString::from("different-key".to_owned()),
            ..JwtConfig::default()
        });

        let token = mgr1.issue_access("user-1", Role::Operator, None).unwrap();
        assert!(mgr2.validate(&token).is_err());
    }

    #[test]
    fn expired_token_rejected() {
        let mgr = test_manager();
        let key = hmac_key(b"test-secret-key-for-jwt");

        let claims = Claims {
            sub: "user-1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            iss: "aletheia-test".to_owned(),
            iat: 1_000_000,
            exp: 1_000_001, // 1970: long expired
            jti: "expired-jti".to_owned(),
            kind: TokenKind::Access,
        };
        let token = encode(&claims, &key).unwrap();

        let result = mgr.validate(&token);
        assert!(result.is_err());
    }

    #[test]
    fn refresh_flow_produces_valid_tokens() {
        let mgr = test_manager();
        let refresh = mgr.issue_refresh("user-1", Role::Operator).unwrap();
        let pair = mgr.refresh(&refresh).unwrap();

        let access_claims = mgr.validate(&pair.access_token).unwrap();
        assert_eq!(access_claims.sub, "user-1");
        assert_eq!(access_claims.kind, TokenKind::Access);

        let refresh_claims = mgr.validate(&pair.refresh_token).unwrap();
        assert_eq!(refresh_claims.kind, TokenKind::Refresh);
    }

    #[test]
    fn refresh_with_access_token_rejected() {
        let mgr = test_manager();
        let access = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        let result = mgr.refresh(&access);
        assert!(result.is_err());
    }

    #[test]
    fn claims_jti_is_unique() {
        let mgr = test_manager();
        let t1 = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        let t2 = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        let c1 = mgr.validate(&t1).unwrap();
        let c2 = mgr.validate(&t2).unwrap();
        assert_ne!(c1.jti, c2.jti);
    }

    #[test]
    fn config_debug_redacts_key() {
        let config = JwtConfig {
            signing_key: SecretString::from("super-secret".to_owned()),
            ..JwtConfig::default()
        };
        let debug_output = format!("{config:?}");
        assert!(!debug_output.contains("super-secret"));
        assert!(debug_output.contains("[REDACTED]"));
    }

    #[test]
    fn malformed_token_rejected() {
        let mgr = test_manager();
        assert!(mgr.validate("not.a.jwt").is_err());
        assert!(mgr.validate("").is_err());
        assert!(mgr.validate("abc123").is_err());
    }

    #[test]
    fn has_insecure_key_true_for_default_config() {
        let config = JwtConfig::default();
        assert!(config.has_insecure_key());
    }

    #[test]
    fn has_insecure_key_false_for_custom_key() {
        let config = JwtConfig {
            signing_key: SecretString::from("my-secure-production-key".to_owned()),
            ..JwtConfig::default()
        };
        assert!(!config.has_insecure_key());
    }

    #[test]
    fn rejects_insecure_key_with_jwt_auth_mode() {
        let config = JwtConfig::default();
        assert!(config.validate_for_auth_mode("jwt").is_err());
        assert!(config.validate_for_auth_mode("token").is_err());
    }

    #[test]
    fn allows_insecure_key_with_auth_mode_none() {
        let config = JwtConfig::default();
        assert!(config.validate_for_auth_mode("none").is_ok());
    }

    #[test]
    fn allows_secure_key_with_any_auth_mode() {
        let config = JwtConfig {
            signing_key: SecretString::from("my-secure-production-key".to_owned()),
            ..JwtConfig::default()
        };
        assert!(config.validate_for_auth_mode("jwt").is_ok());
        assert!(config.validate_for_auth_mode("token").is_ok());
        assert!(config.validate_for_auth_mode("none").is_ok());
    }

    #[test]
    fn tampered_payload_rejected() {
        let mgr = test_manager();
        let token = mgr.issue_access("user-1", Role::Operator, None).unwrap();

        // Tamper with the payload segment
        let parts: Vec<&str> = token.splitn(3, '.').collect();
        let tampered = format!("{}.dGFtcGVyZWQ.{}", parts[0], parts[2]);
        assert!(mgr.validate(&tampered).is_err());
    }

    #[test]
    fn tampered_signature_rejected() {
        let mgr = test_manager();
        let token = mgr.issue_access("user-1", Role::Operator, None).unwrap();

        // Replace last character of signature
        let mut tampered = token.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert!(mgr.validate(&tampered).is_err());
    }

    #[test]
    fn token_has_three_dot_separated_segments() {
        let mgr = test_manager();
        let token = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        assert_eq!(
            token.matches('.').count(),
            2,
            "JWT must have exactly 3 segments"
        );
    }

    #[test]
    fn roundtrip_preserves_all_claims_fields() {
        let mgr = test_manager();
        let token = mgr
            .issue_access("agent-syn", Role::Agent, Some("syn-nous"))
            .unwrap();
        let claims = mgr.validate(&token).unwrap();

        assert_eq!(claims.sub, "agent-syn");
        assert_eq!(claims.role, Role::Agent);
        assert_eq!(claims.nous_id.as_deref(), Some("syn-nous"));
        assert_eq!(claims.iss, "aletheia-test");
        assert_eq!(claims.kind, TokenKind::Access);
        assert!(claims.iat > 0, "iat must be positive");
        assert!(claims.exp > claims.iat, "exp must be after iat");
        assert!(!claims.jti.is_empty(), "jti must be non-empty");
    }

    #[test]
    fn issuer_mismatch_rejected() {
        let mgr1 = JwtManager::new(JwtConfig {
            signing_key: SecretString::from("shared-key".to_owned()),
            issuer: "issuer-a".to_owned(),
            ..JwtConfig::default()
        });
        let mgr2 = JwtManager::new(JwtConfig {
            signing_key: SecretString::from("shared-key".to_owned()),
            issuer: "issuer-b".to_owned(),
            ..JwtConfig::default()
        });

        let token = mgr1.issue_access("user-1", Role::Operator, None).unwrap();
        assert!(mgr2.validate(&token).is_err());
    }
}
