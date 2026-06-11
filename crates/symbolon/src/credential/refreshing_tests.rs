#![expect(clippy::expect_used, reason = "test assertions")]

//! Tests for time helpers (`unix_epoch_ms`, `seconds_remaining`) and the
//! `RefreshingCredentialProvider` happy path / shutdown / write-back semantics.

use koina::credential::{CredentialProvider, CredentialSource};
use koina::secret::SecretString;

use super::*;

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
        token: SecretString::from("t"),
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
        token: SecretString::from("t"),
        refresh_token: None,
        expires_at: Some(1_000_000_000_000),
        scopes: None,
        subscription_type: None,
    };
    let remaining = cred
        .seconds_remaining()
        .expect("expired credential has seconds_remaining");
    assert!(
        remaining < 0,
        "expected negative remaining, got {remaining}"
    );
}

#[test]
fn seconds_remaining_positive_for_future_expiry() {
    let far_future_ms = unix_epoch_ms() + 3_600_000;
    let cred = CredentialFile {
        token: SecretString::from("t"),
        refresh_token: None,
        expires_at: Some(far_future_ms),
        scopes: None,
        subscription_type: None,
    };
    let remaining = cred
        .seconds_remaining()
        .expect("future-expiry credential has seconds_remaining");
    assert!(
        remaining > 0 && remaining <= 3600,
        "expected ~3600s remaining, got {remaining}"
    );
}

#[tokio::test]
async fn refreshing_provider_reads_credential_file_and_provides_token() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    let far_future_ms = unix_epoch_ms() + 7_200_000;
    let cred = CredentialFile {
        token: SecretString::from("sk-ant-oat-initial-token"),
        refresh_token: Some(SecretString::from("rt-test-refresh")),
        expires_at: Some(far_future_ms),
        scopes: Some(vec!["user:inference".to_owned()]),
        subscription_type: Some("max".to_owned()),
    };
    cred.save(&path)
        .expect("save refreshing provider credential");

    let provider = RefreshingCredentialProvider::new(path.clone()).expect("should create provider");
    let resolved = provider
        .get_credential()
        .expect("refreshing provider should return credential");
    assert_eq!(resolved.secret.expose_secret(), "sk-ant-oat-initial-token");
    assert_eq!(resolved.source, CredentialSource::OAuth);

    // WHY: Shut down background task to avoid leaking
    provider.shutdown();
}

#[tokio::test]
async fn refresh_write_back_preserves_subscription_type() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    let cred = CredentialFile {
        token: SecretString::from("sk-ant-oat-original"),
        refresh_token: Some(SecretString::from("rt-original")),
        expires_at: Some(unix_epoch_ms() + 7_200_000),
        scopes: Some(vec!["user:inference".to_owned()]),
        subscription_type: Some("max".to_owned()),
    };
    cred.save(&path)
        .expect("save original credential for refresh test");

    // NOTE: Simulate what refresh_loop does after a successful OAuth response:
    // read the original file, build a new CredentialFile with refreshed tokens,
    // and verify subscription_type is preserved in the write-back.
    let original = CredentialFile::load(&path).expect("load original credential");
    let refreshed = CredentialFile {
        token: SecretString::from("sk-ant-oat-refreshed"),
        refresh_token: Some(SecretString::from("rt-new")),
        expires_at: Some(unix_epoch_ms() + 28_800_000),
        scopes: original.scopes.clone(),
        subscription_type: original.subscription_type.clone(),
    };
    refreshed.save(&path).expect("save refreshed credential");

    let reloaded = CredentialFile::load(&path).expect("load refreshed credential");
    assert_eq!(reloaded.token.expose_secret(), "sk-ant-oat-refreshed");
    assert_eq!(
        reloaded
            .refresh_token
            .as_ref()
            .map(SecretString::expose_secret),
        Some("rt-new")
    );
    assert_eq!(
        reloaded.subscription_type.as_deref(),
        Some("max"),
        "subscription_type must survive refresh write-back"
    );
}

#[tokio::test]
async fn refresh_write_back_from_claude_code_wrapper_preserves_subscription_type() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");

    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts",
    )]
    std::fs::write(
        &path,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-wrapped","refreshToken":"rt-wrapped","expiresAt":9999999999000,"subscriptionType":"pro_plus"}}"#,
    )
    .expect("write wrapped credential with subscription_type");

    let original =
        CredentialFile::load(&path).expect("load wrapped credential with subscription_type");
    assert_eq!(original.subscription_type.as_deref(), Some("pro_plus"));

    // NOTE: Simulate refresh write-back preserving subscription_type
    let refreshed = CredentialFile {
        token: SecretString::from("sk-ant-oat-new"),
        refresh_token: Some(SecretString::from("rt-new")),
        expires_at: Some(unix_epoch_ms() + 28_800_000),
        scopes: None,
        subscription_type: original.subscription_type,
    };
    refreshed
        .save(&path)
        .expect("save refreshed wrapped credential");

    let reloaded = CredentialFile::load(&path).expect("load refreshed wrapped credential");
    assert_eq!(
        reloaded.subscription_type.as_deref(),
        Some("pro_plus"),
        "subscription_type from Claude Code wrapper must survive refresh"
    );
}

#[tokio::test]
async fn refreshing_provider_shuts_down_cleanly() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    let cred = CredentialFile {
        token: SecretString::from("sk-ant-oat-token"),
        refresh_token: Some(SecretString::from("rt-test")),
        expires_at: Some(unix_epoch_ms() + 7_200_000),
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).expect("save credential for shutdown test");

    let provider = RefreshingCredentialProvider::new(path).expect("should create provider");
    provider.shutdown();
    drop(provider);
}

#[tokio::test]
async fn credential_file_roundtrip_preserves_all_fields() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("full.json");

    let original = CredentialFile {
        token: SecretString::from("sk-ant-oat-full"),
        refresh_token: Some(SecretString::from("rt-full")),
        expires_at: Some(1_800_000_000_000),
        scopes: Some(vec!["user:inference".to_owned(), "org:admin".to_owned()]),
        subscription_type: Some("enterprise".to_owned()),
    };
    original.save(&path).expect("save full credential file");

    let loaded = CredentialFile::load(&path).expect("load full credential file");
    assert_eq!(loaded.token.expose_secret(), original.token.expose_secret());
    assert_eq!(
        loaded
            .refresh_token
            .as_ref()
            .map(SecretString::expose_secret),
        original
            .refresh_token
            .as_ref()
            .map(SecretString::expose_secret)
    );
    assert_eq!(loaded.expires_at, original.expires_at);
    assert_eq!(loaded.scopes, original.scopes);
    assert_eq!(loaded.subscription_type, original.subscription_type);
}
