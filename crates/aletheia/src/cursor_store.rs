//! Fjall-backed adapter for agora's [`CursorStore`] trait (#7104).
//!
//! Agora is a message-routing layer and owns no persistence (see
//! `crates/agora/clippy.toml`), so the runtime provides the durable half of
//! Matrix `/sync` cursor resumption: one record per channel+account in a
//! small fjall keyspace under the instance data directory, the same shared
//! state mechanism (`koina::fjall`) the other stores build on.
//!
//! Account identifiers never appear in an on-disk path: the keyspace
//! directory has a fixed name and accounts are record keys inside the LSM
//! store's segment files. Account IDs are phone numbers and Matrix user IDs,
//! and paths leak into directory listings, backups, and any error message
//! that prints a path.

use std::io;
use std::path::Path;

use fjall::{
    KeyspaceCreateOptions, PersistMode, Readable as _, SingleWriterTxDatabase,
    SingleWriterTxKeyspace,
};
use serde::{Deserialize, Serialize};

use agora::cursor::CursorStore;

/// Directory name for the channel cursor keyspace under `oikos.data()`.
pub(crate) const CURSOR_DB_DIR: &str = "channel-cursors.fjall";

/// Partition holding one JSON [`CursorDocument`] per channel+account.
const PARTITION: &str = "cursors";

/// Fjall-backed store for provider resumption cursors.
pub(crate) struct FjallCursorStore {
    db: SingleWriterTxDatabase,
}

impl FjallCursorStore {
    /// Open (or create) the cursor keyspace at `path`.
    ///
    /// `path` is a directory; fjall manages its own files within it.
    ///
    /// # Errors
    ///
    /// Returns an error when the keyspace cannot be opened or the `cursors`
    /// partition cannot be initialised.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let fdb = koina::fjall::FjallDb::open(path, &[PARTITION]).map_err(io::Error::other)?;
        Ok(Self { db: fdb.db })
    }

    fn partition(&self) -> io::Result<SingleWriterTxKeyspace> {
        self.db
            .keyspace(PARTITION, KeyspaceCreateOptions::default)
            .map_err(|e| io::Error::other(format!("fjall partition {PARTITION}: {e}")))
    }
}

impl CursorStore for FjallCursorStore {
    fn load(&self, channel: &str, account: &str) -> io::Result<Option<String>> {
        let partition = self.partition()?;
        let snap = self.db.read_tx();
        let Some(value) = snap
            .get(&partition, record_key(channel, account))
            .map_err(|e| io::Error::other(format!("fjall get cursor: {e}")))?
        else {
            return Ok(None);
        };
        // WHY InvalidData rather than None: a record that exists but cannot
        // be decoded is corruption, and corruption must not look like a
        // fresh account — starting without the cursor replays batches this
        // instance already accepted (#7104).
        let document: CursorDocument = serde_json::from_slice(&value)
            .map_err(|source| io::Error::new(io::ErrorKind::InvalidData, source))?;
        validate_cursor(&document.cursor)?;
        Ok(Some(document.cursor))
    }

    fn save(&self, channel: &str, account: &str, cursor: &str) -> io::Result<()> {
        validate_cursor(cursor)?;
        let partition = self.partition()?;
        let document = CursorDocument {
            cursor: cursor.to_owned(),
        };
        let contents = serde_json::to_vec(&document).map_err(io::Error::other)?;

        let mut tx = self.db.write_tx();
        tx.insert(&partition, record_key(channel, account), contents);
        tx.commit()
            .map_err(|e| io::Error::other(format!("fjall commit cursor: {e}")))?;

        // WHY(#7104): without an explicit fsync the committed cursor sits in
        // the OS page cache; a crash before the next fjall flush would make
        // the following start resume from a stale token and replay the batch
        // this record was checkpointing. Cursor writes are one per accepted
        // sync batch, so the synchronous fsync cost is acceptable (same
        // reasoning as daemon's task-state store, #5752).
        self.db
            .persist(PersistMode::SyncAll)
            .map_err(|e| io::Error::other(format!("fjall persist cursor: {e}")))?;
        Ok(())
    }
}

/// Record key for a channel+account pair.
///
/// A NUL separator keeps the pair unambiguous: neither channel IDs nor
/// account identifiers contain NUL.
fn record_key(channel: &str, account: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(channel.len() + 1 + account.len());
    key.extend_from_slice(channel.as_bytes());
    key.push(0);
    key.extend_from_slice(account.as_bytes());
    key
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn open_store(root: &Path) -> FjallCursorStore {
        FjallCursorStore::open(&root.join(CURSOR_DB_DIR)).expect("open cursor store")
    }

    #[test]
    fn save_then_load_roundtrips_and_overwrites() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let store = open_store(tmp.path());
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
    fn cursor_survives_store_reopen() {
        // WHY(#7104): this test simulates a restart. The store must fsync on
        // save so the cursor is still present after the old handle is
        // dropped and a new one opens the same directory.
        let tmp = tempfile::tempdir().expect("tmpdir");
        {
            let store = open_store(tmp.path());
            store.save("matrix", "primary", "s-1").expect("save");
        }

        let reopened = open_store(tmp.path());
        assert_eq!(
            reopened.load("matrix", "primary").expect("load").as_deref(),
            Some("s-1")
        );
    }

    #[test]
    fn cursors_are_scoped_per_channel_and_account() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let store = open_store(tmp.path());
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
    fn account_ids_do_not_appear_in_on_disk_paths() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let store = open_store(tmp.path());
        store
            .save("matrix", "@bot:example.org", "s-1")
            .expect("save");
        drop(store);

        let mut pending = vec![tmp.path().to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).expect("read dir") {
                let entry = entry.expect("entry");
                let name = entry.file_name();
                let name = name.to_string_lossy();
                assert!(
                    !name.contains('@') && !name.contains("bot"),
                    "account id must not appear in any on-disk path component: {name}"
                );
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                }
            }
        }
    }

    #[test]
    fn corrupt_record_fails_closed() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let db_path = tmp.path().join(CURSOR_DB_DIR);
        {
            let store = open_store(tmp.path());
            store.save("matrix", "primary", "s-1").expect("save");
        }

        // Overwrite the record with garbage through a raw handle, as disk
        // corruption would.
        {
            let fdb = koina::fjall::FjallDb::open(&db_path, &[PARTITION]).expect("raw open");
            let partition = fdb
                .db
                .keyspace(PARTITION, KeyspaceCreateOptions::default)
                .expect("partition");
            let mut tx = fdb.db.write_tx();
            tx.insert(&partition, record_key("matrix", "primary"), b"{not json");
            tx.commit().expect("commit");
            fdb.db.persist(PersistMode::SyncAll).expect("persist");
        }

        let store = open_store(tmp.path());
        let error = store
            .load("matrix", "primary")
            .expect_err("corruption must not look like absence");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn empty_cursor_is_rejected_without_replacing_last_checkpoint() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let store = open_store(tmp.path());
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
