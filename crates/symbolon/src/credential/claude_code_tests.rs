#![expect(clippy::expect_used, reason = "test assertions")]

//! Tests for the `claude_code_provider` factory and its default path discovery.

use std::path::Path;

use koina::credential::CredentialSource;
use koina::secret::SecretString;

use super::*;

#[test]
fn claude_code_provider_missing_file_returns_none() {
    let result = claude_code_provider(Path::new("/nonexistent/.credentials.json"));
    assert!(result.is_none());
}

#[test]
fn claude_code_provider_static_token() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    let cred = CredentialFile {
        token: SecretString::from("sk-ant-api-static"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).expect("save static credential file");

    let provider = claude_code_provider(&path).expect("should return provider");
    let resolved = provider
        .get_credential()
        .expect("static credential should resolve");
    assert_eq!(resolved.secret.expose_secret(), "sk-ant-api-static");
    assert_eq!(resolved.source, CredentialSource::File);
}

#[tokio::test]
async fn claude_code_provider_with_access_token_alias() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
    )]
    std::fs::write(
        &path,
        r#"{"accessToken": "sk-ant-oat-cc-token", "refreshToken": "rt-cc"}"#,
    )
    .expect("write access-token-alias credential file");

    let provider = claude_code_provider(&path).expect("should return provider");
    let resolved = provider
        .get_credential()
        .expect("access token alias credential should resolve");
    assert_eq!(resolved.secret.expose_secret(), "sk-ant-oat-cc-token");
    assert_eq!(resolved.source, CredentialSource::OAuth);
}

#[tokio::test]
async fn claude_code_provider_with_claude_code_oauth_wrapper() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
    )]
    std::fs::write(
        &path,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-wrapped","refreshToken":"rt-wrapped"}}"#,
    )
    .expect("write claude-code oauth wrapper credential file");

    let provider = claude_code_provider(&path).expect("should return provider for wrapped format");
    let resolved = provider
        .get_credential()
        .expect("wrapped oauth credential should resolve");
    assert_eq!(resolved.secret.expose_secret(), "sk-ant-oat-wrapped");
    assert_eq!(resolved.source, CredentialSource::OAuth);
}

#[test]
fn claude_code_provider_malformed_returns_none() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
    )]
    std::fs::write(&path, "not valid json").expect("write malformed json credential file");
    assert!(claude_code_provider(&path).is_none());
}
