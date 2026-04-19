#![expect(clippy::expect_used, reason = "test assertions")]

// WHY: The default mock backend shipped by `keyring` 3.x uses `EntryOnly`
// persistence — each `Entry::new(service, user)` call produces a *fresh*
// MockCredential with independent state. That breaks round-trips for our
// provider, whose `store` and `get_credential` methods each construct a new
// `Entry`. To exercise real persistence semantics (what every real backend
// offers) we install a test-only `CredentialBuilder` keyed on
// (service, user) into a process-global HashMap. Because the builder is
// installed exactly once and each test uses unique identifiers, parallel
// execution stays safe.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use keyring::credential::{
    Credential as KeyringCredential, CredentialApi, CredentialBuilder, CredentialBuilderApi,
    CredentialPersistence,
};
use keyring::error::Error as KeyringError;

use koina::credential::{CredentialProvider, CredentialSource};

use super::*;

// ---- test-only in-memory backend ---------------------------------------------

type BackendStore = Mutex<HashMap<(String, String), Vec<u8>>>;

fn backend_store() -> &'static BackendStore {
    static STORE: OnceLock<BackendStore> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug)]
struct TestCredential {
    service: String,
    user: String,
}

impl CredentialApi for TestCredential {
    fn set_secret(&self, secret: &[u8]) -> Result<(), KeyringError> {
        let mut map = backend_store().lock().expect("backend lock poisoned");
        map.insert((self.service.clone(), self.user.clone()), secret.to_vec());
        Ok(())
    }

    fn get_secret(&self) -> Result<Vec<u8>, KeyringError> {
        let map = backend_store().lock().expect("backend lock poisoned");
        match map.get(&(self.service.clone(), self.user.clone())) {
            Some(v) => Ok(v.clone()),
            None => Err(KeyringError::NoEntry),
        }
    }

    fn delete_credential(&self) -> Result<(), KeyringError> {
        let mut map = backend_store().lock().expect("backend lock poisoned");
        match map.remove(&(self.service.clone(), self.user.clone())) {
            Some(_) => Ok(()),
            None => Err(KeyringError::NoEntry),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct TestBuilder;

impl CredentialBuilderApi for TestBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> Result<Box<KeyringCredential>, KeyringError> {
        Ok(Box::new(TestCredential {
            service: service.to_owned(),
            user: user.to_owned(),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::ProcessOnly
    }
}

fn install_test_backend() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let boxed: Box<CredentialBuilder> = Box::new(TestBuilder);
        keyring::set_default_credential_builder(boxed);
    });
}

fn provider(service: &str, username: &str) -> KeyringCredentialProvider {
    install_test_backend();
    KeyringCredentialProvider::with_identifiers(service, username)
}

// ---- constructor mutants (L38 `with_identifiers` -> `Default::default()`,
//      and `new()` / `Default::default()` must carry the documented constants)

#[test]
fn new_uses_documented_default_identifiers() {
    // Kills: `new()` body swapped for a stub that returns unrelated identifiers.
    let p = KeyringCredentialProvider::new();
    assert_eq!(p.service, DEFAULT_SERVICE);
    assert_eq!(p.service, "aletheia");
    assert_eq!(p.username, DEFAULT_USERNAME);
    assert_eq!(p.username, "api-token");
}

#[test]
fn default_matches_new() {
    // Kills: `impl Default` diverging from `new()`.
    let defaulted = KeyringCredentialProvider::default();
    let constructed = KeyringCredentialProvider::new();
    assert_eq!(defaulted.service, constructed.service);
    assert_eq!(defaulted.username, constructed.username);
}

#[test]
fn with_identifiers_stores_custom_service_and_username() {
    // Kills: L38 `with_identifiers` -> `Default::default()` (would yield the
    // default "aletheia"/"api-token" instead of the caller-supplied values).
    let p = provider("svc-with-identifiers-1", "user-with-identifiers-1");
    assert_eq!(p.service, "svc-with-identifiers-1");
    assert_eq!(p.username, "user-with-identifiers-1");
    assert_ne!(p.service, DEFAULT_SERVICE);
    assert_ne!(p.username, DEFAULT_USERNAME);
}

#[test]
fn with_identifiers_accepts_owned_and_borrowed() {
    // Kills: a variant of L38 that ignores one of the two `Into<String>` args.
    let svc = String::from("svc-owned-2");
    let user = "user-borrowed-2";
    let p = KeyringCredentialProvider::with_identifiers(svc, user);
    assert_eq!(p.service, "svc-owned-2");
    assert_eq!(p.username, "user-borrowed-2");
}

// ---- `entry()` mutant (L45): must hand back a handle bound to the configured
//      service/username — verified through the full round-trip.

#[test]
fn entry_round_trips_password_through_store_and_get() {
    // Kills: L45 `entry` -> stub returning an Entry that ignores &self, plus
    // L55 `store` -> `Ok(())` (no-op write would yield NoEntry on read).
    let p = provider("svc-entry-roundtrip-3", "user-entry-roundtrip-3");
    let token = "tok-entry-roundtrip-3";
    p.store(token)
        .expect("store should succeed against test backend");

    let cred = p
        .get_credential()
        .expect("stored credential should be retrievable");
    assert_eq!(cred.secret.expose_secret(), token);
    assert_eq!(cred.source, CredentialSource::Keyring);

    p.delete().expect("cleanup delete should succeed");
}

#[test]
fn entry_handles_are_isolated_per_identifier_pair() {
    // Kills: L45 `entry` -> stub ignoring &self (both providers would then
    // share one default-backed entry and see each other's writes).
    let a = provider("svc-isolation-A-4", "user-isolation-4");
    let b = provider("svc-isolation-B-4", "user-isolation-4");

    a.store("only-in-A").expect("store in A");
    assert_eq!(
        a.get_credential()
            .expect("A should have credential")
            .secret
            .expose_secret(),
        "only-in-A"
    );
    assert!(
        b.get_credential().is_none(),
        "B must not observe A's entry when service differs",
    );

    a.delete().expect("cleanup A");
}

// ---- `store` mutant (L55)

#[test]
fn store_overwrites_existing_token() {
    // Kills: L55 `store` -> `Ok(())` in the update case (a no-op would leave
    // the original value in place).
    let p = provider("svc-overwrite-5", "user-overwrite-5");
    p.store("first").expect("initial store");
    p.store("second").expect("overwriting store");
    let cred = p.get_credential().expect("overwritten credential visible");
    assert_eq!(cred.secret.expose_secret(), "second");
    p.delete().expect("cleanup");
}

// ---- `delete` mutant (L66)

#[test]
fn delete_removes_stored_credential() {
    // Kills: L66 `delete` -> `Ok(())` (would leave the secret, failing the
    // post-condition below).
    let p = provider("svc-delete-6", "user-delete-6");
    p.store("will-be-deleted").expect("store before delete");
    p.delete().expect("delete should succeed");
    assert!(
        p.get_credential().is_none(),
        "get_credential must return None after delete",
    );
}

#[test]
fn delete_on_missing_entry_is_idempotent() {
    // Kills: a variant of L66 that returns Err on NoEntry instead of mapping
    // it to Ok(()). The documented contract promises idempotence.
    let p = provider("svc-delete-missing-7", "user-delete-missing-7");
    // No prior store.
    p.delete()
        .expect("delete of missing entry should map NoEntry to Ok");
    // And again — still Ok.
    p.delete()
        .expect("second delete of missing entry should also be Ok");
}

// ---- `get_credential` mutants (L81)

#[test]
fn get_credential_returns_exact_stored_bytes_with_keyring_source() {
    // Kills: L81 stub returning a default/empty Credential (both the secret
    // and the source-tag assertions below would fail).
    let p = provider("svc-get-exact-8", "user-get-exact-8");
    let token = "exact-bytes-8-!@#$%^&*()";
    p.store(token).expect("store exact-bytes token");

    let cred = p.get_credential().expect("credential present");
    assert_eq!(cred.secret.expose_secret(), token);
    assert_eq!(
        cred.source,
        CredentialSource::Keyring,
        "provider must tag the credential with CredentialSource::Keyring",
    );

    p.delete().expect("cleanup");
}

#[test]
fn get_credential_returns_none_when_no_entry_exists() {
    // Kills: L81 stub returning Some(Default::default()) (a missing entry
    // would then look present). Also pins the NoEntry arm of the match.
    let p = provider("svc-get-missing-9", "user-get-missing-9");
    assert!(
        p.get_credential().is_none(),
        "missing entry must yield None from get_credential",
    );
}

// ---- `token.is_empty()` match-guard mutant (L90)

#[test]
fn get_credential_rejects_empty_stored_token() {
    // Kills: L90 guard flipped to `false` (empty token would then be returned
    // as a valid credential, defeating the documented rejection contract).
    let p = provider("svc-empty-token-10", "user-empty-token-10");
    p.store("").expect("store empty string");
    assert!(
        p.get_credential().is_none(),
        "empty stored token must be rejected (guard must fire)",
    );
    p.delete().expect("cleanup");
}

#[test]
fn get_credential_accepts_nonempty_token() {
    // Kills: L90 guard flipped to `true` (every token would then be rejected
    // as empty, returning None for perfectly valid credentials).
    let p = provider("svc-nonempty-token-11", "user-nonempty-token-11");
    p.store("x").expect("store shortest nonempty token");
    let cred = p
        .get_credential()
        .expect("single-char token must be accepted");
    assert_eq!(cred.secret.expose_secret(), "x");
    p.delete().expect("cleanup");
}

// ---- `name()` mutant (L108 `"keyring"` -> `"xyzzy"` / `""`)

#[test]
fn name_is_exactly_keyring_literal() {
    // Kills: L108 replacements with any other literal ("", "xyzzy", ...).
    let p = KeyringCredentialProvider::new();
    assert_eq!(p.name(), "keyring");
    assert!(!p.name().is_empty(), "name must not be the empty string");
    assert_ne!(p.name(), "xyzzy");
}

#[test]
fn name_is_stable_across_custom_identifiers() {
    // Kills: a variant of L108 that derives the name from `self.service` /
    // `self.username` instead of the fixed contract string.
    let p = provider("svc-name-12", "user-name-12");
    assert_eq!(p.name(), "keyring");
}
