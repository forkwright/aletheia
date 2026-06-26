#![expect(clippy::expect_used, reason = "test assertions")]

//! Tests for `FileCredentialProvider` and `CredentialChain` fallback semantics.

use std::path::PathBuf;
use std::time::Instant;

use koina::credential::{Credential, CredentialProvider, CredentialSource};
use koina::secret::SecretString;

use super::*;

#[test]
fn file_provider_reads_token() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("anthropic.json");
    let cred = CredentialFile {
        token: SecretString::from("sk-file-token"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).expect("save file credential");

    let provider = FileCredentialProvider::new(path);
    let result = provider
        .get_credential()
        .expect("file provider should return credential");
    assert_eq!(result.secret.expose_secret(), "sk-file-token");
    assert_eq!(result.source, CredentialSource::File);
}

#[test]
fn file_provider_missing_file_returns_none() {
    let provider = FileCredentialProvider::new(PathBuf::from("/nonexistent/cred.json"));
    assert!(provider.get_credential().is_none());
}

#[test]
fn file_provider_detects_file_change() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("anthropic.json");

    let cred1 = CredentialFile {
        token: SecretString::from("token-v1"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred1.save(&path).expect("save initial credential");

    let provider = FileCredentialProvider::new(path.clone());
    let r1 = provider
        .get_credential()
        .expect("initial credential should load");
    assert_eq!(r1.secret.expose_secret(), "token-v1");

    if let Ok(mut guard) = provider.cached.write()
        && let Some(ref mut c) = *guard
    {
        c.checked_at = Instant::now()
            .checked_sub(Duration::from_mins(1))
            .unwrap_or(Instant::now());
        c.mtime = SystemTime::UNIX_EPOCH;
    }

    let cred2 = CredentialFile {
        token: SecretString::from("token-v2"),
        ..cred1
    };
    cred2.save(&path).expect("save updated credential");

    let r2 = provider
        .get_credential()
        .expect("updated credential should reload");
    assert_eq!(r2.secret.expose_secret(), "token-v2");
}

struct StaticProvider {
    token: Option<String>,
    name: &'static str,
}

struct ShutdownTrackingProvider {
    name: &'static str,
    shutdown_called: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CredentialProvider for StaticProvider {
    fn get_credential(&self) -> Option<Credential> {
        self.token.as_ref().map(|t| Credential {
            secret: SecretString::from(t.as_str()),
            source: CredentialSource::Environment,
        })
    }
    fn name(&self) -> &str {
        self.name
    }
}

impl CredentialProvider for ShutdownTrackingProvider {
    fn shutdown(&self) {
        self.shutdown_called
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn get_credential(&self) -> Option<Credential> {
        None
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
        .expect("chain should return first matching credential");
    assert_eq!(cred.secret.expose_secret(), "first");
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
        .expect("chain should skip empty and return fallback");
    assert_eq!(cred.secret.expose_secret(), "fallback");
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
fn chain_shutdown_propagates_to_all_providers() {
    let first_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let second_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let first_flag_check = std::sync::Arc::clone(&first_flag);
    let second_flag_check = std::sync::Arc::clone(&second_flag);

    let chain = CredentialChain::new(vec![
        Box::new(ShutdownTrackingProvider {
            name: "first",
            shutdown_called: first_flag,
        }),
        Box::new(ShutdownTrackingProvider {
            name: "second",
            shutdown_called: second_flag,
        }),
        Box::new(StaticProvider {
            token: None,
            name: "non-shutdown",
        }),
    ]);

    // WHY: `CredentialProvider::shutdown` has a default no-op, but
    // `CredentialChain` must explicitly forward the signal to every provider
    // so stateful providers like `RefreshingCredentialProvider` receive it.
    chain.shutdown();

    assert!(
        first_flag_check.load(std::sync::atomic::Ordering::SeqCst),
        "first provider should receive shutdown"
    );
    assert!(
        second_flag_check.load(std::sync::atomic::Ordering::SeqCst),
        "second provider should receive shutdown"
    );
}
