#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::OnceLock;

use koina::credential::{CredentialProvider, CredentialSource};

use super::*;

/// Install a mock credential store, once per process.
///
/// WHY this replaced ~85 lines of hand-rolled mock: keyring 3.x's shipped mock used
/// `EntryOnly` persistence -- each `Entry::new(service, user)` produced a *fresh*
/// credential with independent state, which breaks a round-trip for a provider whose
/// `store` and `get_credential` each construct a new `Entry`. This crate therefore
/// carried its own `CredentialBuilder` over a process-global `HashMap` to get
/// persistence.
///
/// keyring-core 1.0's mock store already has that property: `mock::Store::build` looks
/// up an existing credential by (service, user) and returns the same one. The reason
/// the bespoke backend existed is gone, so it is gone too.
///
/// WHY installed exactly once and never unset: the store is process-global. Each test
/// uses unique identifiers, so parallel execution stays safe -- but a test that reset
/// it would pull the backend out from under any test running beside it.
fn install_test_backend() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let store = keyring_core::mock::Store::new().expect("mock store must build");
        keyring_core::set_default_store(store);
    });
}

fn provider(service: &str, username: &str) -> KeyringCredentialProvider {
    install_test_backend();
    KeyringCredentialProvider::with_identifiers(service, username)
}

// ── constructor mutants ──

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
    // Kills: `with_identifiers` -> `Default::default()` (would yield the
    // default "aletheia"/"api-token" instead of the caller-supplied values).
    let p = provider("svc-with-identifiers-1", "user-with-identifiers-1");
    assert_eq!(p.service, "svc-with-identifiers-1");
    assert_eq!(p.username, "user-with-identifiers-1");
    assert_ne!(p.service, DEFAULT_SERVICE);
    assert_ne!(p.username, DEFAULT_USERNAME);
}

#[test]
fn with_identifiers_accepts_owned_and_borrowed() {
    // Kills: a `with_identifiers` variant that ignores one of the two `Into<String>` args.
    let svc = String::from("svc-owned-2");
    let user = "user-borrowed-2";
    let p = KeyringCredentialProvider::with_identifiers(svc, user);
    assert_eq!(p.service, "svc-owned-2");
    assert_eq!(p.username, "user-borrowed-2");
}

#[test]
fn for_instance_namespaces_service_by_provider_name() {
    // WHY(#5250): the keyring service must incorporate the credential
    // provider name so two different providers on the same instance never
    // share one entry.
    let root = std::path::Path::new("/tmp/instance-a");
    let anthropic = KeyringCredentialProvider::for_instance(root, "anthropic");
    let openai = KeyringCredentialProvider::for_instance(root, "openai");
    assert_ne!(anthropic.service, openai.service);
    assert_eq!(anthropic.service, "aletheia:anthropic");
    assert_eq!(anthropic.username, openai.username);
}

#[test]
fn for_instance_namespaces_username_by_instance_root() {
    // WHY(#5250): the keyring username must incorporate the instance's
    // oikos root so two co-installed deployments never share one entry.
    let a = KeyringCredentialProvider::for_instance(
        std::path::Path::new("/home/alice/.aletheia"),
        "anthropic",
    );
    let b = KeyringCredentialProvider::for_instance(
        std::path::Path::new("/home/bob/.aletheia"),
        "anthropic",
    );
    assert_ne!(a.username, b.username);
    assert_eq!(a.service, b.service);
}

#[test]
fn for_instance_never_collides_with_the_legacy_global_identity() {
    // WHY(#5250): a namespaced provider must never coincidentally resolve
    // to the exact (service, user) pair `new()` uses -- that pair is what a
    // stale keyring entry from a pre-namespacing install occupies.
    let namespaced =
        KeyringCredentialProvider::for_instance(std::path::Path::new("/srv/aletheia"), "anthropic");
    let legacy = KeyringCredentialProvider::new();
    assert!(
        namespaced.service != legacy.service || namespaced.username != legacy.username,
        "a namespaced identity must not equal the legacy global (service, user) pair"
    );
}

#[test]
fn for_instance_isolates_two_deployments_sharing_one_machine() {
    // WHY(#5250): before namespacing, `KeyringCredentialProvider::new()`
    // always resolved the same (service, user) pair regardless of which
    // instance constructed it, so instance B silently read whatever
    // instance A (or a stale prior install) had stored. This test fails
    // against the pre-fix constructor: replacing `for_instance` with two
    // `new()` calls makes B observe A's token.
    install_test_backend();
    let instance_a = KeyringCredentialProvider::for_instance(
        std::path::Path::new("/srv/instance-a"),
        "anthropic",
    );
    let instance_b = KeyringCredentialProvider::for_instance(
        std::path::Path::new("/srv/instance-b"),
        "anthropic",
    );

    instance_a
        .store("token-belongs-to-a")
        .expect("store for instance A");

    assert!(
        instance_b.get_credential().is_none(),
        "instance B must not see instance A's keyring-stored credential"
    );
    assert_eq!(
        instance_a
            .get_credential()
            .expect("instance A retains its own credential")
            .secret
            .expose_secret(),
        "token-belongs-to-a"
    );

    instance_a.delete().expect("cleanup instance A");
}

// ── entry() mutants ──

#[test]
fn entry_round_trips_password_through_store_and_get() {
    // Kills: `entry` -> stub returning an Entry that ignores &self, plus
    // `store` -> `Ok(())` (no-op write would yield NoEntry on read).
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
    // Kills: `entry` -> stub ignoring &self (both providers would then
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

// ── store mutants ──

#[test]
fn store_overwrites_existing_token() {
    // Kills: `store` -> `Ok(())` in the update case (a no-op would leave
    // the original value in place).
    let p = provider("svc-overwrite-5", "user-overwrite-5");
    p.store("first").expect("initial store");
    p.store("second").expect("overwriting store");
    let cred = p.get_credential().expect("overwritten credential visible");
    assert_eq!(cred.secret.expose_secret(), "second");
    p.delete().expect("cleanup");
}

// ── delete mutants ──

#[test]
fn delete_removes_stored_credential() {
    // Kills: `delete` -> `Ok(())` (would leave the secret, failing the
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
    // Kills: a `delete` variant that returns Err on NoEntry instead of mapping
    // it to Ok(()). The documented contract promises idempotence.
    let p = provider("svc-delete-missing-7", "user-delete-missing-7");
    // No prior store.
    p.delete()
        .expect("delete of missing entry should map NoEntry to Ok");
    // And again — still Ok.
    p.delete()
        .expect("second delete of missing entry should also be Ok");
}

// ── get_credential mutants ──

#[test]
fn get_credential_returns_exact_stored_bytes_with_keyring_source() {
    // Kills: a `get_credential` stub returning a default/empty Credential (both the secret
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
    // Kills: a `get_credential` stub returning Some(Default::default()) (a missing entry
    // would then look present). Also pins the NoEntry arm of the match.
    let p = provider("svc-get-missing-9", "user-get-missing-9");
    assert!(
        p.get_credential().is_none(),
        "missing entry must yield None from get_credential",
    );
}

// ── token.is_empty() match-guard mutants ──

#[test]
fn get_credential_rejects_empty_stored_token() {
    // Kills: the `token.is_empty()` guard flipped to `false` (empty token would then be returned
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
    // Kills: the `token.is_empty()` guard flipped to `true` (every token would then be rejected
    // as empty, returning None for perfectly valid credentials).
    let p = provider("svc-nonempty-token-11", "user-nonempty-token-11");
    p.store("x").expect("store shortest nonempty token");
    let cred = p
        .get_credential()
        .expect("single-char token must be accepted");
    assert_eq!(cred.secret.expose_secret(), "x");
    p.delete().expect("cleanup");
}

// ── name() mutants ──

#[test]
fn name_is_exactly_keyring_literal() {
    // Kills: `name()` replacements with any other literal ("", "xyzzy", ...).
    let p = KeyringCredentialProvider::new();
    assert_eq!(p.name(), "keyring");
    assert!(!p.name().is_empty(), "name must not be the empty string");
    assert_ne!(p.name(), "xyzzy");
}

#[test]
fn name_is_stable_across_custom_identifiers() {
    // Kills: a `name()` variant that derives the name from `self.service` /
    // `self.username` instead of the fixed contract string.
    let p = provider("svc-name-12", "user-name-12");
    assert_eq!(p.name(), "keyring");
}
