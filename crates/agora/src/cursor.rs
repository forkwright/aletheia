//! Per-account provider cursor persistence.
//!
//! Matrix `/sync` cursors survive process restarts when a store is wired in,
//! so a restart resumes after the last accepted batch instead of replaying
//! it. Signal needs no cursor: signal-cli's `receive` consumes messages
//! destructively on the daemon side.

use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::fs::File;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Where providers keep their resumption cursors.
pub trait CursorStore: Send + Sync {
    /// Last persisted cursor for this channel+account, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when a present cursor cannot be read or validated.
    /// Absence alone returns `Ok(None)`.
    fn load(&self, channel: &str, account: &str) -> io::Result<Option<String>>;

    /// Persist the cursor before the provider accepts the corresponding batch.
    ///
    /// # Errors
    ///
    /// Returns an error when any directory, serialization, write, rename, or
    /// durability step fails.
    fn save(&self, channel: &str, account: &str, cursor: &str) -> io::Result<()>;
}

/// JSON-file cursor store: one file per channel+account under `dir`.
///
/// File names are a SHA-256 digest of the channel+account pair, not the
/// account ID itself — account IDs are phone numbers or Matrix user IDs and
/// must not leak into filesystem paths.
#[derive(Debug)]
pub struct FileCursorStore {
    dir: PathBuf,
}

impl FileCursorStore {
    /// Store cursors under `dir`; created lazily on first save.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path_for(&self, channel: &str, account: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(channel.as_bytes());
        hasher.update(b"\0");
        hasher.update(account.as_bytes());
        let digest = hasher.finalize();
        self.dir
            .join(format!("{}.json", crate::types::hex_lower(&digest)))
    }

    // WHY the suppression: agora's clippy policy asks for either `tokio::fs` or
    // "abstract behind a trait for testability". The second is what this file does --
    // `CursorStore` is the abstraction and `FileCursorStore` is one implementation of
    // it, consumed everywhere as `Arc<dyn CursorStore>`. The trait is synchronous, so
    // moving to `tokio::fs` would make every caller async for no behavioural gain.
    //
    // The larger question this does NOT settle: the crate header says all persistence
    // belongs in mneme, which argues this store should not live in agora at all. That
    // is a placement decision, not a lint decision, and is raised on the PR rather
    // than resolved by this attribute.
    #[expect(
        clippy::disallowed_methods,
        reason = "CursorStore is the trait abstraction the policy asks for; the trait is sync"
    )]
    fn try_load(&self, channel: &str, account: &str) -> io::Result<Option<String>> {
        let path = self.path_for(channel, account);
        let contents = match std::fs::read(&path) {
            Ok(contents) => contents,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(source),
        };
        let document: CursorDocument = serde_json::from_slice(&contents)
            .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
        validate_cursor(&document.cursor)?;
        Ok(Some(document.cursor))
    }

    fn try_save(&self, channel: &str, account: &str, cursor: &str) -> io::Result<()> {
        validate_cursor(cursor)?;
        create_dir_all_durable(&self.dir)?;
        let path = self.path_for(channel, account);
        let document = CursorDocument {
            cursor: cursor.to_owned(),
        };
        let contents = serde_json::to_vec(&document).map_err(io::Error::other)?;

        // WHY: Bathron owns the tempfile, file-fsync, rename, and directory-
        // fsync sequence. `create_dir_all_durable` separately makes the first
        // `channel-cursors/` entry durable in its owning data directory.
        bathron::atomic::write_atomic(&path, &contents, Some(0o600)).map_err(io::Error::other)?;
        Ok(())
    }
}

impl CursorStore for FileCursorStore {
    fn load(&self, channel: &str, account: &str) -> io::Result<Option<String>> {
        self.try_load(channel, account)
    }

    fn save(&self, channel: &str, account: &str, cursor: &str) -> io::Result<()> {
        self.try_save(channel, account, cursor)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CursorDocument {
    cursor: String,
}

fn validate_cursor(cursor: &str) -> io::Result<()> {
    if cursor.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "provider cursor is empty",
        ));
    }
    Ok(())
}

fn create_dir_all_durable(path: &Path) -> io::Result<()> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            return match usable_parent(path) {
                Ok(parent) => sync_directory(parent),
                Err(_) if path.has_root() => Ok(()),
                Err(source) => Err(source),
            };
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "cursor directory path is not a directory",
            ));
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(source),
    }

    let parent = usable_parent(path)?;
    create_dir_all_durable(parent)?;
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => {}
        Err(source) => return Err(source),
    }

    // WHY: creating `path` changes its parent's directory entry. Bathron's
    // file replace later fsyncs `path`, but only this fsync makes the new
    // directory itself reachable after a power loss.
    sync_directory(parent)
}

fn usable_parent(path: &Path) -> io::Result<&Path> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent),
        _ if path.is_relative() => Ok(Path::new(".")),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cursor directory has no parent",
        )),
    }
}

#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "fsync of the containing directory has no tokio::fs equivalent; durability step behind CursorStore"
)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path).and_then(|directory| directory.sync_all())
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "signature matches the unix durability operation, which can fail"
)]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().join("channel-cursors"));
        assert_eq!(store.load("matrix", "primary").expect("load"), None);

        store.save("matrix", "primary", "s-123").expect("save");
        assert_eq!(
            store.load("matrix", "primary").expect("load").as_deref(),
            Some("s-123")
        );

        store.save("matrix", "primary", "s-124").expect("save");
        assert_eq!(
            store.load("matrix", "primary").expect("load").as_deref(),
            Some("s-124")
        );
    }

    #[test]
    fn cursors_are_scoped_per_channel_and_account() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().to_path_buf());
        store.save("matrix", "primary", "s-1").expect("save");
        store.save("matrix", "secondary", "s-2").expect("save");
        assert_eq!(
            store.load("matrix", "primary").expect("load").as_deref(),
            Some("s-1")
        );
        assert_eq!(
            store.load("matrix", "secondary").expect("load").as_deref(),
            Some("s-2")
        );
        assert_eq!(store.load("signal", "primary").expect("load"), None);
    }

    #[test]
    fn file_names_do_not_carry_raw_account_ids() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().to_path_buf());
        store
            .save("matrix", "@bot:example.org", "s-1")
            .expect("save");
        for entry in std::fs::read_dir(dir.path()).expect("read dir") {
            let name = entry.expect("entry").file_name();
            let name = name.to_string_lossy();
            assert!(
                !name.contains('@') && !name.contains("bot"),
                "account id must not appear in the cursor file name: {name}"
            );
        }
    }

    #[test]
    fn corrupt_cursor_file_fails_closed() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().to_path_buf());
        store.save("matrix", "primary", "s-1").expect("save");
        let path = store.path_for("matrix", "primary");
        std::fs::write(&path, "{not json").expect("corrupt");
        let error = store
            .load("matrix", "primary")
            .expect_err("corruption must not look like absence");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn empty_cursor_is_rejected_without_replacing_last_checkpoint() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().to_path_buf());
        store.save("matrix", "primary", "s-1").expect("save");

        let error = store
            .save("matrix", "primary", "  ")
            .expect_err("empty cursor");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            store.load("matrix", "primary").expect("load").as_deref(),
            Some("s-1")
        );
    }
}
