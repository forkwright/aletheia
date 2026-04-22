#![expect(clippy::expect_used, reason = "test assertions")]

//! Tests for `CredentialFile` parsing and save/load round-trips.
//!
//! Split from `credential_tests.rs` to satisfy `RUST/file-too-long`.

use std::path::Path;

use koina::secret::SecretString;

use super::*;

#[test]
fn credential_file_roundtrip() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("test.json");

    let cred = CredentialFile {
        token: SecretString::from("sk-test-123"),
        refresh_token: Some(SecretString::from("rt-test-456")),
        expires_at: Some(1_700_000_000_000),
        scopes: Some(vec!["user:inference".to_owned()]),
        subscription_type: Some("max".to_owned()),
    };
    cred.save(&path).expect("save credential file");

    let loaded = CredentialFile::load(&path).expect("load saved credential file");
    assert_eq!(loaded.token.expose_secret(), "sk-test-123");
    assert_eq!(
        loaded
            .refresh_token
            .as_ref()
            .map(SecretString::expose_secret),
        Some("rt-test-456")
    );
    assert_eq!(loaded.expires_at, Some(1_700_000_000_000));
}

#[test]
fn credential_file_missing_returns_none() {
    assert!(CredentialFile::load(Path::new("/nonexistent/path.json")).is_none());
}

#[test]
fn credential_file_malformed_returns_none() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("bad.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
    )]
    std::fs::write(&path, "not json").expect("write malformed json file");
    assert!(CredentialFile::load(&path).is_none());
}

#[test]
fn credential_file_load_claude_code_oauth_wrapper() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts",
    )]
    std::fs::write(
        &path,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-wrapped","refreshToken":"rt-wrapped","expiresAt":9999999999000}}"#,
    )
    .expect("write oauth wrapper credential file");

    let loaded = CredentialFile::load(&path).expect("load oauth wrapper credential file");
    assert_eq!(loaded.token.expose_secret(), "sk-ant-oat-wrapped");
    assert_eq!(
        loaded
            .refresh_token
            .as_ref()
            .map(SecretString::expose_secret),
        Some("rt-wrapped")
    );
    assert_eq!(loaded.expires_at, Some(9_999_999_999_000));
    assert!(loaded.has_refresh_token());
}

#[test]
fn credential_file_load_wrapped_no_refresh_token() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
    )]
    std::fs::write(
        &path,
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-no-rt"}}"#,
    )
    .expect("write no-refresh-token credential file");

    let loaded = CredentialFile::load(&path).expect("load no-refresh-token credential file");
    assert_eq!(loaded.token.expose_secret(), "sk-ant-oat-no-rt");
    assert!(!loaded.has_refresh_token());
}

#[test]
fn credential_file_load_flat_takes_precedence_over_wrapper() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
    )]
    std::fs::write(
        &path,
        r#"{"token":"flat-token","claudeAiOauth":{"accessToken":"wrapped-token"}}"#,
    )
    .expect("write flat-token-wins credential file");
    let loaded = CredentialFile::load(&path).expect("load flat-token-wins credential file");
    assert_eq!(loaded.token.expose_secret(), "flat-token");
}

#[test]
fn credential_file_load_wrapper_missing_key_returns_none() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");
    #[expect(
        clippy::disallowed_methods,
        reason = "symbolon credential storage writes configuration files; synchronous I/O is required in CLI/init contexts"
    )]
    std::fs::write(&path, r#"{"someOtherKey":{"value":1}}"#)
        .expect("write unknown-key credential file");
    assert!(CredentialFile::load(&path).is_none());
}

#[test]
fn has_refresh_token() {
    let with = CredentialFile {
        token: SecretString::from("t"),
        refresh_token: Some(SecretString::from("rt")),
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
        refresh_token: Some(SecretString::from("")),
        ..without
    };
    assert!(!empty.has_refresh_token());
}
