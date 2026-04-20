#![expect(clippy::expect_used, reason = "test assertions")]

//! Mutation-hardening tests for `EnvCredentialProvider`, `FileCredentialProvider`,
//! and `CredentialChain` covering the 16 missed mutants tracked in #3710.
//!
//! WHY: existing tests in `../../credential_tests.rs` exercise happy paths and a
//! subset of edge cases, but cargo-mutants still kills mutants in the constructor
//! bodies, arithmetic (`/ 1000`), the mtime-cache freshness comparison (`<`), and
//! the `name()` accessors. These tests pin those specific behaviours.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use koina::credential::{Credential, CredentialProvider, CredentialSource};
use koina::secret::SecretString;

use super::super::file_ops::CredentialFile;
use super::{CredentialChain, EnvCredentialProvider, FileCredentialProvider};

// ---- EnvCredentialProvider ---------------------------------------------------

// WHY: kills the `with_source` → `Default::default()` mutant (line 40). If the
// constructor body is replaced with `Default::default()`, `var_name` becomes
// empty, so `name()` no longer reflects the caller-supplied variable name.
#[test]
fn env_provider_with_source_preserves_var_name() {
    let provider = EnvCredentialProvider::with_source("MY_CUSTOM_VAR", CredentialSource::OAuth);
    assert_eq!(
        provider.name(),
        "MY_CUSTOM_VAR",
        "with_source() must record the caller-supplied var name"
    );
}

// WHY: kills the `get_credential` → `None` mutant at line 49 by asserting that a
// populated env var returns `Some` with the *exact* secret and the
// Environment source (not OAuth). Also pins the prefix-detection path.
#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_returns_exact_secret_and_environment_source() {
    let var = "ALETHEIA_3710_ENV_EXACT_VALUE";
    let token = "sk-ant-api-exact-value-123";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, token) };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider
        .get_credential()
        .expect("populated env var should yield Some(credential)");
    assert_eq!(
        cred.secret.expose_secret(),
        token,
        "get_credential() must return the env var's exact value"
    );
    assert_eq!(
        cred.source,
        CredentialSource::Environment,
        "non-OAuth-prefixed token must be tagged as Environment"
    );
    unsafe { std::env::remove_var(var) };
}

// WHY: kills the `"" → None` mutant at line 49 separately from a missing env var.
// Empty string must still yield `None`.
#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_empty_string_returns_none() {
    let var = "ALETHEIA_3710_ENV_EMPTY_VALUE";
    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, "") };
    let provider = EnvCredentialProvider::new(var);
    assert!(
        provider.get_credential().is_none(),
        "empty env var value must be rejected"
    );
    unsafe { std::env::remove_var(var) };
}

// WHY: kills mutants on `unix_epoch_ms() / 1000` at line 59. Replacing `/` with
// `*` or `%` would scale the "now" comparison wildly wrong. With an OAuth token
// whose exp is set to `(unix_epoch_ms() / 1000) + 1 hour`, the real division
// accepts it (exp_secs > now_secs). A `*` mutation computes a "now" ~1e15s in
// the future, making every exp appear expired — the token would be rejected.
// A `%` mutation yields a tiny "now" (< 1000), also flipping the comparison.
#[test]
#[expect(unsafe_code, reason = "test-only env var manipulation")]
fn env_provider_now_seconds_division_is_correct() {
    use super::super::unix_epoch_ms;

    let var = "ALETHEIA_3710_ENV_NOW_DIVISION";
    let now_secs = unix_epoch_ms() / 1000;
    // Token expires 1 hour in the future: accepted iff now is computed via `/ 1000`.
    let future_exp = now_secs + 3600;
    let payload_json = format!(r#"{{"exp":{future_exp}}}"#);
    let payload_b64 = koina::base64::encode_url_safe_no_pad(payload_json.as_bytes());
    let token = format!("sk-ant-oat.{payload_b64}.stub");

    // SAFETY: test uses unique var name, no concurrent access
    unsafe { std::env::set_var(var, &token) };
    let provider = EnvCredentialProvider::new(var);
    let cred = provider
        .get_credential()
        .expect("valid future OAuth token must be accepted with correct now-seconds math");
    assert_eq!(cred.source, CredentialSource::OAuth);
    assert_eq!(cred.secret.expose_secret(), token);
    unsafe { std::env::remove_var(var) };
}

// ---- FileCredentialProvider --------------------------------------------------

// WHY: kills the `get_credential` → `None` mutant at line 150 and the
// `name` → `"xyzzy"`/`""` mutants at line 190. The provider must return the
// exact token + File source, and its `.name()` must be the literal "file".
#[test]
fn file_provider_returns_exact_credential_and_file_source() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("exact.json");
    let cred = CredentialFile {
        token: SecretString::from("sk-file-exact-token-001"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).expect("save credential");

    let provider = FileCredentialProvider::new(path);
    let resolved = provider
        .get_credential()
        .expect("loaded credential must be Some");
    assert_eq!(
        resolved.secret.expose_secret(),
        "sk-file-exact-token-001",
        "file provider must return the exact token bytes"
    );
    assert_eq!(
        resolved.source,
        CredentialSource::File,
        "source must be File, not OAuth/Environment/Keyring"
    );
    assert_eq!(
        provider.name(),
        "file",
        "file provider's name() must be the literal \"file\""
    );
}

// WHY: kills the `reload` → `None` mutant at line 130. After first load, the
// cache is populated. Reading a second time without cache invalidation must
// still return the token (hitting the fast path), and a forced reload (by
// clearing the cache) must also re-populate from disk. If `reload` returned
// `None`, the fresh-cache fast path would still work but the slow path would
// fail: we force the slow path by clearing the cache.
#[test]
fn file_provider_reload_populates_cache_from_disk() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("reload.json");
    let cred = CredentialFile {
        token: SecretString::from("sk-file-reload-token"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).expect("save credential");

    let provider = FileCredentialProvider::new(path);

    // First call: cache empty → reload() must populate it.
    let first = provider
        .get_credential()
        .expect("first get_credential must populate cache via reload");
    assert_eq!(first.secret.expose_secret(), "sk-file-reload-token");

    // Clear cache, then ensure reload runs again and still yields Some.
    {
        let mut guard = provider.cached.write().expect("write cache");
        *guard = None;
    }
    let second = provider
        .get_credential()
        .expect("post-clear get_credential must re-reload from disk, not return None");
    assert_eq!(
        second.secret.expose_secret(),
        "sk-file-reload-token",
        "reload() must return Some(token), not None"
    );
}

// WHY: kills the `<` → `==`/`>`/`<=` mutants at line 153 by exercising both
// sides of the `elapsed < FILE_MTIME_CHECK_INTERVAL` boundary. If the comparison
// is flipped, a very recent `checked_at` (elapsed ~0) would take the *slow*
// path, and a stale `checked_at` would take the *fast* path. We observe this
// indirectly: with an obviously-stale `checked_at` and a file whose mtime has
// changed on disk, only the slow path detects the new token. A `<` → `>`
// mutation would return the cached (old) token instead.
#[test]
fn file_provider_mtime_check_interval_boundary_takes_slow_path_when_stale() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("boundary.json");

    let cred_v1 = CredentialFile {
        token: SecretString::from("token-v1"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred_v1.save(&path).expect("save v1");

    let provider = FileCredentialProvider::new(path.clone());
    let r1 = provider.get_credential().expect("v1 load");
    assert_eq!(r1.secret.expose_secret(), "token-v1");

    // Force the cache's checked_at *far* past the FILE_MTIME_CHECK_INTERVAL
    // window (1 hour). With correct `<`: elapsed (1h) < interval (30s) is false,
    // so the slow path runs and re-reads disk. With a flipped comparison
    // (e.g., `>`), the fast path would serve the stale cached token.
    // We also null out the cached mtime to force `reload()` rather than the
    // mtime-matches short-circuit.
    {
        let mut guard = provider.cached.write().expect("write cache");
        if let Some(ref mut c) = *guard {
            c.checked_at = Instant::now()
                .checked_sub(Duration::from_hours(1))
                .unwrap_or(Instant::now());
            c.mtime = SystemTime::UNIX_EPOCH;
        }
    }

    let cred_v2 = CredentialFile {
        token: SecretString::from("token-v2"),
        ..cred_v1
    };
    cred_v2.save(&path).expect("save v2");

    let r2 = provider
        .get_credential()
        .expect("stale checked_at must trigger slow path and reload v2");
    assert_eq!(
        r2.secret.expose_secret(),
        "token-v2",
        "with `elapsed < interval` correctly evaluating to false, slow path must reload new token"
    );
}

// WHY: complements the stale-boundary test above by asserting the fast-path
// side: a *fresh* `checked_at` (elapsed ~0) must return the cached token even
// if the on-disk file has changed. A `<` → `>=` mutation would flip this to
// always take the slow path — still correct here but the mtime mismatch branch
// would then overwrite the cached token. We pin the fast-path observable:
// cached token returned on hot call.
#[test]
fn file_provider_fresh_cache_returns_cached_token_without_disk_read() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("fresh.json");
    let cred = CredentialFile {
        token: SecretString::from("cached-token"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).expect("save cached");

    let provider = FileCredentialProvider::new(path.clone());
    let r1 = provider.get_credential().expect("initial load");
    assert_eq!(r1.secret.expose_secret(), "cached-token");

    // Overwrite the cached secret in memory without touching disk.
    // If the fast path is actually taken (elapsed < interval), this injected
    // value is what get_credential() returns. If a mutation flips `<` and the
    // slow path runs instead, reload() would overwrite our injection with the
    // on-disk "cached-token" — but since the on-disk file is unchanged, the
    // observable would still be "cached-token". To distinguish, we also remove
    // the on-disk file: the slow path's reload() would then return None, while
    // the fast path returns our injected value.
    {
        let mut guard = provider.cached.write().expect("write cache");
        if let Some(ref mut c) = *guard {
            c.token = SecretString::from("fastpath-only-injection");
            c.checked_at = Instant::now(); // ensure elapsed is ~0
        }
    }
    std::fs::remove_file(&path).expect("remove on-disk credential");

    let r2 = provider
        .get_credential()
        .expect("fast path must return cached token without touching (now-missing) disk");
    assert_eq!(
        r2.secret.expose_secret(),
        "fastpath-only-injection",
        "fast path (elapsed < interval) must serve cached token, not re-read disk"
    );
}

// ---- CredentialChain ---------------------------------------------------------

/// Counts calls to `get_credential()` so tests can assert chain ordering and
/// short-circuit semantics.
struct CountingProvider {
    token: Option<String>,
    name: &'static str,
    calls: Arc<Mutex<u32>>,
}

impl CredentialProvider for CountingProvider {
    fn get_credential(&self) -> Option<Credential> {
        if let Ok(mut c) = self.calls.lock() {
            *c += 1;
        }
        self.token.as_ref().map(|t| Credential {
            secret: SecretString::from(t.as_str()),
            source: CredentialSource::Environment,
        })
    }
    fn name(&self) -> &str {
        self.name
    }
}

// WHY: kills the `get_credential` → `None` mutant at line 213 by asserting the
// chain returns the *exact* credential from the first successful provider and
// short-circuits (does not call subsequent providers).
#[test]
fn chain_returns_exact_credential_from_first_successful_provider() {
    let first_calls = Arc::new(Mutex::new(0_u32));
    let second_calls = Arc::new(Mutex::new(0_u32));

    let chain = CredentialChain::new(vec![
        Box::new(CountingProvider {
            token: Some("chain-first-wins".to_owned()),
            name: "first",
            calls: Arc::clone(&first_calls),
        }),
        Box::new(CountingProvider {
            token: Some("chain-second".to_owned()),
            name: "second",
            calls: Arc::clone(&second_calls),
        }),
    ]);

    let cred = chain
        .get_credential()
        .expect("chain must return first provider's credential, not None");
    assert_eq!(
        cred.secret.expose_secret(),
        "chain-first-wins",
        "chain must return the first provider's exact secret"
    );
    assert_eq!(
        *first_calls.lock().expect("lock first_calls"),
        1,
        "first provider must be consulted exactly once"
    );
    assert_eq!(
        *second_calls.lock().expect("lock second_calls"),
        0,
        "chain must short-circuit: second provider must not be called"
    );
}

// WHY: kills the `get_credential` → `None` mutant at line 213 for the fallback
// case: the chain must skip providers that return None and return the next
// successful one's credential.
#[test]
fn chain_skips_none_and_returns_fallback_credential() {
    let chain = CredentialChain::new(vec![
        Box::new(CountingProvider {
            token: None,
            name: "skipped",
            calls: Arc::new(Mutex::new(0)),
        }),
        Box::new(CountingProvider {
            token: Some("chain-fallback-token".to_owned()),
            name: "fallback",
            calls: Arc::new(Mutex::new(0)),
        }),
    ]);

    let cred = chain
        .get_credential()
        .expect("chain must fall through to fallback provider, not return None");
    assert_eq!(
        cred.secret.expose_secret(),
        "chain-fallback-token",
        "chain must return the fallback provider's secret"
    );
}

// WHY: kills the `name` → `""` mutant at line 229. The chain's name must be the
// literal "chain" before any provider has resolved a credential.
#[test]
fn chain_default_name_is_literal_chain() {
    let chain = CredentialChain::new(vec![]);
    assert_eq!(
        chain.name(),
        "chain",
        "fresh chain's name() must be the literal \"chain\", not empty"
    );
}

// WHY: complements the above — even with providers present, the name is still
// "chain" until one resolves. Pins the literal against `""`/`"xyzzy"` mutations.
#[test]
fn chain_name_before_resolution_is_chain() {
    let chain = CredentialChain::new(vec![Box::new(CountingProvider {
        token: Some("t".to_owned()),
        name: "inner",
        calls: Arc::new(Mutex::new(0)),
    })]);
    assert_eq!(chain.name(), "chain");
}

// WHY: explicit coverage for an empty chain returning None. This is the
// canonical "no providers" case and pins the return type's None branch.
#[test]
fn chain_with_no_providers_returns_none() {
    let chain = CredentialChain::new(vec![]);
    assert!(
        chain.get_credential().is_none(),
        "empty chain must return None"
    );
}

// WHY: explicit coverage for all-None providers returning None. Kills any
// mutation that would make the chain return `Some(Default::default())` or
// otherwise fabricate a credential.
#[test]
fn chain_with_all_none_providers_returns_none() {
    let chain = CredentialChain::new(vec![
        Box::new(CountingProvider {
            token: None,
            name: "a",
            calls: Arc::new(Mutex::new(0)),
        }),
        Box::new(CountingProvider {
            token: None,
            name: "b",
            calls: Arc::new(Mutex::new(0)),
        }),
    ]);
    assert!(chain.get_credential().is_none());
}

// WHY: file provider with non-existent path must return None. Separately pinned
// to kill any `get_credential` mutation that would fabricate `Some`.
#[test]
fn file_provider_nonexistent_path_returns_none() {
    let provider =
        FileCredentialProvider::new(PathBuf::from("/definitely/not/a/real/path/3710.json"));
    assert!(provider.get_credential().is_none());
}
