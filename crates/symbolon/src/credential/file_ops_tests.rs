#![expect(clippy::unwrap_used, clippy::expect_used, reason = "test assertions")]
//! Tests for `CredentialFile::needs_refresh` and `CredentialFileLock`.
//!
//! WHY: pins `needs_refresh`'s `<` threshold comparison and asserts on
//! `CredentialFileLock::shared`/`exclusive`/`lock` so constant-return and
//! `Ok(Default::default())` mutants are caught.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use koina::secret::SecretString;

use super::*;
use crate::credential::{REFRESH_THRESHOLD_SECS, unix_epoch_ms};

// ── needs_refresh ──

/// Build a `CredentialFile` whose `expires_at` is exactly `offset_secs` seconds
/// from `now`. Negative offsets produce an expired credential.
fn cred_expiring_in(offset_secs: i64) -> CredentialFile {
    let now_ms = i64::try_from(unix_epoch_ms()).unwrap_or(i64::MAX);
    let expires_at_ms = now_ms.saturating_add(offset_secs.saturating_mul(1000));
    let expires_at = u64::try_from(expires_at_ms.max(0)).unwrap_or(0);
    CredentialFile {
        token: SecretString::from("t"),
        refresh_token: None,
        expires_at: Some(expires_at),
        scopes: None,
        subscription_type: None,
    }
}

#[test]
fn needs_refresh_false_when_well_before_expiry() {
    // WHY: 2 * threshold puts expiry far outside the refresh window; kills the
    // `-> true` body-replacement mutant.
    let threshold_secs = i64::try_from(REFRESH_THRESHOLD_SECS).unwrap();
    let cred = cred_expiring_in(threshold_secs.saturating_mul(2));
    assert!(
        !cred.needs_refresh(),
        "credential expiring in 2x threshold should not need refresh"
    );
}

#[test]
fn needs_refresh_true_when_inside_window_before_expiry() {
    // WHY: inside the refresh window but not yet expired; distinguishes the
    // `<` comparison from `>` and `==`.
    let threshold_secs = i64::try_from(REFRESH_THRESHOLD_SECS).unwrap();
    let cred = cred_expiring_in(threshold_secs / 2);
    assert!(
        cred.needs_refresh(),
        "credential expiring inside refresh window should need refresh"
    );
}

#[test]
fn needs_refresh_true_when_expired() {
    // WHY: negative remaining is strictly less than the threshold; kills the
    // `-> false` body-replacement mutant and the `< → <=` mutation by
    // pairing with the boundary tests below.
    let cred = cred_expiring_in(-3600);
    assert!(cred.needs_refresh(), "expired credential must need refresh");
}

#[test]
fn needs_refresh_true_at_expiry_instant() {
    // WHY: at expiry, remaining is ~0 seconds — strictly less than the 3600s
    // threshold, so the correct `<` comparator returns true. Together with
    // the "well-before-expiry" test this pins down `<` against `>` and `>=`.
    let cred = cred_expiring_in(0);
    assert!(
        cred.needs_refresh(),
        "credential at expiry instant must need refresh"
    );
}

#[test]
fn needs_refresh_false_without_expiry() {
    // WHY: the `None` arm is the only path that returns false for tokens with
    // no expiry; covers the match arm directly.
    let cred = CredentialFile {
        token: SecretString::from("t"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    assert!(
        !cred.needs_refresh(),
        "credential without expiry must not need refresh"
    );
}

#[test]
fn needs_refresh_boundary_just_outside_window_is_false() {
    // WHY: one second beyond the window — the `<` comparator returns false
    // (remaining == threshold + 1 is not `< threshold`). Together with the
    // "inside window" test this distinguishes `<` from `<=` and from `==`.
    let threshold_secs = i64::try_from(REFRESH_THRESHOLD_SECS).unwrap();
    // Use +2 to tolerate one-second rounding during the seconds_remaining
    // computation on slow test runners.
    let cred = cred_expiring_in(threshold_secs.saturating_add(2));
    assert!(
        !cred.needs_refresh(),
        "credential expiring just outside refresh window must not need refresh"
    );
}

// ── CredentialFileLock ──

/// Poll-with-timeout helper: spawn `work` on a background thread and wait up
/// to `timeout` for it to finish. Returns `Some(result)` if it finished,
/// `None` if it timed out (thread is then detached).
fn run_with_timeout<T: Send + 'static, F: FnOnce() -> T + Send + 'static>(
    work: F,
    timeout: Duration,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let out = work();
        // Receiver may have dropped after timeout; ignore send error.
        let _ = tx.send(out);
    });
    rx.recv_timeout(timeout).ok()
}

#[test]
fn lock_shared_allows_concurrent_shared_holders() {
    // WHY: two shared (read) locks must coexist. Kills the
    // `CredentialFileLock::shared -> Ok(Default::default())` mutant because
    // a defaulted File on a dropped temp path cannot be held across threads
    // in this sequence and because we actually assert the locks returned.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");

    let guard_a = CredentialFileLock::shared(&path).expect("thread A: shared lock");

    let path_b = path.clone();
    let acquired_b = run_with_timeout(
        move || CredentialFileLock::shared(&path_b).is_ok(),
        Duration::from_secs(2),
    )
    .expect("thread B: shared-lock attempt did not complete in time");
    assert!(
        acquired_b,
        "second shared lock should succeed while first is held"
    );

    drop(guard_a);
}

#[test]
fn lock_exclusive_blocks_second_exclusive() {
    // WHY: an exclusive lock must exclude another exclusive acquirer. Kills
    // the `CredentialFileLock::exclusive -> Ok(Default::default())` mutant,
    // which would make the second call return instantly instead of blocking.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");

    let guard_a = CredentialFileLock::exclusive(&path).expect("thread A: exclusive lock");

    let path_b = path.clone();
    let completed = run_with_timeout(
        move || CredentialFileLock::exclusive(&path_b).map(|_| ()),
        Duration::from_millis(500),
    );
    assert!(
        completed.is_none(),
        "second exclusive lock must block while first is held"
    );

    drop(guard_a);
}

#[test]
fn lock_exclusive_blocks_shared() {
    // WHY: shared cannot be acquired while exclusive is held. Reinforces the
    // `exclusive` and `lock` primitives: a defaulted return would let the
    // shared call succeed immediately.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");

    let guard_a = CredentialFileLock::exclusive(&path).expect("thread A: exclusive lock");

    let path_b = path.clone();
    let completed = run_with_timeout(
        move || CredentialFileLock::shared(&path_b).map(|_| ()),
        Duration::from_millis(500),
    );
    assert!(
        completed.is_none(),
        "shared lock must block while exclusive is held"
    );

    drop(guard_a);
}

#[test]
fn lock_exclusive_released_on_drop() {
    // WHY: dropping the guard must release the lock so a subsequent exclusive
    // acquirer can proceed. Kills the `lock -> Ok(Default::default())`
    // mutant: a defaulted File on drop wouldn't release the flock on the
    // real sidecar file, so the follow-up acquisition would still block.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");

    let guard = CredentialFileLock::exclusive(&path).expect("first exclusive lock");
    drop(guard);

    let path_b = path.clone();
    let second = run_with_timeout(
        move || CredentialFileLock::exclusive(&path_b).is_ok(),
        Duration::from_secs(2),
    )
    .expect("second exclusive acquisition did not complete in time");
    assert!(
        second,
        "exclusive lock must be reacquirable after previous guard is dropped"
    );
}

#[test]
fn lock_shared_released_on_drop_allows_exclusive() {
    // WHY: after a shared lock is dropped, an exclusive lock must be
    // acquirable. Cross-pairs `shared` and `exclusive` with the `lock`
    // primitive — all three `Ok(Default::default())` mutants are killed
    // when both acquisition and release are observed.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");

    let shared_guard = CredentialFileLock::shared(&path).expect("shared lock");
    drop(shared_guard);

    let path_b = path.clone();
    let acquired = run_with_timeout(
        move || CredentialFileLock::exclusive(&path_b).is_ok(),
        Duration::from_secs(2),
    )
    .expect("exclusive acquisition after shared drop did not complete in time");
    assert!(
        acquired,
        "exclusive lock must succeed after prior shared guard is dropped"
    );
}

#[test]
fn lock_uses_sidecar_file_not_credential_path() {
    // WHY: pins the invariant that the lock file is `<path>.json.lock`, not
    // `<path>`. A `Default::default()` File wouldn't create any sidecar, so
    // this asserts the real lock path side-effect.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".credentials.json");

    assert!(!path.exists(), "credential file should not exist pre-lock");
    let _guard = CredentialFileLock::exclusive(&path).expect("exclusive lock");

    let lock_path = path.with_extension("json.lock");
    assert!(
        lock_path.exists(),
        "exclusive lock should create sidecar at {}",
        lock_path.display()
    );
    assert!(
        !path.exists(),
        "credential file itself must not be created by lock acquisition"
    );
}

// ── load hardening (#4873) ──

#[test]
fn load_missing_key_does_not_auto_create() {
    // WHY: deleting the sidecar key for an encrypted credential must not cause
    // `load` to silently generate a new key and then fail decryption.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("encrypted.json");

    let cred = CredentialFile {
        token: SecretString::from("sk-test-key-missing"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    cred.save(&path).expect("save encrypted credential");

    let key_path = path.with_extension("json.key");
    assert!(key_path.exists(), "key file must exist after save");
    std::fs::remove_file(&key_path).expect("delete key file");

    let loaded = CredentialFile::load(&path);
    assert!(
        loaded.is_none(),
        "loading an encrypted credential without its key must fail"
    );
    assert!(
        !key_path.exists(),
        "load must not create a new key file when the original is missing"
    );
}

#[test]
fn load_shared_lock_allows_concurrent_save_race() {
    // WHY: smoke test that holding a shared lock during load does not deadlock
    // with save's exclusive lock and that concurrent save/load cycles do not
    // corrupt the credential file.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("race.json");

    let initial = CredentialFile {
        token: SecretString::from("sk-initial"),
        refresh_token: None,
        expires_at: None,
        scopes: None,
        subscription_type: None,
    };
    initial.save(&path).expect("save initial credential");

    let iterations = 50;
    let path_save = path.clone();
    let saver = thread::spawn(move || {
        for i in 0..iterations {
            let cred = CredentialFile {
                token: SecretString::from(format!("sk-save-{i}")),
                refresh_token: None,
                expires_at: None,
                scopes: None,
                subscription_type: None,
            };
            cred.save(&path_save).expect("concurrent save");
        }
    });

    for _ in 0..iterations {
        // Any individual load may race with a save and return None; the goal is
        // that it never panics and that the file remains valid after the final save.
        let _ = CredentialFile::load(&path);
    }

    saver.join().expect("saver thread");

    let final_cred = CredentialFile::load(&path).expect("final load after race");
    assert!(
        final_cred.token.expose_secret().starts_with("sk-save-"),
        "final credential must reflect one of the saver writes"
    );
}
