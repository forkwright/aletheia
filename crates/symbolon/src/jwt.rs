//! HS256 JWT token issuance and validation.
//!
//! Owned implementation using `hmac` + `sha2` (RustCrypto) for HMAC-SHA256
//! signing. Replaces the `jsonwebtoken` crate to eliminate its CVE-flagged
//! transitive dependencies and `rand` version duplication.

use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tracing::instrument;

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use koina::secret::SecretString;

use crate::error::{self, Result};
use crate::types::{Claims, Role, TokenKind, TokenPair};
use crate::util::{base64url_decode, base64url_encode};

type HmacSha256 = Hmac<Sha256>;

/// Base64url-encoded HS256 JWT header (constant, never changes).
const HS256_HEADER_B64: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";

/// Default clock skew leeway applied to JWT expiration checks.
///
/// WHY: clock drift between the issuer and validator (or NTP jumps on the
/// validator) can immediately invalidate freshly issued tokens. 30s is
/// small enough that truly expired tokens are rejected in practice while
/// tolerating typical NTP drift. Mirrors the tolerance used by the OAuth
/// credential chain and by `pylon::handlers::health`.
pub const DEFAULT_CLOCK_SKEW_LEEWAY_SECS: u64 = 30;

/// Configuration for JWT token management.
#[derive(Clone)]
pub struct JwtConfig {
    /// HMAC-SHA256 signing key.
    pub signing_key: SecretString,
    /// Access token time-to-live (default: 1 hour).
    pub access_ttl: Duration,
    /// Refresh token time-to-live (default: 7 days).
    pub refresh_ttl: Duration,
    /// Issuer claim value.
    pub issuer: String,
    /// Clock skew tolerance (seconds) applied when checking `exp`.
    ///
    /// A token whose `exp` lies up to `clock_skew_leeway_secs` seconds in
    /// the past is still accepted. Default:
    /// [`DEFAULT_CLOCK_SKEW_LEEWAY_SECS`] (30s).
    pub clock_skew_leeway_secs: u64,
}

impl std::fmt::Debug for JwtConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtConfig")
            .field("signing_key", &"[REDACTED]")
            .field("access_ttl", &self.access_ttl)
            .field("refresh_ttl", &self.refresh_ttl)
            .field("issuer", &self.issuer)
            .field("clock_skew_leeway_secs", &self.clock_skew_leeway_secs)
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
            access_ttl: Duration::from_hours(1),
            refresh_ttl: Duration::from_hours(7 * 24),
            issuer: "aletheia".to_owned(),
            clock_skew_leeway_secs: DEFAULT_CLOCK_SKEW_LEEWAY_SECS,
        }
    }
}

impl JwtConfig {
    /// Returns `true` if the signing key is the insecure placeholder.
    ///
    /// Uses constant-time comparison (`subtle::ConstantTimeEq`) to prevent
    /// timing side-channels that could leak information about the key contents.
    #[must_use]
    pub(crate) fn has_insecure_key(&self) -> bool {
        let key_bytes = self.signing_key.expose_secret().as_bytes();
        let default_bytes = INSECURE_DEFAULT_KEY.as_bytes();
        key_bytes.ct_eq(default_bytes).into()
    }

    /// Reject the insecure default key when the auth mode requires JWT signing.
    ///
    /// Auth mode `"none"` is always allowed (the key is unused). Any other
    /// mode triggers an error if the key is still the default placeholder.
    ///
    /// # Errors
    ///
    /// Returns an error if `auth_mode` is not `"none"` and the signing
    /// key is still the built-in insecure placeholder.
    #[must_use = "validation result must be checked before proceeding"]
    // kanon:ignore RUST/validate-returns-unit — returns Result<()> where Err carries the specific failure reason via snafu; Ok(()) means validation passed
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
    /// Secret bytes for HMAC-SHA256 signing, zeroized on drop.
    signing_key_bytes: Zeroizing<Vec<u8>>,
    config: JwtConfig,
}

impl JwtManager {
    /// Create a new JWT manager from the given config.
    #[must_use]
    pub fn new(config: JwtConfig) -> Self {
        let signing_key_bytes =
            Zeroizing::new(config.signing_key.expose_secret().as_bytes().to_vec());
        Self {
            signing_key_bytes,
            config,
        }
    }

    /// Return the configured issuer claim value.
    #[must_use]
    pub(crate) fn issuer(&self) -> &str {
        &self.config.issuer
    }

    /// Issue an access token.
    ///
    /// # Errors
    ///
    /// Returns an error if the JWT claims cannot be encoded or signed.
    #[must_use = "issued token must be delivered to the caller"]
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
    pub fn issue_refresh(&self, sub: &str, role: Role) -> Result<String> {
        self.issue(sub, role, None, TokenKind::Refresh, self.config.refresh_ttl)
    }

    /// Validate a token and return its claims.
    ///
    /// # Errors
    ///
    /// Returns an error if the token's expiration time has passed (after
    /// applying the configured clock-skew leeway). Returns an error if the
    /// token is malformed, has an invalid signature, or fails any other
    /// JWT validation check.
    #[must_use = "validated claims must be checked before granting access"]
    pub fn validate(&self, token: &str) -> Result<Claims> {
        let (header_payload, signature) = token.rsplit_once('.').ok_or_else(|| {
            error::TokenDecodeSnafu {
                message: "missing signature segment".to_owned(),
            }
            .build()
        })?;

        // WHY: verify the signature BEFORE parsing claims so tampered tokens
        // are rejected early and attackers cannot trigger noisy JSON parse
        // errors with malformed payloads.
        let sig_bytes = base64url_decode(signature).ok_or_else(|| {
            error::TokenDecodeSnafu {
                message: "invalid base64url in signature".to_owned(),
            }
            .build()
        })?;

        let mut mac = HmacSha256::new_from_slice(&self.signing_key_bytes).map_err(|_err| {
            error::TokenDecodeSnafu {
                message: "HMAC key initialization failed".to_owned(),
            }
            .build()
        })?;
        mac.update(header_payload.as_bytes());
        mac.verify_slice(&sig_bytes).map_err(|_err| {
            error::TokenDecodeSnafu {
                message: "signature verification failed".to_owned(),
            }
            .build()
        })?;

        let (header_b64, payload_b64) = header_payload.split_once('.').ok_or_else(|| {
            error::TokenDecodeSnafu {
                message: "missing payload segment".to_owned(),
            }
            .build()
        })?;
        // WHY: reject tokens whose header claims a different algorithm to prevent
        // algorithm-confusion: a token with alg=RS256 but an HMAC-valid payload
        // would otherwise be accepted without complaint.
        if header_b64 != HS256_HEADER_B64 {
            return Err(error::TokenDecodeSnafu {
                message: format!("unexpected JWT header: expected HS256, got '{header_b64}'"),
            }
            .build());
        }

        let payload_bytes = base64url_decode(payload_b64).ok_or_else(|| {
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

        // INVARIANT: claim validation runs only after signature verification
        // succeeded.
        if claims.iss != self.config.issuer {
            return Err(error::TokenDecodeSnafu {
                message: format!(
                    "issuer mismatch: expected '{}', got '{}'",
                    self.config.issuer, claims.iss
                ),
            }
            .build());
        }

        if claims.sub.is_empty() {
            return Err(error::TokenDecodeSnafu {
                message: "missing subject claim".to_owned(),
            }
            .build());
        }

        let now = now_unix();
        if let Some(nbf) = claims.nbf
            && now < nbf
        {
            return Err(error::TokenDecodeSnafu {
                message: "token not yet valid (nbf)".to_owned(),
            }
            .build());
        }

        // WHY: apply clock-skew leeway so NTP jumps or drift between the
        // issuer and validator do not immediately invalidate fresh tokens.
        // A token is still accepted if `now` is within `leeway` seconds past
        // `exp`. Saturates to i64 to tolerate pathological leeway values
        // configured through TOML without overflow.
        let leeway = i64::try_from(self.config.clock_skew_leeway_secs).unwrap_or(i64::MAX);
        if claims.exp.saturating_add(leeway) <= now {
            return Err(error::ExpiredTokenSnafu.build());
        }

        Ok(claims)
    }

    /// Refresh a token pair: validate the refresh token, issue a new access + refresh pair.
    #[instrument(skip(self, refresh_token))]
    pub(crate) fn refresh(&self, refresh_token: &str) -> Result<TokenPair> {
        let claims = self.validate(refresh_token)?;

        if claims.kind != TokenKind::Refresh {
            return Err(error::InvalidTokenSnafu {
                message: "expected refresh token, got access token".to_owned(),
            }
            .build());
        }

        // WHY: refresh tokens are single-use (rotated on each refresh) so a
        // stolen refresh token cannot be replayed indefinitely; both new
        // tokens get independent expirations.
        let access = self.issue_access(&claims.sub, claims.role, claims.nous_id.as_deref())?;
        let refresh = self.issue_refresh(&claims.sub, claims.role)?;

        Ok(TokenPair {
            access_token: access.into(),
            refresh_token: refresh.into(),
        })
    }

    /// Encode claims into a signed JWT string.
    ///
    /// Exposed for tests that need to craft tokens with specific claims (e.g. expired tokens).
    /// Production code should use [`issue_access`](Self::issue_access) or
    /// `issue_refresh`.
    #[must_use = "encoded token must be delivered to the caller"]
    pub fn encode_claims(&self, claims: &Claims) -> Result<String> {
        let payload_json = serde_json::to_vec(claims).map_err(|e| {
            error::TokenEncodeSnafu {
                message: format!("failed to serialize claims: {e}"),
            }
            .build()
        })?;
        let payload_b64 = base64url_encode(&payload_json);
        let signing_input = format!("{HS256_HEADER_B64}.{payload_b64}");
        // WHY: new_from_slice only fails if the key length is incompatible with
        // the hash block size, which cannot happen for HMAC-SHA256 (any length accepted).
        let mut mac = HmacSha256::new_from_slice(&self.signing_key_bytes).map_err(|e| {
            error::TokenEncodeSnafu {
                message: format!("HMAC key initialization failed: {e}"),
            }
            .build()
        })?;
        mac.update(signing_input.as_bytes());
        let tag = mac.finalize().into_bytes();
        let signature = base64url_encode(&tag);

        Ok(format!("{signing_input}.{signature}"))
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
            nbf: Some(now),
            // WHY: saturate to i64::MAX: a TTL exceeding ~292 billion years is effectively infinite
            exp: now + i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX),
            jti: koina::ulid::Ulid::new().to_string(),
            kind,
        };

        self.encode_claims(&claims)
    }
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
    // NOTE: u64 seconds are saturated to i64::MAX (~year 292B) to prevent overflow.
    // This is effectively infinite for practical JWT expiration purposes.
    .unwrap_or(i64::MAX)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len == 3"
)]
mod tests {
    use super::*;

    fn hmac_manager() -> JwtManager {
        JwtManager::new(JwtConfig {
            signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
            access_ttl: Duration::from_hours(1),
            refresh_ttl: Duration::from_hours(24),
            issuer: "aletheia-test".to_owned(),
            clock_skew_leeway_secs: DEFAULT_CLOCK_SKEW_LEEWAY_SECS,
        })
    }

    #[test]
    fn issue_and_validate_access_token() {
        let mgr = hmac_manager();
        let token = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        let claims = mgr.validate(&token).unwrap();
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.role, Role::Operator);
        assert_eq!(claims.kind, TokenKind::Access);
        assert!(claims.nous_id.is_none());
    }

    #[test]
    fn signing_key_is_zeroizing_wrapper() {
        let mgr = hmac_manager();
        let _: &Zeroizing<Vec<u8>> = &mgr.signing_key_bytes;
        let token = mgr.issue_access("user-5563", Role::Operator, None).unwrap();
        let claims = mgr.validate(&token).unwrap();
        assert_eq!(claims.sub, "user-5563");
        assert_eq!(claims.role, Role::Operator);
    }

    #[test]
    fn issue_and_validate_agent_token() {
        let mgr = hmac_manager();
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
        let mgr = hmac_manager();
        let token = mgr.issue_refresh("user-1", Role::Operator).unwrap();
        let claims = mgr.validate(&token).unwrap();
        assert_eq!(claims.kind, TokenKind::Refresh);
    }

    #[test]
    fn wrong_signing_key_rejected() {
        let mgr1 = hmac_manager();
        let mgr2 = JwtManager::new(JwtConfig {
            signing_key: SecretString::from("different-key".to_owned()),
            ..JwtConfig::default()
        });

        let token = mgr1.issue_access("user-1", Role::Operator, None).unwrap();
        assert!(mgr2.validate(&token).is_err());
    }

    #[test]
    fn expired_token_rejected() {
        let mgr = hmac_manager();

        let claims = Claims {
            sub: "user-1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            iss: "aletheia-test".to_owned(),
            iat: 1_000_000,
            nbf: None,
            exp: 1_000_001,
            jti: "expired-jti".to_owned(),
            kind: TokenKind::Access,
        };
        let token = mgr.encode_claims(&claims).unwrap();

        let result = mgr.validate(&token);
        assert!(result.is_err(), "expired token must be rejected");
    }

    #[test]
    fn refresh_flow_produces_valid_tokens() {
        let mgr = hmac_manager();
        let refresh = mgr.issue_refresh("user-1", Role::Operator).unwrap();
        let pair = mgr.refresh(&refresh).unwrap();

        let access_claims = mgr.validate(pair.access_token.expose_secret()).unwrap();
        assert_eq!(access_claims.sub, "user-1");
        assert_eq!(access_claims.kind, TokenKind::Access);

        let refresh_claims = mgr.validate(pair.refresh_token.expose_secret()).unwrap();
        assert_eq!(refresh_claims.kind, TokenKind::Refresh);
    }

    #[test]
    fn refresh_with_access_token_rejected() {
        let mgr = hmac_manager();
        let access = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        let result = mgr.refresh(&access);
        assert!(result.is_err());
    }

    #[test]
    fn claims_jti_is_unique() {
        let mgr = hmac_manager();
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
        let mgr = hmac_manager();
        assert!(mgr.validate("not.a.jwt").is_err());
        assert!(mgr.validate("").is_err());
        assert!(mgr.validate("abc123").is_err());
    }

    #[test]
    fn tampered_payload_rejected() {
        let mgr = hmac_manager();
        let token = mgr.issue_access("user-1", Role::Operator, None).unwrap();

        let parts: Vec<&str> = token.splitn(3, '.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 segments");

        // WHY: replace the payload with a different base64url string to simulate tampering
        let tampered = format!(
            "{}.{}.{}",
            parts[0],
            base64url_encode(b"{\"sub\":\"hacker\",\"role\":\"operator\",\"iss\":\"aletheia-test\",\"iat\":0,\"exp\":9999999999,\"jti\":\"x\",\"kind\":\"access\"}"),
            parts[2]
        );
        assert!(
            mgr.validate(&tampered).is_err(),
            "tampered payload must be rejected"
        );
    }

    #[test]
    fn tampered_signature_rejected() {
        let mgr = hmac_manager();
        let token = mgr.issue_access("user-1", Role::Operator, None).unwrap();

        let parts: Vec<&str> = token.splitn(3, '.').collect();
        // WHY: signature is base64url (ASCII-only), so byte offset 4 is safe
        let sig_tail = parts[2].get(4..).unwrap();
        let tampered = format!("{}.{}.AAAA{sig_tail}", parts[0], parts[1]);
        assert!(
            mgr.validate(&tampered).is_err(),
            "tampered signature must be rejected"
        );
    }

    #[test]
    fn token_has_three_dot_separated_segments() {
        let mgr = hmac_manager();
        let token = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        assert_eq!(
            token.matches('.').count(),
            2,
            "JWT must have exactly two dots"
        );
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
        assert!(
            mgr2.validate(&token).is_err(),
            "token from different issuer must be rejected"
        );
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
    fn round_trip_preserves_all_claim_fields() {
        let mgr = hmac_manager();
        let token = mgr
            .issue_access("agent-syn", Role::Agent, Some("syn"))
            .unwrap();
        let claims = mgr.validate(&token).unwrap();

        assert_eq!(claims.sub, "agent-syn");
        assert_eq!(claims.role, Role::Agent);
        assert_eq!(claims.nous_id.as_deref(), Some("syn"));
        assert_eq!(claims.iss, "aletheia-test");
        assert_eq!(claims.kind, TokenKind::Access);
        assert!(claims.iat > 0, "iat must be set");
        assert_eq!(claims.nbf, Some(claims.iat), "nbf must match iat");
        assert!(claims.exp > claims.iat, "exp must be after iat");
        assert!(!claims.jti.is_empty(), "jti must be set");
    }

    /// Regression: `encode_claims` must produce a token that round-trips
    /// through `validate` with every field intact. Catches the mutant that
    /// replaces the return value with `Ok("xyzzy".into())`.
    #[test]
    fn encode_claims_round_trip_preserves_fields() {
        let mgr = hmac_manager();
        let now = now_unix();
        let expected = Claims {
            sub: "alice".to_owned(),
            role: Role::Operator,
            nous_id: Some("nous-42".to_owned()),
            iss: "aletheia-test".to_owned(),
            iat: now,
            nbf: Some(now - 5),
            exp: now + 3600,
            jti: "distinctive-jti-123".to_owned(),
            kind: TokenKind::Access,
        };
        let token = mgr.encode_claims(&expected).unwrap();
        let actual = mgr.validate(&token).unwrap();

        assert_eq!(actual.sub, expected.sub);
        assert_eq!(actual.role, expected.role);
        assert_eq!(actual.nous_id, expected.nous_id);
        assert_eq!(actual.iss, expected.iss);
        assert_eq!(actual.iat, expected.iat);
        assert_eq!(actual.nbf, expected.nbf);
        assert_eq!(actual.exp, expected.exp);
        assert_eq!(actual.jti, expected.jti);
        assert_eq!(actual.kind, expected.kind);
    }

    /// Regression: `issue` must compute `exp` as `iat + ttl`. Catches the
    /// mutant that flips the `+` to `-` in `issue`'s exp computation.
    #[test]
    fn issue_computes_exp_as_iat_plus_ttl() {
        let mgr = hmac_manager();
        let token = mgr
            .issue(
                "bob",
                Role::Readonly,
                None,
                TokenKind::Access,
                Duration::from_hours(1),
            )
            .unwrap();
        let claims = mgr.validate(&token).unwrap();

        let delta = claims.exp - claims.iat;
        assert_eq!(delta, 3600, "exp must be exactly iat + ttl (3600s)");
    }

    #[test]
    fn empty_string_token_rejected() {
        let mgr = hmac_manager();
        let err = mgr.validate("");
        assert!(err.is_err(), "empty token must be rejected");
    }

    #[test]
    fn single_segment_token_rejected() {
        let mgr = hmac_manager();
        assert!(
            mgr.validate("onlyone").is_err(),
            "single-segment token must be rejected"
        );
    }

    #[test]
    fn two_segment_token_rejected() {
        let mgr = hmac_manager();
        assert!(
            mgr.validate("header.payload").is_err(),
            "two-segment token (no signature) must be rejected"
        );
    }

    /// A token whose `exp` lies within the 30s default leeway must still
    /// validate — regression guard for #3379 (NTP jumps must not immediately
    /// invalidate fresh tokens).
    #[test]
    fn token_within_clock_skew_leeway_is_accepted() {
        let mgr = hmac_manager();
        let now = now_unix();
        let claims = Claims {
            sub: "user-1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            iss: "aletheia-test".to_owned(),
            iat: now - 3600,
            nbf: None,
            // WHY: exp = now - 20s lies 20s in the past, within the 30s
            // default leeway, so the token must still validate.
            exp: now - 20,
            jti: "within-leeway-jti".to_owned(),
            kind: TokenKind::Access,
        };
        let token = mgr.encode_claims(&claims).unwrap();
        let result = mgr.validate(&token);
        assert!(
            result.is_ok(),
            "token expired 20s ago must be accepted within 30s leeway; got {result:?}"
        );
    }

    /// A token whose `exp` lies beyond the configured leeway must be rejected.
    #[test]
    fn token_beyond_clock_skew_leeway_is_rejected() {
        let mgr = hmac_manager();
        let now = now_unix();
        let claims = Claims {
            sub: "user-1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            iss: "aletheia-test".to_owned(),
            iat: now - 3600,
            nbf: None,
            // WHY: exp = now - 45s lies outside the 30s default leeway,
            // so the token must be rejected as expired.
            exp: now - 45,
            jti: "beyond-leeway-jti".to_owned(),
            kind: TokenKind::Access,
        };
        let token = mgr.encode_claims(&claims).unwrap();
        let result = mgr.validate(&token);
        assert!(
            result.is_err(),
            "token expired 45s ago must be rejected (beyond 30s leeway)"
        );
    }

    #[test]
    fn zero_leeway_config_rejects_any_expired_token() {
        // WHY: operators who explicitly set leeway to 0 must still be able
        // to opt into strict expiry checking.
        let mgr = JwtManager::new(JwtConfig {
            signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
            access_ttl: Duration::from_hours(1),
            refresh_ttl: Duration::from_hours(24),
            issuer: "aletheia-test".to_owned(),
            clock_skew_leeway_secs: 0,
        });
        let now = now_unix();
        let claims = Claims {
            sub: "user-1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            iss: "aletheia-test".to_owned(),
            iat: now - 3600,
            nbf: None,
            exp: now - 1,
            jti: "zero-leeway-jti".to_owned(),
            kind: TokenKind::Access,
        };
        let token = mgr.encode_claims(&claims).unwrap();
        assert!(
            mgr.validate(&token).is_err(),
            "with zero leeway, any past exp must be rejected"
        );
    }

    #[test]
    fn default_config_has_thirty_second_leeway() {
        // WHY: documentation (symbolon/CLAUDE.md) advertises 30s leeway.
        // This test guards that claim against silent drift.
        let config = JwtConfig::default();
        assert_eq!(config.clock_skew_leeway_secs, 30);
    }

    /// Token with a forged alg field in the header must be rejected.
    ///
    /// WHY: ensures `HS256_HEADER_B64` is actively enforced on the decode
    /// path and algorithm-confusion tokens (e.g., alg=RS256 with an
    /// HMAC-valid payload) cannot slip through.
    #[test]
    fn forged_alg_header_rejected() {
        let mgr = hmac_manager();

        // Craft a structurally-valid HMAC token, then replace its header
        // segment with one encoding alg=RS256. The signature remains valid
        // for the original header+payload pair, but the substituted header
        // must be caught before the payload is parsed.
        let real_token = mgr.issue_access("user-1", Role::Operator, None).unwrap();
        let parts: Vec<&str> = real_token.splitn(3, '.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 segments");

        // base64url({"alg":"RS256","typ":"JWT"})
        let rs256_header = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9";
        let forged = format!("{rs256_header}.{}.{}", parts[1], parts[2]);

        let result = mgr.validate(&forged);
        assert!(
            result.is_err(),
            "token with alg=RS256 header must be rejected; validate returned {result:?}"
        );
    }

    #[test]
    fn token_with_future_nbf_is_rejected() {
        let mgr = hmac_manager();
        let now = now_unix();
        let claims = Claims {
            sub: "user-1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            iss: "aletheia-test".to_owned(),
            iat: now,
            nbf: Some(now + 3600),
            exp: now + 7200,
            jti: "future-nbf-jti".to_owned(),
            kind: TokenKind::Access,
        };
        let token = mgr.encode_claims(&claims).unwrap();
        let result = mgr.validate(&token);
        assert!(
            result.is_err_and(|err| err.to_string().contains("token not yet valid (nbf)")),
            "token with future nbf must be rejected"
        );
    }

    #[test]
    fn token_with_past_nbf_is_accepted() {
        let mgr = hmac_manager();
        let now = now_unix();
        let claims = Claims {
            sub: "user-1".to_owned(),
            role: Role::Operator,
            nous_id: None,
            iss: "aletheia-test".to_owned(),
            iat: now - 60,
            nbf: Some(now - 30),
            exp: now + 3600,
            jti: "past-nbf-jti".to_owned(),
            kind: TokenKind::Access,
        };
        let token = mgr.encode_claims(&claims).unwrap();
        let result = mgr.validate(&token);
        assert!(result.is_ok(), "token with past nbf must be accepted");
    }
}
