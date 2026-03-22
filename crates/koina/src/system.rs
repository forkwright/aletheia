//! System operation traits for filesystem, clock, and environment access.
//!
//! Abstracts over real I/O so that code depending on these traits can be tested
//! without touching the disk, the real clock, or the process environment.
//!
//! ## Design
//!
//! Three orthogonal traits cover the most common system interactions:
//!
//! - [`FileSystem`](crate::system::FileSystem): read, write, query, and enumerate files and directories.
//! - [`Clock`](crate::system::Clock): obtain the current time and measure elapsed duration.
//! - [`Environment`](crate::system::Environment): read process environment variables and the working directory.
//!
//! [`RealSystem`](crate::system::RealSystem) implements all three using the standard library and OS syscalls.
//! [`TestSystem`](crate::system::TestSystem) implements all three in memory, giving tests full deterministic
//! control over every interaction.
//!
//! ## Usage
//!
//! Accept `impl FileSystem` (or `impl Clock`, `impl Environment`) in any function
//! that needs I/O, then pass `&RealSystem` in production and a pre-configured
//! `TestSystem` in tests.
//!
//! ```
//! use std::path::Path;
//! use aletheia_koina::system::{FileSystem, TestSystem};
//!
//! fn has_config(fs: &impl FileSystem, path: &Path) -> bool {
//!     fs.exists(path)
//! }
//!
//! let mut sys = TestSystem::new();
//! sys.add_file("/etc/aletheia.toml", b"port = 9000");
//! assert!(has_config(&sys, Path::new("/etc/aletheia.toml")), "config file must exist");
//! assert!(!has_config(&sys, Path::new("/etc/missing.toml")), "missing file must not exist");
//! ```

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jiff::{SignedDuration, Timestamp};

// ── FileSystem ────────────────────────────────────────────────────────────────

/// Abstraction over filesystem read, write, and query operations.
///
/// Implement this trait to substitute an in-memory store in tests instead of
/// touching the real disk.
///
/// All directory-creation methods use `create_dir_all` semantics (they create
/// the full ancestor chain as needed).
pub trait FileSystem: Send + Sync {
    /// Read the entire contents of a file.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::NotFound`] when the path does not exist, or an
    /// OS error when the file cannot be read.
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Write `contents` to `path`, creating or truncating the file.
    ///
    /// Parent directories are **not** created automatically. Call
    /// [`FileSystem::create_dir`] first when the directory may be absent.
    ///
    /// # Errors
    ///
    /// Returns an OS error if the file cannot be created or written.
    fn write_file(&self, path: &Path, contents: &[u8]) -> io::Result<()>;

    /// Return `true` if `path` exists (file or directory).
    #[must_use]
    fn exists(&self, path: &Path) -> bool;

    /// Return `true` if `path` is a regular file.
    #[must_use]
    fn is_file(&self, path: &Path) -> bool;

    /// Create `path` and all necessary parent directories.
    ///
    /// Succeeds silently when the directory already exists.
    ///
    /// # Errors
    ///
    /// Returns an OS error if any directory cannot be created.
    fn create_dir(&self, path: &Path) -> io::Result<()>;

    /// Return the immediate children of a directory.
    ///
    /// The returned paths are absolute (or use the same base as `path`).
    /// The order is unspecified.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::NotFound`] when `path` does not exist, or an
    /// OS error when the directory cannot be read.
    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;

    /// Remove a file.
    ///
    /// # Errors
    ///
    /// Returns an OS error if the file does not exist or cannot be removed.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Rename `from` to `to`, atomically on most platforms.
    ///
    /// # Errors
    ///
    /// Returns an OS error if the rename fails.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
}

// ── Clock ─────────────────────────────────────────────────────────────────────

/// Abstraction over the system clock.
///
/// Use [`RealSystem`] in production and a frozen [`TestSystem`] in tests to
/// obtain deterministic timestamps without sleeping.
pub trait Clock: Send + Sync {
    /// Return the current time as a [`Timestamp`].
    #[must_use]
    fn now(&self) -> Timestamp;

    /// Return the duration elapsed since `since`.
    ///
    /// For a frozen clock this is `self.now() - since`, which may be negative
    /// when `since` is in the future relative to the frozen instant.
    #[must_use]
    fn elapsed(&self, since: Timestamp) -> SignedDuration;
}

// ── Environment ───────────────────────────────────────────────────────────────

/// Abstraction over the process environment.
///
/// Use [`RealSystem`] in production and a pre-populated [`TestSystem`] in
/// tests to avoid polluting or reading the real process environment.
pub trait Environment: Send + Sync {
    /// Return the value of environment variable `name`, or `None` if unset.
    #[must_use]
    fn var(&self, name: &str) -> Option<String>;

    /// Return all environment variables as `(name, value)` pairs.
    #[must_use]
    fn vars(&self) -> Vec<(String, String)>;

    /// Return the current working directory.
    ///
    /// # Errors
    ///
    /// Returns an OS error if the working directory cannot be determined (e.g.
    /// because it has been deleted).
    fn current_dir(&self) -> io::Result<PathBuf>;
}

// ── RealSystem ────────────────────────────────────────────────────────────────

/// Production system implementation backed by the operating system.
///
/// Delegates every operation to the standard library or OS syscalls.
/// Construct once and pass by reference wherever a trait bound is required.
#[derive(Debug, Clone, Default)]
pub struct RealSystem;

impl FileSystem for RealSystem {
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn write_file(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        std::fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect()
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
}

impl Clock for RealSystem {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    fn elapsed(&self, since: Timestamp) -> SignedDuration {
        Timestamp::now().duration_since(since)
    }
}

impl Environment for RealSystem {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn vars(&self) -> Vec<(String, String)> {
        std::env::vars().collect()
    }

    fn current_dir(&self) -> io::Result<PathBuf> {
        std::env::current_dir()
    }
}

// ── TestSystem ────────────────────────────────────────────────────────────────

/// In-memory system implementation for deterministic testing.
///
/// - All filesystem operations target an in-memory `HashMap`.
/// - The clock is frozen at construction time (default: [`Timestamp::UNIX_EPOCH`]).
/// - Environment variables are served from a provided `HashMap`.
///
/// # Example
///
/// ```
/// use std::path::Path;
/// use aletheia_koina::system::{Clock, Environment, FileSystem, TestSystem};
/// use jiff::Timestamp;
///
/// let sys = TestSystem::new()
///     .with_clock(Timestamp::UNIX_EPOCH)
///     .with_env("HOME", "/home/alice");
///
/// assert_eq!(sys.now(), Timestamp::UNIX_EPOCH, "clock must return configured time");
/// assert_eq!(sys.var("HOME").as_deref(), Some("/home/alice"), "env var must match");
/// assert!(!sys.exists(Path::new("/etc/missing")), "path must not exist");
/// ```
#[derive(Debug, Clone)]
pub struct TestSystem {
    /// Byte contents of virtual files, keyed by absolute path.
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    /// Set of virtual directories (populated automatically on write/add).
    dirs: Arc<Mutex<HashSet<PathBuf>>>,
    /// Frozen clock value.
    clock: Timestamp,
    /// Fake environment variables.
    env: HashMap<String, String>,
}

impl TestSystem {
    /// Create an empty [`TestSystem`] with the clock frozen at the Unix epoch.
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            dirs: Arc::new(Mutex::new(HashSet::new())),
            clock: Timestamp::UNIX_EPOCH,
            env: HashMap::new(),
        }
    }

    /// Set the frozen clock to `ts` (builder pattern).
    #[must_use]
    pub fn with_clock(mut self, ts: Timestamp) -> Self {
        self.clock = ts;
        self
    }

    /// Add an environment variable (builder pattern).
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Pre-populate a virtual file with `contents`.
    ///
    /// Parent directories are automatically registered so that [`exists`] and
    /// [`list_dir`](FileSystem::list_dir) work without calling [`create_dir`](FileSystem::create_dir) first.
    ///
    /// [`exists`]: FileSystem::exists
    pub fn add_file(&mut self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) {
        let path = path.into();
        self.register_ancestors(&path);
        self.files_guard().insert(path, contents.into());
    }

    /// Add (or overwrite) an environment variable after construction.
    pub fn add_env(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.env.insert(key.into(), value.into());
    }

    /// Return all virtual file paths.
    #[must_use]
    pub fn file_paths(&self) -> Vec<PathBuf> {
        self.files_guard().keys().cloned().collect()
    }

    /// Return the content of a virtual file, or `None` if absent.
    #[must_use]
    pub fn get_file(&self, path: &Path) -> Option<Vec<u8>> {
        self.files_guard().get(path).cloned()
    }

    /// Register all ancestor directories of `path` in the dirs set.
    fn register_ancestors(&self, path: &Path) {
        let mut dirs = self.dirs_guard();
        let mut current = path.to_path_buf();
        while let Some(parent) = current.parent() {
            if parent == current {
                break;
            }
            dirs.insert(parent.to_path_buf());
            current = parent.to_path_buf();
        }
    }

    /// Acquire the files lock.
    ///
    /// Recovers from mutex poisoning by accepting the potentially-inconsistent
    /// state. In a test-only type, this avoids cascading panics while still
    /// letting the original failure propagate through test assertions.
    fn files_guard(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Vec<u8>>> {
        self.files
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Acquire the dirs lock.
    ///
    /// See [`files_guard`](TestSystem::files_guard) for rationale.
    fn dirs_guard(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        self.dirs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for TestSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for TestSystem {
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.files_guard()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn write_file(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let path = path.to_path_buf();
        self.register_ancestors(&path);
        self.files_guard().insert(path, contents.to_vec());
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        if self.files_guard().contains_key(path) {
            return true;
        }
        self.dirs_guard().contains(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files_guard().contains_key(path)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        // WHY: create_dir_all semantics — register the full ancestor chain.
        let path = path.to_path_buf();
        let mut dirs = self.dirs_guard();
        let mut current = path.clone();
        loop {
            dirs.insert(current.clone());
            match current.parent() {
                Some(parent) if parent != current => current = parent.to_path_buf(),
                _ => break,
            }
        }
        Ok(())
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let files = self.files_guard();
        let dirs = self.dirs_guard();

        let dir_known = dirs.contains(path);
        let has_children = files.keys().any(|f| f.parent() == Some(path))
            || dirs.iter().any(|d| d.parent() == Some(path));

        if !dir_known && !has_children {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            ));
        }

        let mut children: HashSet<PathBuf> = HashSet::new();
        for f in files.keys() {
            if f.parent() == Some(path) {
                children.insert(f.clone());
            }
        }
        for d in dirs.iter() {
            if d.parent() == Some(path) {
                children.insert(d.clone());
            }
        }

        Ok(children.into_iter().collect())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let removed = self.files_guard().remove(path).is_some();
        if removed {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            ))
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let contents = self
            .files_guard()
            .remove(from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, from.display().to_string()))?;
        self.register_ancestors(to);
        self.files_guard().insert(to.to_path_buf(), contents);
        Ok(())
    }
}

impl Clock for TestSystem {
    fn now(&self) -> Timestamp {
        self.clock
    }

    fn elapsed(&self, since: Timestamp) -> SignedDuration {
        self.clock.duration_since(since)
    }
}

impl Environment for TestSystem {
    fn var(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }

    fn vars(&self) -> Vec<(String, String)> {
        self.env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from("/test"))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::path::Path;

    use jiff::Timestamp;

    use super::*;

    // ── FileSystem ────────────────────────────────────────────────────────

    #[test]
    fn read_missing_file_returns_not_found() {
        let sys = TestSystem::new();
        let err = sys.read_file(Path::new("/no/such/file")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn write_and_read_roundtrip() {
        let sys = TestSystem::new();
        sys.write_file(Path::new("/tmp/hello"), b"world").unwrap();
        let got = sys.read_file(Path::new("/tmp/hello")).unwrap();
        assert_eq!(got, b"world");
    }

    #[test]
    fn write_overwrites_existing_content() {
        let sys = TestSystem::new();
        sys.write_file(Path::new("/tmp/f"), b"first").unwrap();
        sys.write_file(Path::new("/tmp/f"), b"second").unwrap();
        let got = sys.read_file(Path::new("/tmp/f")).unwrap();
        assert_eq!(got, b"second");
    }

    #[test]
    fn exists_true_after_write() {
        let sys = TestSystem::new();
        sys.write_file(Path::new("/tmp/x"), b"").unwrap();
        assert!(sys.exists(Path::new("/tmp/x")));
    }

    #[test]
    fn exists_false_for_missing_path() {
        let sys = TestSystem::new();
        assert!(!sys.exists(Path::new("/no/such/path")));
    }

    #[test]
    fn is_file_true_for_written_path() {
        let sys = TestSystem::new();
        sys.write_file(Path::new("/tmp/f"), b"data").unwrap();
        assert!(sys.is_file(Path::new("/tmp/f")));
    }

    #[test]
    fn is_file_false_for_directory() {
        let sys = TestSystem::new();
        sys.create_dir(Path::new("/some/dir")).unwrap();
        assert!(!sys.is_file(Path::new("/some/dir")));
    }

    #[test]
    fn is_file_false_for_missing_path() {
        let sys = TestSystem::new();
        assert!(!sys.is_file(Path::new("/missing")));
    }

    #[test]
    fn create_dir_makes_directory_visible() {
        let sys = TestSystem::new();
        sys.create_dir(Path::new("/srv/data")).unwrap();
        assert!(sys.exists(Path::new("/srv/data")));
        assert!(sys.exists(Path::new("/srv")));
    }

    #[test]
    fn list_dir_returns_direct_children() {
        let mut sys = TestSystem::new();
        sys.add_file("/base/a.txt", b"a");
        sys.add_file("/base/b.txt", b"b");
        let mut entries = sys.list_dir(Path::new("/base")).unwrap();
        entries.sort();
        assert_eq!(
            entries,
            vec![PathBuf::from("/base/a.txt"), PathBuf::from("/base/b.txt"),]
        );
    }

    #[test]
    fn list_dir_returns_not_found_for_missing_dir() {
        let sys = TestSystem::new();
        let err = sys.list_dir(Path::new("/nonexistent")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn remove_file_deletes_content() {
        let sys = TestSystem::new();
        sys.write_file(Path::new("/tmp/del"), b"bye").unwrap();
        sys.remove_file(Path::new("/tmp/del")).unwrap();
        assert!(!sys.exists(Path::new("/tmp/del")));
    }

    #[test]
    fn remove_file_on_missing_returns_not_found() {
        let sys = TestSystem::new();
        let err = sys.remove_file(Path::new("/tmp/gone")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn rename_moves_file() {
        let sys = TestSystem::new();
        sys.write_file(Path::new("/tmp/src"), b"payload").unwrap();
        sys.rename(Path::new("/tmp/src"), Path::new("/tmp/dst"))
            .unwrap();
        assert!(!sys.exists(Path::new("/tmp/src")));
        assert_eq!(sys.read_file(Path::new("/tmp/dst")).unwrap(), b"payload");
    }

    #[test]
    fn rename_missing_source_returns_not_found() {
        let sys = TestSystem::new();
        let err = sys
            .rename(Path::new("/no/src"), Path::new("/no/dst"))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn add_file_registers_parent_dirs() {
        let mut sys = TestSystem::new();
        sys.add_file("/deep/nested/file.txt", b"x");
        assert!(sys.exists(Path::new("/deep/nested")));
        assert!(sys.exists(Path::new("/deep")));
    }

    #[test]
    fn get_file_returns_content_when_present() {
        let mut sys = TestSystem::new();
        sys.add_file("/tmp/q", b"query");
        assert_eq!(sys.get_file(Path::new("/tmp/q")).unwrap(), b"query");
    }

    #[test]
    fn get_file_returns_none_when_absent() {
        let sys = TestSystem::new();
        assert!(sys.get_file(Path::new("/absent")).is_none());
    }

    // ── Clock ─────────────────────────────────────────────────────────────

    #[test]
    fn frozen_clock_returns_configured_time() {
        let ts: Timestamp = "2025-06-15T12:00:00Z".parse().unwrap();
        let sys = TestSystem::new().with_clock(ts);
        assert_eq!(sys.now(), ts);
    }

    #[test]
    fn elapsed_with_frozen_clock_is_deterministic() {
        let base: Timestamp = "2025-01-01T00:00:00Z".parse().unwrap();
        let later: Timestamp = "2025-01-01T01:00:00Z".parse().unwrap();
        let sys = TestSystem::new().with_clock(later);
        let dur = sys.elapsed(base);
        assert_eq!(dur.as_secs(), 3600);
    }

    #[test]
    fn default_clock_is_unix_epoch() {
        let sys = TestSystem::new();
        assert_eq!(sys.now(), Timestamp::UNIX_EPOCH);
    }

    // ── Environment ───────────────────────────────────────────────────────

    #[test]
    fn var_returns_configured_value() {
        let sys = TestSystem::new().with_env("ALETHEIA_ROOT", "/srv/aletheia");
        assert_eq!(sys.var("ALETHEIA_ROOT").as_deref(), Some("/srv/aletheia"));
    }

    #[test]
    fn var_returns_none_for_unset_key() {
        let sys = TestSystem::new();
        assert!(sys.var("UNSET_VAR_XYZ").is_none());
    }

    #[test]
    fn vars_returns_all_entries() {
        let sys = TestSystem::new().with_env("A", "1").with_env("B", "2");
        let mut pairs = sys.vars();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned()),
            ]
        );
    }

    #[test]
    fn current_dir_returns_test_path() {
        let sys = TestSystem::new();
        assert_eq!(sys.current_dir().unwrap(), PathBuf::from("/test"));
    }

    #[test]
    fn add_env_after_construction() {
        let mut sys = TestSystem::new();
        sys.add_env("LATE", "value");
        assert_eq!(sys.var("LATE").as_deref(), Some("value"));
    }

    // ── Trait-object compatibility ─────────────────────────────────────────

    #[test]
    fn filesystem_trait_object_compiles() {
        let sys: Box<dyn FileSystem> = Box::new(TestSystem::new());
        assert!(!sys.exists(Path::new("/any")));
    }

    #[test]
    fn clock_trait_object_compiles() {
        let sys: Box<dyn Clock> = Box::new(TestSystem::new());
        assert_eq!(sys.now(), Timestamp::UNIX_EPOCH);
    }

    #[test]
    fn environment_trait_object_compiles() {
        let sys: Box<dyn Environment> = Box::new(TestSystem::new());
        assert!(sys.var("NONE").is_none());
    }
}
