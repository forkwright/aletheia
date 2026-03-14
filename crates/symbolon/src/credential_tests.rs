#![expect(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
use super::*;

#[test]
fn credential_file_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.json");

    let cred = CredentialFile {
        token: "sk-test-123".to_owned(),
        refresh_token: Some("rt-test-456".to_owned()),
        expires_at: Some(1_700_000_000_000),
        scopes: Some(vec!["user:inference".to_owned()]),
        subscription_type: Some("max".to_owned()),
    };
    cred.save(&path).unwrap();

    let loaded = CredentialFile::load(&path).unwrap();
    assert_eq!(loaded.token, "sk-test-123");
    assert_eq!(loaded.refresh_token.as_deref(), Some("rt-test-456"));
    assert_eq!(loaded.expires_at, Some(1_700_000_000_000));
}

#[test]
fn credential_file_missing_returns_none() {
    assert!(CredentialFile::load(Path::new("/nonexistent/path.json")).is_none());
}

#[test]
fn credential_file_malformed_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "not json").unwrap();
    assert!(CredentialFile::load(&path).is_none());
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
    assert!(with.has_refresh_token());

    let without = CredentialFile {
        refresh_token: None,
        ..with.clone()
    };
    assert!(!without.has_refresh_token());

    let empty = CredentialFile {
        refresh_token: Some(String::new()),
        ..without
    };
    assert!(!empty.has_refresh_token());
}

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
fn env_provider_detects_oauth_by_prefix() {
    let var = "ALETHEIA_TEST_OAUTH_PREFIX_748";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, "sk-ant-oat-test-token-value") };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider.get_credential().unwrap();
    assert_eq!(cred.source, CredentialSource::OAuth);
    unsafe { std::env::remove_var(var) };
}

#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_api_key_stays_environment() {
    let var = "ALETHEIA_TEST_APIKEY_748";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, "sk-ant-api-test-key") };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider.get_credential().unwrap();
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
    let cred = provider.get_credential().unwrap();
    assert_eq!(cred.source, CredentialSource::OAuth);
    unsafe { std::env::remove_var(var) };
}

#[test]
fn file_provider_reads_token() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("anthropic.json");
    let cred = CredentialFile {
        token: "sk-file-token".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).unwrap();

    let provider = FileCredentialProvider::new(path);
    let result = provider.get_credential().unwrap();
    assert_eq!(result.secret, "sk-file-token");
    assert_eq!(result.source, CredentialSource::File);
}

#[test]
fn file_provider_missing_file_returns_none() {
    let provider = FileCredentialProvider::new(PathBuf::from("/nonexistent/cred.json"));
    assert!(provider.get_credential().is_none());
}

#[test]
fn file_provider_detects_file_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("anthropic.json");

    let cred1 = CredentialFile {
        token: "token-v1".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred1.save(&path).unwrap();

    let provider = FileCredentialProvider::new(path.clone());
    let r1 = provider.get_credential().unwrap();
    assert_eq!(r1.secret, "token-v1");

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
    cred2.save(&path).unwrap();

    let r2 = provider.get_credential().unwrap();
    assert_eq!(r2.secret, "token-v2");
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
    let cred = chain.get_credential().unwrap();
    assert_eq!(cred.secret, "first");
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
    let cred = chain.get_credential().unwrap();
    assert_eq!(cred.secret, "fallback");
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
    assert!(chain.get_credential().is_none());
}

#[test]
fn chain_empty_providers_returns_none() {
    let chain = CredentialChain::new(vec![]);
    assert!(chain.get_credential().is_none());
}

#[test]
fn claude_code_default_path_uses_home() {
    // NOTE: depends on $HOME being set — typical in CI and dev
    if let Some(path) = claude_code_default_path() {
        assert!(path.ends_with(".claude/.credentials.json"));
    }
}

#[test]
fn claude_code_provider_missing_file_returns_none() {
    let result = claude_code_provider(Path::new("/nonexistent/.credentials.json"));
    assert!(result.is_none());
}

#[test]
fn claude_code_provider_static_token() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".credentials.json");
    let cred = CredentialFile {
        token: "sk-ant-api-static".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).unwrap();

    let provider = claude_code_provider(&path).expect("should return provider");
    let resolved = provider.get_credential().unwrap();
    assert_eq!(resolved.secret, "sk-ant-api-static");
    assert_eq!(resolved.source, CredentialSource::File);
}

#[tokio::test]
async fn claude_code_provider_with_access_token_alias() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".credentials.json");
    std::fs::write(
        &path,
        r#"{"accessToken": "sk-ant-oat-cc-token", "refreshToken": "rt-cc"}"#,
    )
    .unwrap();

    let provider = claude_code_provider(&path).expect("should return provider");
    let resolved = provider.get_credential().unwrap();
    assert_eq!(resolved.secret, "sk-ant-oat-cc-token");
    assert_eq!(resolved.source, CredentialSource::OAuth);
}

#[test]
fn claude_code_provider_malformed_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".credentials.json");
    std::fs::write(&path, "not valid json").unwrap();
    assert!(claude_code_provider(&path).is_none());
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
    assert!(cred.seconds_remaining().is_none());
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
    let remaining = cred.seconds_remaining().unwrap();
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
    let remaining = cred.seconds_remaining().unwrap();
    assert!(
        remaining > 0 && remaining <= 3600,
        "expected ~3600s remaining, got {remaining}"
    );
}
