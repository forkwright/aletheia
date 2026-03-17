#![expect(clippy::expect_used, reason = "test assertions use .expect() for descriptive panic messages")]
use super::*;

#[test]
fn credential_file_roundtrip() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join("test.json");

    let cred = CredentialFile {
        token: "sk-test-123".to_owned(),
        refresh_token: Some("rt-test-456".to_owned()),
        expires_at: Some(1_700_000_000_000),
        scopes: Some(vec!["user:inference".to_owned()]),
        subscription_type: Some("max".to_owned()),
    };
    cred.save(&path)
        .expect("credential file save should succeed");

    let loaded = CredentialFile::load(&path).expect("credential file load should succeed");
    assert_eq!(
        loaded.token, "sk-test-123",
        "loaded token should match saved token"
    );
    assert_eq!(
        loaded.refresh_token.as_deref(),
        Some("rt-test-456"),
        "loaded refresh token should match saved refresh token"
    );
    assert_eq!(
        loaded.expires_at,
        Some(1_700_000_000_000),
        "loaded expires_at should match saved value"
    );
}

#[test]
fn credential_file_missing_returns_none() {
    assert!(
        CredentialFile::load(Path::new("/nonexistent/path.json")).is_none(),
        "loading a missing file should return None"
    );
}

#[test]
fn credential_file_malformed_returns_none() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "not json").expect("writing malformed json file should succeed");
    assert!(
        CredentialFile::load(&path).is_none(),
        "loading a malformed JSON file should return None"
    );
}

// --- claudeAiOauth wrapper tests ---

#[test]
fn credential_file_load_claude_code_oauth_wrapper() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    std::fs::write(
        &path,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-wrapped","refreshToken":"rt-wrapped","expiresAt":9999999999000}}"#,
    )
    .expect("writing credential JSON file should succeed");

    let loaded = CredentialFile::load(&path)
        .expect("credential file load should succeed for claudeAiOauth wrapper");
    assert_eq!(
        loaded.token, "sk-ant-oat-wrapped",
        "token should be extracted from claudeAiOauth wrapper"
    );
    assert_eq!(
        loaded.refresh_token.as_deref(),
        Some("rt-wrapped"),
        "refresh token should be extracted from claudeAiOauth wrapper"
    );
    assert_eq!(
        loaded.expires_at,
        Some(9_999_999_999_000),
        "expires_at should be extracted from claudeAiOauth wrapper"
    );
    assert!(
        loaded.has_refresh_token(),
        "has_refresh_token should return true when refresh token is present"
    );
}

#[test]
fn credential_file_load_wrapped_no_refresh_token() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    std::fs::write(
        &path,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-no-rt"}}"#,
    )
    .expect("writing credential JSON file should succeed");

    let loaded = CredentialFile::load(&path)
        .expect("credential file load should succeed for wrapper without refresh token");
    assert_eq!(
        loaded.token, "sk-ant-oat-no-rt",
        "token should be extracted from claudeAiOauth wrapper"
    );
    assert!(
        !loaded.has_refresh_token(),
        "has_refresh_token should return false when refresh token is absent"
    );
}

#[test]
fn credential_file_load_flat_takes_precedence_over_wrapper() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    std::fs::write(
        &path,
        r#"{"token":"flat-token","claudeAiOauth":{"accessToken":"wrapped-token"}}"#,
    )
    .expect("writing credential JSON file should succeed");
    let loaded = CredentialFile::load(&path)
        .expect("credential file load should succeed when both flat and wrapper formats present");
    assert_eq!(
        loaded.token, "flat-token",
        "flat token field should take precedence over claudeAiOauth wrapper"
    );
}

#[test]
fn credential_file_load_wrapper_missing_key_returns_none() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    std::fs::write(&path, r#"{"someOtherKey":{"value":1}}"#)
        .expect("writing credential JSON file should succeed");
    assert!(
        CredentialFile::load(&path).is_none(),
        "loading a JSON file with missing required keys should return None"
    );
}

#[test]
fn has_refresh_token() {
    let with = CredentialFile {
        token: "t".to_owned(),
        refresh_token: Some("rt".to_owned()),
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    assert!(
        with.has_refresh_token(),
        "has_refresh_token should return true when refresh token is present"
    );

    let without = CredentialFile {
        refresh_token: None,
        ..with.clone()
    };
    assert!(
        !without.has_refresh_token(),
        "has_refresh_token should return false when refresh token is None"
    );

    let empty = CredentialFile {
        refresh_token: Some(String::new()),
        ..without
    };
    assert!(
        !empty.has_refresh_token(),
        "has_refresh_token should return false when refresh token is an empty string"
    );
}

// NOTE: env tests use a guaranteed-absent var name to avoid depending on CI/dev env vars

#[test]
fn env_provider_missing_returns_none() {
    let provider = EnvCredentialProvider::new("ALETHEIA_TEST_NONEXISTENT_49_XYZ");
    assert!(
        provider.get_credential().is_none(),
        "env provider should return None when environment variable is missing"
    );
}

#[test]
fn env_provider_name() {
    let provider = EnvCredentialProvider::new("MY_VAR");
    assert_eq!(
        provider.name(),
        "MY_VAR",
        "provider name should match the variable name it was created with"
    );
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_detects_oauth_by_prefix() {
    let var = "ALETHEIA_TEST_OAUTH_PREFIX_748";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, "sk-ant-oat-test-token-value") };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider
        .get_credential()
        .expect("env provider should return credential for set variable");
    assert_eq!(
        cred.source,
        CredentialSource::OAuth,
        "credential source should be OAuth for token with OAuth prefix"
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
        .expect("env provider should return credential for set variable");
    assert_eq!(
        cred.source,
        CredentialSource::Environment,
        "credential source should be Environment for non-OAuth API key"
    );
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
        .expect("env provider should return credential for set variable");
    assert_eq!(
        cred.source,
        CredentialSource::OAuth,
        "credential source should be OAuth when forced via with_source"
    );
    unsafe { std::env::remove_var(var) };
}

// --- expired OAuth env var fallthrough tests ---

/// Build a synthetic token with the OAuth prefix and a dot-segmented payload
/// carrying the given exp (seconds since epoch). Prefix satisfies
/// `starts_with(OAUTH_TOKEN_PREFIX)`; payload carries the exp claim; stub
/// signature is unused (no verification is performed).
fn make_test_oauth_token(exp_secs: u64) -> String {
    fn base64url_encode(input: &[u8]) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let mut buf = [0u8; 3];
            for (i, &b) in chunk.iter().enumerate() {
                buf[i] = b;
            }
            let n = (u32::from(buf[0]) << 16) | (u32::from(buf[1]) << 8) | u32::from(buf[2]);
            out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
            }
            if chunk.len() > 2 {
                out.push(TABLE[(n & 0x3F) as usize] as char);
            }
        }
        out
    }

    let payload_json = format!(r#"{{"exp":{exp_secs}}}"#);
    let payload_b64 = base64url_encode(payload_json.as_bytes());
    format!("sk-ant-oat.{payload_b64}.stub")
}

#[test]
fn decode_jwt_exp_roundtrips_known_value() {
    let token = make_test_oauth_token(42_000);
    let exp = decode_jwt_exp_secs(&token);
    assert_eq!(
        exp,
        Some(42_000),
        "decoded JWT exp should match the value encoded in the token"
    );
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
    // Set exp to 10 seconds in the past: within the 30-second leeway window
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
        Some(&CredentialSource::OAuth),
        "credential source should be OAuth for token within clock skew leeway"
    );
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_beyond_skew_window_rejected() {
    let var = "ALETHEIA_TEST_BEYOND_SKEW_OAUTH_505";
    // Set exp to 60 seconds in the past: beyond the 30-second leeway
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
        .expect("env provider should return credential for valid non-expired OAuth token");
    assert_eq!(
        cred.secret, valid_token,
        "credential secret should match the token set in the env var"
    );
    assert_eq!(
        cred.source,
        CredentialSource::OAuth,
        "credential source should be OAuth for token with OAuth prefix"
    );
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_opaque_oauth_without_exp_is_returned() {
    // Opaque token with OAuth prefix but no parseable exp must not be dropped.
    let var = "ALETHEIA_TEST_OPAQUE_OAUTH_505";
    let opaque = "sk-ant-oat-opaque-no-dots";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, opaque) };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider
        .get_credential()
        .expect("env provider should return credential for opaque OAuth token without exp");
    assert_eq!(
        cred.secret, opaque,
        "credential secret should match the opaque token set in the env var"
    );
    assert_eq!(
        cred.source,
        CredentialSource::OAuth,
        "credential source should be OAuth for token with OAuth prefix"
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

    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    let cred_file = CredentialFile {
        token: "sk-ant-api-file-fallback".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred_file
        .save(&path)
        .expect("credential file save should succeed");

    let chain = CredentialChain::new(vec![
        Box::new(EnvCredentialProvider::new(var)),
        Box::new(FileCredentialProvider::new(path)),
    ]);

    let resolved = chain
        .get_credential()
        .expect("chain should resolve to file provider after skipping expired env token");
    assert_eq!(
        resolved.secret, "sk-ant-api-file-fallback",
        "chain should skip expired env token and use file provider"
    );

    unsafe { std::env::remove_var(var) };
}

#[test]
fn file_provider_reads_token() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join("anthropic.json");
    let cred = CredentialFile {
        token: "sk-file-token".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path)
        .expect("credential file save should succeed");

    let provider = FileCredentialProvider::new(path);
    let result = provider
        .get_credential()
        .expect("file provider should return credential for existing file");
    assert_eq!(
        result.secret, "sk-file-token",
        "credential secret should match token stored in file"
    );
    assert_eq!(
        result.source,
        CredentialSource::File,
        "credential source should be File for file-based provider"
    );
}

#[test]
fn file_provider_missing_file_returns_none() {
    let provider = FileCredentialProvider::new(PathBuf::from("/nonexistent/cred.json"));
    assert!(
        provider.get_credential().is_none(),
        "file provider should return None when credential file does not exist"
    );
}

#[test]
fn file_provider_detects_file_change() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join("anthropic.json");

    let cred1 = CredentialFile {
        token: "token-v1".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred1
        .save(&path)
        .expect("credential file save should succeed");

    let provider = FileCredentialProvider::new(path.clone());
    let r1 = provider
        .get_credential()
        .expect("file provider should return credential for existing file");
    assert_eq!(
        r1.secret, "token-v1",
        "initial credential secret should match token-v1"
    );

    if let Ok(mut guard) = provider.cached.write()
        && let Some(ref mut c) = *guard
    {
        c.checked_at = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .unwrap_or(Instant::now());
        c.mtime = SystemTime::UNIX_EPOCH;
    }

    let cred2 = CredentialFile {
        token: "token-v2".to_owned(),
        ..cred1
    };
    cred2
        .save(&path)
        .expect("updated credential file save should succeed");

    let r2 = provider
        .get_credential()
        .expect("file provider should return updated credential after file change");
    assert_eq!(
        r2.secret, "token-v2",
        "credential secret should reflect updated token-v2 after file change"
    );
}

struct StaticProvider {
    token: Option<String>,
    name: &'static str,
}

impl CredentialProvider for StaticProvider {
    fn get_credential(&self) -> Option<Credential> {
        self.token.as_ref().map(|t| Credential {
            secret: t.clone(),
            source: CredentialSource::Environment,
        })
    }
    fn name(&self) -> &str {
        self.name
    }
}

#[test]
fn chain_first_wins() {
    let chain = CredentialChain::new(vec![
        Box::new(StaticProvider {
            token: Some("first".to_owned()),
            name: "a",
        }),
        Box::new(StaticProvider {
            token: Some("second".to_owned()),
            name: "b",
        }),
    ]);
    let cred = chain
        .get_credential()
        .expect("chain should return credential from first provider");
    assert_eq!(
        cred.secret, "first",
        "chain should return the first provider's credential when both providers have tokens"
    );
}

#[test]
fn chain_skips_empty() {
    let chain = CredentialChain::new(vec![
        Box::new(StaticProvider {
            token: None,
            name: "empty",
        }),
        Box::new(StaticProvider {
            token: Some("fallback".to_owned()),
            name: "fb",
        }),
    ]);
    let cred = chain
        .get_credential()
        .expect("chain should return credential from fallback provider");
    assert_eq!(
        cred.secret, "fallback",
        "chain should skip empty provider and return fallback credential"
    );
}

#[test]
fn chain_all_empty_returns_none() {
    let chain = CredentialChain::new(vec![
        Box::new(StaticProvider {
            token: None,
            name: "a",
        }),
        Box::new(StaticProvider {
            token: None,
            name: "b",
        }),
    ]);
    assert!(
        chain.get_credential().is_none(),
        "chain should return None when all providers return None"
    );
}

#[test]
fn chain_empty_providers_returns_none() {
    let chain = CredentialChain::new(vec![]);
    assert!(
        chain.get_credential().is_none(),
        "chain with no providers should return None"
    );
}

#[test]
fn claude_code_default_path_uses_home() {
    // NOTE: depends on $HOME being set: typical in CI and dev
    if let Some(path) = claude_code_default_path() {
        assert!(
            path.ends_with(".claude/.credentials.json"),
            "default Claude Code credential path should end with .claude/.credentials.json"
        );
    }
}

#[test]
fn claude_code_provider_missing_file_returns_none() {
    let result = claude_code_provider(Path::new("/nonexistent/.credentials.json"));
    assert!(
        result.is_none(),
        "claude_code_provider should return None when credential file does not exist"
    );
}

#[test]
fn claude_code_provider_static_token() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    let cred = CredentialFile {
        token: "sk-ant-api-static".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path)
        .expect("credential file save should succeed");

    let provider = claude_code_provider(&path).expect("should return provider");
    let resolved = provider
        .get_credential()
        .expect("claude_code_provider should return credential for static token file");
    assert_eq!(
        resolved.secret, "sk-ant-api-static",
        "resolved secret should match static API token"
    );
    assert_eq!(
        resolved.source,
        CredentialSource::File,
        "credential source should be File for static token"
    );
}

#[tokio::test]
async fn claude_code_provider_with_access_token_alias() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    std::fs::write(
        &path,
        r#"{"accessToken": "sk-ant-oat-cc-token", "refreshToken": "rt-cc"}"#,
    )
    .expect("writing credential JSON file should succeed");

    let provider = claude_code_provider(&path).expect("should return provider");
    let resolved = provider
        .get_credential()
        .expect("claude_code_provider should return credential for accessToken alias format");
    assert_eq!(
        resolved.secret, "sk-ant-oat-cc-token",
        "resolved secret should match accessToken alias"
    );
    assert_eq!(
        resolved.source,
        CredentialSource::OAuth,
        "credential source should be OAuth for accessToken alias"
    );
}

#[tokio::test]
async fn claude_code_provider_with_claude_code_oauth_wrapper() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    std::fs::write(
        &path,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-wrapped","refreshToken":"rt-wrapped"}}"#,
    )
    .expect("writing credential JSON file should succeed");

    let provider = claude_code_provider(&path).expect("should return provider for wrapped format");
    let resolved = provider
        .get_credential()
        .expect("claude_code_provider should return credential for claudeAiOauth wrapped format");
    assert_eq!(
        resolved.secret, "sk-ant-oat-wrapped",
        "resolved secret should match accessToken from claudeAiOauth wrapper"
    );
    assert_eq!(
        resolved.source,
        CredentialSource::OAuth,
        "credential source should be OAuth for wrapped OAuth format"
    );
}

#[test]
fn claude_code_provider_malformed_returns_none() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    std::fs::write(&path, "not valid json").expect("writing malformed json file should succeed");
    assert!(
        claude_code_provider(&path).is_none(),
        "claude_code_provider should return None for malformed JSON file"
    );
}

#[test]
fn unix_epoch_ms_returns_nonzero() {
    let ms = unix_epoch_ms();
    assert!(
        ms > 1_000_000_000_000,
        "expected modern timestamp in ms, got {ms}"
    );
}

#[test]
fn seconds_remaining_none_when_no_expiry() {
    let cred = CredentialFile {
        token: "t".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    assert!(
        cred.seconds_remaining().is_none(),
        "seconds_remaining should return None when expires_at is not set"
    );
}

#[test]
fn seconds_remaining_negative_when_expired() {
    let cred = CredentialFile {
        token: "t".to_owned(),
        refresh_token: None,
        expires_at: Some(1_000_000_000_000),
        scopes: None,
        subscription_type: None,
    };
    let remaining = cred
        .seconds_remaining()
        .expect("seconds_remaining should return Some for credential with expires_at set");
    assert!(
        remaining < 0,
        "expected negative remaining, got {remaining}"
    );
}

#[test]
fn seconds_remaining_positive_for_future_expiry() {
    let far_future_ms = unix_epoch_ms() + 3_600_000;
    let cred = CredentialFile {
        token: "t".to_owned(),
        refresh_token: None,
        expires_at: Some(far_future_ms),
        scopes: None,
        subscription_type: None,
    };
    let remaining = cred
        .seconds_remaining()
        .expect("seconds_remaining should return Some for credential with future expires_at");
    assert!(
        remaining > 0 && remaining <= 3600,
        "expected ~3600s remaining, got {remaining}"
    );
}

// --- Integration: credential file refresh cycle ---

#[tokio::test]
async fn refreshing_provider_reads_credential_file_and_provides_token() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    let far_future_ms = unix_epoch_ms() + 7_200_000;
    let cred = CredentialFile {
        token: "sk-ant-oat-initial-token".to_owned(),
        refresh_token: Some("rt-test-refresh".to_owned()),
        expires_at: Some(far_future_ms),
        scopes: Some(vec!["user:inference".to_owned()]),
        subscription_type: Some("max".to_owned()),
    };
    cred.save(&path)
        .expect("credential file save should succeed");

    let provider = RefreshingCredentialProvider::new(path.clone()).expect("should create provider");
    let resolved = provider
        .get_credential()
        .expect("refreshing provider should return credential for valid credential file");
    assert_eq!(
        resolved.secret, "sk-ant-oat-initial-token",
        "resolved secret should match initial token"
    );
    assert_eq!(
        resolved.source,
        CredentialSource::OAuth,
        "credential source should be OAuth for OAuth token"
    );

    // Shut down background task to avoid leaking
    provider.shutdown();
}

#[tokio::test]
async fn refresh_write_back_preserves_subscription_type() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    let cred = CredentialFile {
        token: "sk-ant-oat-original".to_owned(),
        refresh_token: Some("rt-original".to_owned()),
        expires_at: Some(unix_epoch_ms() + 7_200_000),
        scopes: Some(vec!["user:inference".to_owned()]),
        subscription_type: Some("max".to_owned()),
    };
    cred.save(&path)
        .expect("credential file save should succeed");

    // Simulate what refresh_loop does after a successful OAuth response:
    // read the original file, build a new CredentialFile with refreshed tokens,
    // and verify subscription_type is preserved in the write-back.
    let original = CredentialFile::load(&path).expect("credential file load should succeed");
    let refreshed = CredentialFile {
        token: "sk-ant-oat-refreshed".to_owned(),
        refresh_token: Some("rt-new".to_owned()),
        expires_at: Some(unix_epoch_ms() + 28_800_000),
        scopes: original.scopes.clone(),
        subscription_type: original.subscription_type.clone(),
    };
    refreshed
        .save(&path)
        .expect("refreshed credential file save should succeed");

    let reloaded =
        CredentialFile::load(&path).expect("reloaded credential file load should succeed");
    assert_eq!(
        reloaded.token, "sk-ant-oat-refreshed",
        "reloaded token should match the refreshed token"
    );
    assert_eq!(
        reloaded.refresh_token.as_deref(),
        Some("rt-new"),
        "reloaded refresh token should match the new refresh token"
    );
    assert_eq!(
        reloaded.subscription_type.as_deref(),
        Some("max"),
        "subscription_type must survive refresh write-back"
    );
}

#[tokio::test]
async fn refresh_write_back_from_claude_code_wrapper_preserves_subscription_type() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");

    // Write in Claude Code claudeAiOauth wrapper format (with subscriptionType)
    std::fs::write(
        &path,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-wrapped","refreshToken":"rt-wrapped","expiresAt":9999999999000,"subscriptionType":"pro_plus"}}"#,
    )
    .expect("writing credential JSON file should succeed");

    let original = CredentialFile::load(&path).expect(
        "credential file load should succeed for claudeAiOauth wrapper with subscriptionType",
    );
    assert_eq!(
        original.subscription_type.as_deref(),
        Some("pro_plus"),
        "subscription_type should be parsed from claudeAiOauth wrapper"
    );

    // Simulate refresh write-back preserving subscription_type
    let refreshed = CredentialFile {
        token: "sk-ant-oat-new".to_owned(),
        refresh_token: Some("rt-new".to_owned()),
        expires_at: Some(unix_epoch_ms() + 28_800_000),
        scopes: None,
        subscription_type: original.subscription_type,
    };
    refreshed
        .save(&path)
        .expect("refreshed credential file save should succeed");

    let reloaded =
        CredentialFile::load(&path).expect("reloaded credential file load should succeed");
    assert_eq!(
        reloaded.subscription_type.as_deref(),
        Some("pro_plus"),
        "subscription_type from Claude Code wrapper must survive refresh"
    );
}

#[tokio::test]
async fn refreshing_provider_shuts_down_cleanly() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join(".credentials.json");
    let cred = CredentialFile {
        token: "sk-ant-oat-token".to_owned(),
        refresh_token: Some("rt-test".to_owned()),
        expires_at: Some(unix_epoch_ms() + 7_200_000),
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path)
        .expect("credential file save should succeed");

    let provider = RefreshingCredentialProvider::new(path).expect("should create provider");
    provider.shutdown();
    // Drop triggers abort of background task; should not panic
    drop(provider);
}

#[tokio::test]
async fn credential_file_roundtrip_preserves_all_fields() {
    let dir = tempfile::tempdir().expect("temp dir creation should succeed");
    let path = dir.path().join("full.json");

    let original = CredentialFile {
        token: "sk-ant-oat-full".to_owned(),
        refresh_token: Some("rt-full".to_owned()),
        expires_at: Some(1_800_000_000_000),
        scopes: Some(vec!["user:inference".to_owned(), "org:admin".to_owned()]),
        subscription_type: Some("enterprise".to_owned()),
    };
    original
        .save(&path)
        .expect("credential file save should succeed");

    let loaded = CredentialFile::load(&path)
        .expect("credential file load should succeed for full field roundtrip");
    assert_eq!(
        loaded.token, original.token,
        "loaded token should match original"
    );
    assert_eq!(
        loaded.refresh_token, original.refresh_token,
        "loaded refresh_token should match original"
    );
    assert_eq!(
        loaded.expires_at, original.expires_at,
        "loaded expires_at should match original"
    );
    assert_eq!(
        loaded.scopes, original.scopes,
        "loaded scopes should match original"
    );
    assert_eq!(
        loaded.subscription_type, original.subscription_type,
        "loaded subscription_type should match original"
    );
}
