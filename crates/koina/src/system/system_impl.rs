//! System operation trait implementations for filesystem, clock, and environment access.

use std::io;
use std::path::{Path, PathBuf};

#[cfg(any(test, feature = "test-support"))]
use std::collections::HashSet;

use jiff::{SignedDuration, Timestamp};

use super::{Clock, Environment, FileSystem, RealSystem};
#[cfg(any(test, feature = "test-support"))]
use super::TestSystem;

// ── RealSystem implementations ───────────────────────────────────────────────

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

// ── TestSystem implementations ───────────────────────────────────────────────

#[cfg(any(test, feature = "test-support"))]
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
        // WHY: create_dir_all semantics -- register the full ancestor chain.
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

#[cfg(any(test, feature = "test-support"))]
impl Clock for TestSystem {
    fn now(&self) -> Timestamp {
        self.clock
    }

    fn elapsed(&self, since: Timestamp) -> SignedDuration {
        self.clock.duration_since(since)
    }
}

#[cfg(any(test, feature = "test-support"))]
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
