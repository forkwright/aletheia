//! Integration tests for `JwtManager` and `JwtConfig` public API.
//!
//! Exercises only the published API surface, the same way pylon and the
//! auth middleware consume it.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "JWT segments have a known 3-part structure asserted before indexing"
)]

use std::time::Duration;

use koina::secret::SecretString;
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::{Role, TokenKind};

/// Build a `JwtManager` with a test signing key and standard TTLs.
fn test_manager() -> JwtManager {
    JwtManager::new(JwtConfig {
        signing_key: SecretString::from("test-signing-key-for-integration-tests".to_owned()),
        access_ttl: Duration::from_hours(1),
        refresh_ttl: Duration::from_hours(168),
        issuer: "aletheia-test".to_owned(),
        ..JwtConfig::default()
    })
}

// --- JwtConfig ---

#[test]
fn default_config_uses_insecure_placeholder() {
    // WHY: Default JwtConfig uses an insecure placeholder key. The library
    // *must* allow this for testing/dev, but production paths must reject it.
    let config = JwtConfig::default();
    assert_eq!(config.access_ttl, Duration::from_hours(1));
    assert_eq!(config.refresh_ttl, Duration::from_hours(168));
    assert_eq!(config.issuer, "aletheia");
}

#[test]
fn validate_for_auth_mode_none_accepts_default_key() {
    // WHY: auth mode "none" disables JWT verification entirely, so the
    // placeholder key is harmless. validate_for_auth_mode must permit it.
    let config = JwtConfig::default();
    assert!(config.validate_for_auth_mode("none").is_ok());
}

#[test]
fn validate_for_auth_mode_jwt_rejects_default_key() {
    // WHY: production safety — never start an auth-enabled server with the
    // shipped placeholder key. The validation guards startup.
    let config = JwtConfig::default();
    let result = config.validate_for_auth_mode("jwt");
    assert!(
        result.is_err(),
        "default placeholder key must be rejected for non-none auth modes"
    );
}

#[test]
fn validate_for_auth_mode_jwt_accepts_real_key() {
    let config = JwtConfig {
        signing_key: SecretString::from("a-real-non-default-signing-key-12345".to_owned()),
        ..JwtConfig::default()
    };
    assert!(config.validate_for_auth_mode("jwt").is_ok());
}

#[test]
fn debug_does_not_leak_signing_key() {
    // WHY: Debug must redact the signing key. A leaked key in error logs is
    // an immediate compromise of every issued token.
    let config = JwtConfig {
        signing_key: SecretString::from("super-secret-key-must-not-leak".to_owned()),
        ..JwtConfig::default()
    };
    let dbg = format!("{config:?}");
    assert!(
        !dbg.contains("super-secret-key-must-not-leak"),
        "JwtConfig Debug must redact the signing key, got: {dbg}"
    );
    assert!(
        dbg.contains("REDACTED") || dbg.contains("[REDACTED]"),
        "Debug should explicitly mark redacted fields, got: {dbg}"
    );
}

// --- JwtManager: issue + validate round trip ---

#[test]
fn issue_access_then_validate_round_trip() {
    let mgr = test_manager();
    let token = mgr
        .issue_access("user-123", Role::Operator, None)
        .expect("issue access");
    let claims = mgr.validate(&token).expect("validate access");

    assert_eq!(claims.sub, "user-123");
    assert_eq!(claims.role, Role::Operator);
    assert!(claims.nous_id.is_none());
    assert_eq!(claims.iss, "aletheia-test");
    assert_eq!(claims.kind, TokenKind::Access);
    // jti must be non-empty (used for revocation)
    assert!(!claims.jti.is_empty());
}

#[test]
fn issue_refresh_then_validate_round_trip() {
    let mgr = test_manager();
    let token = mgr
        .issue_refresh("user-456", Role::Admin)
        .expect("issue refresh");
    let claims = mgr.validate(&token).expect("validate refresh");

    assert_eq!(claims.sub, "user-456");
    assert_eq!(claims.role, Role::Admin);
    assert_eq!(claims.kind, TokenKind::Refresh);
}

#[test]
fn agent_token_includes_nous_id_scope() {
    // WHY: Agent tokens must carry the nous_id they're scoped to so the
    // RBAC layer can deny cross-agent access. Round-trip the field.
    let mgr = test_manager();
    let token = mgr
        .issue_access("agent-syn", Role::Agent, Some("syn"))
        .expect("issue agent token");
    let claims = mgr.validate(&token).expect("validate");

    assert_eq!(claims.role, Role::Agent);
    assert_eq!(claims.nous_id.as_deref(), Some("syn"));
}

#[test]
fn issue_access_assigns_unique_jti_per_call() {
    // WHY: Each issued token must have a unique jti so the revocation
    // store can target individual tokens. Two consecutive issues for the
    // same subject must yield different jtis.
    let mgr = test_manager();
    let t1 = mgr
        .issue_access("user-1", Role::Operator, None)
        .expect("issue 1");
    let t2 = mgr
        .issue_access("user-1", Role::Operator, None)
        .expect("issue 2");
    let c1 = mgr.validate(&t1).expect("validate 1");
    let c2 = mgr.validate(&t2).expect("validate 2");
    assert_ne!(c1.jti, c2.jti, "consecutive issues must have unique jti");
}

// --- JwtManager: tampered tokens ---

#[test]
fn validate_rejects_tampered_signature() {
    // WHY: signature verification must fail if any byte in the signature
    // segment is altered.
    //
    // WHY deterministic byte-level tampering: a prior implementation flipped
    // the last base64url character of the signature segment. Because
    // base64url encodes 32 HMAC-SHA256 bytes into 43 chars (6 bits each =
    // 258 bits), the trailing char carries only 2 significant bits; the
    // other 4 bits are "don't-care" padding that permissive decoders ignore.
    // Flipping a char that only differs in those padding bits produced the
    // same decoded signature bytes, so verification succeeded and the test
    // flaked. Tracked in #3565.
    //
    // Fix: decode the signature to raw bytes, XOR a specific byte with 0xFF,
    // then re-encode. This guarantees the decoded MAC differs from the
    // correct MAC by exactly 8 bits, so `Mac::verify_slice` (constant-time)
    // will always reject it.
    let mgr = test_manager();
    let token = mgr
        .issue_access("user", Role::Operator, None)
        .expect("issue");

    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT has three segments");

    let mut sig_bytes =
        koina::base64::decode_url_safe_no_pad(parts[2]).expect("signature is valid base64url");
    assert!(!sig_bytes.is_empty(), "HMAC-SHA256 signature is non-empty");
    // WHY: XOR the first byte. Any byte would work; the first is chosen for
    // stability and clarity. Flipping all 8 bits of one byte cannot collide
    // with the original value.
    sig_bytes[0] ^= 0xFF;
    let tampered_sig = koina::base64::encode_url_safe_no_pad(&sig_bytes);
    let tampered = format!("{}.{}.{}", parts[0], parts[1], tampered_sig);

    // Sanity: the tampered token must differ from the original. If this
    // fails the tampering logic above is broken.
    assert_ne!(tampered, token, "tampered token must differ from original");

    let result = mgr.validate(&tampered);
    assert!(
        result.is_err(),
        "tampered signature must fail validation, got: {result:?}"
    );
}

#[test]
fn validate_rejects_tampered_payload() {
    // WHY: changing any byte in the payload (not just the signature) must
    // also fail because the HMAC covers header.payload.
    let mgr = test_manager();
    let token = mgr
        .issue_access("user", Role::Operator, None)
        .expect("issue");

    // Find the payload segment (between the two dots) and corrupt it.
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT has three segments");
    let mut payload: Vec<char> = parts[1].chars().collect();
    let original = payload[0];
    payload[0] = if original == 'e' { 'f' } else { 'e' };
    let new_payload: String = payload.into_iter().collect();
    let tampered = format!("{}.{}.{}", parts[0], new_payload, parts[2]);

    let result = mgr.validate(&tampered);
    assert!(
        result.is_err(),
        "tampered payload must fail validation, got: {result:?}"
    );
}

#[test]
fn validate_rejects_token_signed_with_different_key() {
    // WHY: cross-key tokens must be rejected. A token issued by a different
    // JwtManager (with a different signing key) is not valid.
    let mgr_a = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("key-A-issuer-of-the-token".to_owned()),
        access_ttl: Duration::from_hours(1),
        refresh_ttl: Duration::from_hours(24),
        issuer: "issuer-a".to_owned(),
        ..JwtConfig::default()
    });
    let mgr_b = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("key-B-different-validator".to_owned()),
        access_ttl: Duration::from_hours(1),
        refresh_ttl: Duration::from_hours(24),
        issuer: "issuer-b".to_owned(),
        ..JwtConfig::default()
    });

    let token = mgr_a
        .issue_access("user", Role::Operator, None)
        .expect("issue from A");
    let result = mgr_b.validate(&token);
    assert!(
        result.is_err(),
        "token signed by mgr_a must be rejected by mgr_b, got: {result:?}"
    );
}

#[test]
fn validate_rejects_malformed_token() {
    let mgr = test_manager();
    let result = mgr.validate("not.a.valid.jwt.format");
    assert!(result.is_err());

    let result = mgr.validate("only-one-segment");
    assert!(result.is_err());

    let result = mgr.validate("");
    assert!(result.is_err());
}

#[test]
fn validate_rejects_expired_token() {
    // WHY: expired tokens must be rejected. Issue a token with a 0-second
    // TTL and verify it fails immediately. Leeway is explicitly 0 so the
    // 30s default clock-skew tolerance does not keep the token alive past
    // the short sleep below.
    let mgr = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("key-for-expiry-test".to_owned()),
        access_ttl: Duration::from_secs(0),
        refresh_ttl: Duration::from_secs(0),
        issuer: "test".to_owned(),
        clock_skew_leeway_secs: 0,
    });
    let token = mgr
        .issue_access("user", Role::Operator, None)
        .expect("issue");

    // Sleep briefly to ensure jiff::Timestamp::now > exp.
    std::thread::sleep(Duration::from_millis(1100));

    let result = mgr.validate(&token);
    assert!(
        result.is_err(),
        "expired token must be rejected, got: {result:?}"
    );
}

// --- Role enum ---

#[test]
fn role_ordering_reflects_privilege_hierarchy() {
    // WHY: PartialOrd is used by RBAC checks. Readonly < Agent < Operator < Admin.
    assert!(Role::Readonly < Role::Agent);
    assert!(Role::Agent < Role::Operator);
    assert!(Role::Operator < Role::Admin);
}

#[test]
fn role_round_trips_through_from_str_and_display() {
    use std::str::FromStr;
    for role in [Role::Readonly, Role::Agent, Role::Operator, Role::Admin] {
        let s = role.to_string();
        let parsed = Role::from_str(&s).expect("round trip");
        assert_eq!(parsed, role);
    }
}

#[test]
fn role_from_str_rejects_unknown_value() {
    use std::str::FromStr;
    assert!(Role::from_str("superuser").is_err());
    assert!(Role::from_str("").is_err());
}
