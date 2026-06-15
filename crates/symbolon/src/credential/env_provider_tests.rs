#![expect(clippy::expect_used, reason = "test assertions")]

//! Tests for `EnvCredentialProvider` and its interaction with OAuth JWT expiry
//! semantics. Includes the chain fall-through case that exercises expired-env
//! behavior against a file fallback.

use koina::credential::{CredentialProvider, CredentialSource};
use koina::secret::SecretString;

use super::*;
use crate::util::decode_jwt_exp_secs;

// NOTE: env tests use a guaranteed-absent var name to avoid depending on CI/dev env vars

#[test]
fn env_provider_missing_returns_none() {
    let provider = EnvCredentialProvider::new("ALETHEIA_TEST_NONEXISTENT_49_XYZ");
    assert!(provider.get_credential().is_none());
}

#[test]
fn env_provider_name() {
    let provider = EnvCredentialProvider::new("MY_VAR");
    assert_eq!(provider.name(), "MY_VAR");
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_undecodable_oauth_prefix_returns_none() {
    let var = "ALETHEIA_TEST_OAUTH_PREFIX_748";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, "sk-ant-oat-test-token-value") };
    let provider = EnvCredentialProvider::new(var);
    assert!(
        provider.get_credential().is_none(),
        "undecodable OAuth token should fall through"
    );
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_api_key_stays_environment() {
    let var = "ALETHEIA_TEST_APIKEY_748";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, "sk-ant-api-test-key") };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider
        .get_credential()
        .expect("api key token should yield credential");
    assert_eq!(cred.source, CredentialSource::Environment);
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_with_source_forces_oauth() {
    let var = "ALETHEIA_TEST_FORCE_OAUTH_748";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, "any-token-value") };
    let provider = EnvCredentialProvider::with_source(var, CredentialSource::OAuth);
    let cred = provider
        .get_credential()
        .expect("forced-oauth token should yield credential");
    assert_eq!(cred.source, CredentialSource::OAuth);
    unsafe { std::env::remove_var(var) };
}

/// Build a synthetic token with the OAuth prefix and a dot-segmented payload
/// carrying the given exp (seconds since epoch). Prefix satisfies
/// `starts_with(OAUTH_TOKEN_PREFIX)`; payload carries the exp claim; stub
/// signature is unused (no verification is performed).
fn make_test_oauth_token(exp_secs: u64) -> String {
    fn base64url_encode(input: &[u8]) -> String {
        koina::base64::encode_url_safe_no_pad(input)
    }

    let payload_json = format!(r#"{{"exp":{exp_secs}}}"#);
    let payload_b64 = base64url_encode(payload_json.as_bytes());
    format!("sk-ant-oat.{payload_b64}.stub")
}

#[test]
fn decode_jwt_exp_roundtrips_known_value() {
    let token = make_test_oauth_token(42_000);
    let exp = decode_jwt_exp_secs(&token);
    assert_eq!(exp, Some(42_000));
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_expired_oauth_falls_through() {
    let var = "ALETHEIA_TEST_EXPIRED_OAUTH_505";
    let expired_token = make_test_oauth_token(1); // epoch 1 is always in the past
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, &expired_token) };
    let provider = EnvCredentialProvider::new(var);
    assert!(
        provider.get_credential().is_none(),
        "expired OAuth token should cause fallthrough"
    );
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_within_skew_window_accepted() {
    let var = "ALETHEIA_TEST_SKEW_OAUTH_505";
    let now_secs = unix_epoch_ms() / 1000;
    let within_skew_token = make_test_oauth_token(now_secs - 10);
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, &within_skew_token) };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider.get_credential();
    assert!(
        cred.is_some(),
        "token within clock skew leeway should be accepted"
    );
    assert_eq!(
        cred.as_ref().map(|c| &c.source),
        Some(&CredentialSource::OAuth)
    );
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_beyond_skew_window_rejected() {
    let var = "ALETHEIA_TEST_BEYOND_SKEW_OAUTH_505";
    let now_secs = unix_epoch_ms() / 1000;
    let beyond_skew_token = make_test_oauth_token(now_secs - 60);
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, &beyond_skew_token) };
    let provider = EnvCredentialProvider::new(var);
    assert!(
        provider.get_credential().is_none(),
        "token beyond clock skew leeway should be rejected"
    );
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_valid_oauth_returns_credential() {
    let var = "ALETHEIA_TEST_VALID_OAUTH_505";
    let future_secs = unix_epoch_ms() / 1000 + 7200;
    let valid_token = make_test_oauth_token(future_secs);
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, &valid_token) };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider
        .get_credential()
        .expect("valid oauth token should yield credential");
    assert_eq!(cred.secret.expose_secret(), valid_token);
    assert_eq!(cred.source, CredentialSource::OAuth);
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_opaque_oauth_without_exp_returns_none() {
    let var = "ALETHEIA_TEST_OPAQUE_OAUTH_505";
    let opaque = "sk-ant-oat-opaque-no-dots";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, opaque) };
    let provider = EnvCredentialProvider::new(var);
    assert!(
        provider.get_credential().is_none(),
        "opaque OAuth token without decodable exp should fall through"
    );
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn chain_falls_through_expired_oauth_env_to_file_provider() {
    let var = "ALETHEIA_TEST_CHAIN_EXPIRED_505";
    let expired_token = make_test_oauth_token(1);
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, &expired_token) };

    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    let cred_file = CredentialFile {
        token: SecretString::from("sk-ant-api-file-fallback"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred_file
        .save(&path)
        .expect("save file fallback credential");

    let chain = CredentialChain::new(vec![
        Box::new(EnvCredentialProvider::new(var)),
        Box::new(FileCredentialProvider::new(path)),
    ]);

    let resolved = chain
        .get_credential()
        .expect("chain should resolve to file fallback");
    assert_eq!(
        resolved.secret.expose_secret(),
        "sk-ant-api-file-fallback",
        "chain should skip expired env token and use file provider"
    );

    unsafe { std::env::remove_var(var) };
}
