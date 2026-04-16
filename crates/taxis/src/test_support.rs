//! Test helpers for env-var isolation and temp-dir fixtures.
//!
//! `EnvJail` replaces `figment::Jail` after the figment removal (#3447): it
//! provides a serialised, RAII-scoped fixture that captures any env vars the
//! test touches and restores them on drop, alongside a fresh temp directory.
//!
//! Because `std::env::set_var` is process-wide and Cargo runs tests within a
//! binary in parallel threads, all jails lock a single global mutex for the
//! lifetime of the jail. Tests inside a jail are effectively serialised.

#![cfg(any(test, feature = "test-support"))]
#![expect(
    clippy::expect_used,
    reason = "test helper: failure to create a temp dir or hold a lock is a test harness bug"
)]
#![expect(
    unsafe_code,
    reason = "std::env::{set_var,remove_var} are unsafe in edition 2024; serialised via the jail lock"
)]
#![expect(
    clippy::disallowed_methods,
    reason = "test helper: std::fs::write seeds real files inside the jail's temp dir"
)]

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tempfile::TempDir;

/// Global lock that serialises jail scopes. Cargo runs tests in parallel
/// threads and `std::env` is process-wide, so without this lock one jail's
/// env mutations would bleed into another.
static LOCK: Mutex<()> = Mutex::new(());

/// RAII fixture that owns a fresh temp directory and restores any env vars
/// it set (or cleared) when it is dropped.
///
/// WHY: replaces `figment::Jail` after #3447 (figment replacement). The jail
/// protects a test's env-var mutations from leaking to sibling tests.
pub struct EnvJail {
    _lock: MutexGuard<'static, ()>,
    dir: TempDir,
    saved: HashMap<OsString, Option<OsString>>,
    canonical_dir: PathBuf,
}

impl EnvJail {
    /// Create a fresh jail with its own temp directory and env-var scope.
    ///
    /// Blocks until the process-wide jail lock is acquired.
    #[must_use]
    pub fn new() -> Self {
        let lock = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = TempDir::new().expect("create jail temp dir");
        let canonical_dir = dir
            .path()
            .canonicalize()
            .expect("canonicalize jail temp dir");
        Self {
            _lock: lock,
            dir,
            saved: HashMap::new(),
            canonical_dir,
        }
    }

    /// Return the jail's working directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.canonical_dir
    }

    /// Set an env var for the duration of the jail. The original value (or
    /// absence) is restored when the jail is dropped.
    pub fn set_env<K: AsRef<str>, V: AsRef<str>>(&mut self, key: K, value: V) {
        let key = key.as_ref();
        let key_os = OsString::from(key);
        if !self.saved.contains_key(OsStr::new(key)) {
            self.saved.insert(key_os.clone(), std::env::var_os(key));
        }
        // SAFETY: the global LOCK held by `_lock` serialises all EnvJail scopes,
        // and no test outside an EnvJail mutates env vars we care about.
        unsafe {
            std::env::set_var(key_os, value.as_ref());
        }
    }

    /// Remove an env var for the duration of the jail. The original value is
    /// restored when the jail is dropped.
    pub fn remove_env<K: AsRef<str>>(&mut self, key: K) {
        let key = key.as_ref();
        let key_os = OsString::from(key);
        if !self.saved.contains_key(OsStr::new(key)) {
            self.saved.insert(key_os.clone(), std::env::var_os(key));
        }
        // SAFETY: serialised via LOCK held by `_lock`.
        unsafe {
            std::env::remove_var(key_os);
        }
    }

    /// Create a file inside the jail's directory. Parents are created as
    /// needed. Panics on I/O failure because this is test-harness code.
    pub fn create_file<P: AsRef<Path>>(&self, rel: P, contents: &str) {
        let full = self.dir.path().join(rel.as_ref());
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&full, contents).expect("write jail file");
    }
}

impl Default for EnvJail {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EnvJail {
    fn drop(&mut self) {
        for (key, original) in self.saved.drain() {
            match original {
                // SAFETY: the jail lock is still held until the MutexGuard in
                // `_lock` is dropped, which happens after this loop completes.
                Some(val) => unsafe { std::env::set_var(&key, val) },
                None => unsafe { std::env::remove_var(&key) },
            }
        }
    }
}
