//! Per-account provider cursor persistence.
//!
//! Matrix `/sync` cursors survive process restarts when a store is wired in,
//! so a restart resumes after the last processed batch instead of replaying
//! it. Signal needs no cursor: signal-cli's `receive` consumes messages
//! destructively on the daemon side.

use std::path::PathBuf;

use sha2::{Digest as _, Sha256};

/// Where providers keep their resumption cursors.
pub trait CursorStore: Send + Sync {
    /// Last persisted cursor for this channel+account, if any.
    fn load(&self, channel: &str, account: &str) -> Option<String>;
    /// Persist the cursor. Best-effort: implementations log and drop
    /// errors, because a lost cursor replays a bounded window (covered by
    /// the dedupe filter) while a failed save must never stop ingress.
    fn save(&self, channel: &str, account: &str, cursor: &str);
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

    fn try_save(&self, channel: &str, account: &str, cursor: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path_for(channel, account);
        // WHY: write-then-rename keeps a torn write from corrupting the last
        // good cursor — a partial file would otherwise read as garbage and
        // silently replay from the beginning.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::json!({ "cursor": cursor }).to_string())?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}

impl CursorStore for FileCursorStore {
    fn load(&self, channel: &str, account: &str) -> Option<String> {
        let path = self.path_for(channel, account);
        let contents = std::fs::read_to_string(&path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
        value
            .get("cursor")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
    }

    fn save(&self, channel: &str, account: &str, cursor: &str) {
        if let Err(e) = self.try_save(channel, account, cursor) {
            // NOTE: account is deliberately not logged — it is a phone
            // number or Matrix user ID.
            tracing::warn!(error = %e, channel, "failed to persist channel cursor");
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().to_path_buf());
        assert_eq!(store.load("matrix", "primary"), None);

        store.save("matrix", "primary", "s-123");
        assert_eq!(store.load("matrix", "primary").as_deref(), Some("s-123"));

        store.save("matrix", "primary", "s-124");
        assert_eq!(store.load("matrix", "primary").as_deref(), Some("s-124"));
    }

    #[test]
    fn cursors_are_scoped_per_channel_and_account() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().to_path_buf());
        store.save("matrix", "primary", "s-1");
        store.save("matrix", "secondary", "s-2");
        assert_eq!(store.load("matrix", "primary").as_deref(), Some("s-1"));
        assert_eq!(store.load("matrix", "secondary").as_deref(), Some("s-2"));
        assert_eq!(store.load("signal", "primary"), None);
    }

    #[test]
    fn file_names_do_not_carry_raw_account_ids() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().to_path_buf());
        store.save("matrix", "@bot:example.org", "s-1");
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
    fn corrupt_cursor_file_reads_as_absent() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store = FileCursorStore::new(dir.path().to_path_buf());
        store.save("matrix", "primary", "s-1");
        let path = store.path_for("matrix", "primary");
        std::fs::write(&path, "{not json").expect("corrupt");
        assert_eq!(store.load("matrix", "primary"), None);
    }
}
