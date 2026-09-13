//! Fjall-backed working checkpoint store.
//!
//! Partition: `working_checkpoints`
//! Key pattern: `nous:working_checkpoint:{session_id}:{turn_id}`, where
//! `turn_id` is the canonical `TurnEventIdentity::turn_id` ULID (#4853).
//! Value: JSON-encoded [`WorkingCheckpointRecord`]
//!
//! Per `feedback_fjall_iter_truncate_pitfall.md`: never collect
//! `prefix(p).iter()` before truncating; use `.rev().take(N)`.
//!
//! WHY(#4853) ULID over ordinal: before this, the key suffix was
//! `{turn_number:020}` (`turn_number` zero-padded to 20 ASCII digits) so
//! lexicographic key order tracked numeric order. That was a second,
//! disconnected turn representation instead of the canonical
//! `TurnEventIdentity`. ULIDs are natively lexicographically sortable by
//! creation time, so keying by `turn_id` gets the same correctly-ordered
//! `prune_old`/`read_latest`/`read_recent` behavior from the identity that
//! already exists, with no zero-padding hack. See
//! [`FjallWorkingCheckpointStore::migrate_legacy_ordinal_keys`] for the
//! one-time migration away from the old key shape.

use std::path::Path;
use std::sync::{Arc, Mutex};

use fjall::{KeyspaceCreateOptions, Readable, SingleWriterTxDatabase};
use jiff::Timestamp;
use koina::ulid::Ulid;

use crate::error;

/// Partition used for working checkpoints.
const PARTITION: &str = "working_checkpoints";

/// Maximum number of checkpoints to retain per session.
///
/// WHY: `read_latest`/`read_recent` only need recent entries, so older
/// checkpoints are pruned on write to bound disk usage. (#5707)
const CHECKPOINT_KEEP_N: usize = 20;

/// Internal record stored in fjall.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WorkingCheckpointRecord {
    session_id: String,
    turn_number: u64,
    content: String,
    created_at: String,
}

/// Fjall-backed implementation of [`organon::types::WorkingCheckpointStore`].
pub struct FjallWorkingCheckpointStore {
    db: Arc<SingleWriterTxDatabase>,
    write_lock: Mutex<()>,
    _temp_dir: Option<tempfile::TempDir>,
}

impl std::fmt::Debug for FjallWorkingCheckpointStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FjallWorkingCheckpointStore")
            .field("partition", &PARTITION)
            .finish_non_exhaustive()
    }
}

impl FjallWorkingCheckpointStore {
    /// Open a persistent working checkpoint store.
    ///
    /// # Errors
    ///
    /// Returns `WorkingCheckpointStore` if the database cannot be opened.
    pub fn open(path: &Path) -> error::Result<Self> {
        let fdb = koina::fjall::FjallDb::open(path, &[PARTITION]).map_err(|e| {
            error::WorkingCheckpointStoreSnafu {
                message: format!("failed to open working checkpoint database: {e}"),
            }
            .build()
        })?;
        Ok(Self::from_fjall_db(fdb))
    }

    /// Open an in-memory working checkpoint store (for testing).
    ///
    /// The directory and all data are deleted when the returned store is dropped.
    ///
    /// # Errors
    ///
    /// Returns `WorkingCheckpointStore` if the schema cannot be created.
    pub fn open_in_memory() -> error::Result<Self> {
        let fdb = koina::fjall::FjallDb::open_temp(&[PARTITION]).map_err(|e| {
            error::WorkingCheckpointStoreSnafu {
                message: format!("failed to open in-memory working checkpoint database: {e}"),
            }
            .build()
        })?;
        Ok(Self::from_fjall_db(fdb))
    }

    fn from_fjall_db(fdb: koina::fjall::FjallDb) -> Self {
        let store = Self {
            db: Arc::new(fdb.db),
            write_lock: fdb.write_lock,
            _temp_dir: fdb._temp_dir,
        };
        // WHY(#4853): run once per open, before any caller can observe a
        // mixed-format prefix scan. Cheap no-op when there are no legacy
        // keys (every fresh/already-migrated store).
        if let Err(e) = store.migrate_legacy_ordinal_keys() {
            tracing::warn!(
                error = %e,
                "working checkpoint legacy-key migration failed; old and new \
                 format keys may coexist until the next successful open"
            );
        }
        store
    }

    fn partition(&self) -> error::Result<fjall::SingleWriterTxKeyspace> {
        self.db
            .keyspace(PARTITION, KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::WorkingCheckpointStoreSnafu {
                    message: format!("fjall partition {PARTITION}: {e}"),
                }
                .build()
            })
    }

    fn checkpoint_key(session_id: &str, turn_id: Ulid) -> String {
        format!("nous:working_checkpoint:{session_id}:{turn_id}")
    }

    fn prefix_key(session_id: &str) -> String {
        format!("nous:working_checkpoint:{session_id}:")
    }

    /// One-time migration off the pre-#4853 key shape.
    ///
    /// The legacy key suffix was `{turn_number:020}` -- exactly 20 ASCII
    /// digits. The current suffix is a 26-character Crockford-base32 ULID
    /// string. A legacy key and a ULID key do not interleave correctly
    /// under one lexicographic prefix scan: ASCII digits (`0x30`-`0x39`)
    /// sort before the uppercase letters a ULID can contain, so a mix of
    /// the two shapes under one session prefix would make `prune_old`'s
    /// "keep the newest N by key order" logic evict the wrong entries.
    ///
    /// Working checkpoints are regenerable agent-curated scratch state --
    /// reinjected context, not an audit record (`CHECKPOINT_KEEP_N` already
    /// prunes down to the last 20 per session) -- so rather than fabricate
    /// a synthetic `turn_id` for old rows that never recorded one, the
    /// migration drops the legacy-format rows outright. That is correct by
    /// construction: after this runs, every key remaining under any session
    /// prefix is ULID-shaped, so ordering is consistent again.
    ///
    /// Idempotent: matches only the exact legacy shape (20-byte, all-ASCII-
    /// digit suffix), so a store with no legacy keys -- including a second
    /// call on an already-migrated store -- finds nothing to remove.
    fn migrate_legacy_ordinal_keys(&self) -> error::Result<()> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let partition = self.partition()?;

        let scan_prefix = "nous:working_checkpoint:";
        let legacy = {
            let snap = self.db.read_tx();
            let mut legacy = Vec::new();
            for guard in snap.prefix(&partition, scan_prefix.as_bytes()) {
                let (key, _) = guard.into_inner().map_err(|e| {
                    error::WorkingCheckpointStoreSnafu {
                        message: format!("working checkpoint migration scan failed: {e}"),
                    }
                    .build()
                })?;
                if is_legacy_ordinal_key(&key) {
                    legacy.push(key);
                }
            }
            legacy
        };

        if legacy.is_empty() {
            return Ok(());
        }

        let removed = legacy.len();
        let mut tx = self.db.write_tx();
        for key in legacy {
            tx.remove(&partition, &*key);
        }
        tx.commit().map_err(|e| {
            error::WorkingCheckpointStoreSnafu {
                message: format!("working checkpoint migration commit failed: {e}"),
            }
            .build()
        })?;
        tracing::info!(
            removed,
            "migrated working checkpoint store off legacy zero-padded-ordinal keys (#4853)"
        );

        Ok(())
    }

    /// Delete all but the `keep_n` most recent checkpoints for a session.
    fn prune_old(
        &self,
        session_id: &str,
        keep_n: usize,
    ) -> std::result::Result<(), organon::error::StoreError> {
        let partition = self
            .partition()
            .map_err(|e| organon::error::StoreError::StoreIo {
                context: format!("working checkpoint prune failed: {e}"),
                source: std::io::Error::other("fjall error"),
            })?;

        let prefix = Self::prefix_key(session_id);
        let snap = self.db.read_tx();

        let mut to_remove = Vec::new();
        for guard in snap.prefix(&partition, prefix.as_bytes()) {
            let (key, _) = guard
                .into_inner()
                .map_err(|e| organon::error::StoreError::StoreIo {
                    context: format!("working checkpoint prune iter failed: {e}"),
                    source: std::io::Error::other("fjall iter error"),
                })?;
            to_remove.push(key);
        }

        if to_remove.len() > keep_n {
            let take_n = to_remove.len() - keep_n;
            let mut tx = self.db.write_tx();
            for key in to_remove.into_iter().take(take_n) {
                tx.remove(&partition, &*key);
            }
            tx.commit()
                .map_err(|e| organon::error::StoreError::StoreIo {
                    context: format!("working checkpoint prune commit failed: {e}"),
                    source: std::io::Error::other("fjall commit error"),
                })?;
        }

        Ok(())
    }
}

impl organon::types::WorkingCheckpointStore for FjallWorkingCheckpointStore {
    fn write_checkpoint(
        &self,
        session_id: &str,
        turn_id: Ulid,
        turn_number: u64,
        content: &str,
    ) -> std::result::Result<(), organon::error::StoreError> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let partition = self
            .partition()
            .map_err(|e| organon::error::StoreError::StoreIo {
                context: format!("working checkpoint write failed: {e}"),
                source: std::io::Error::other("fjall error"),
            })?;

        let record = WorkingCheckpointRecord {
            session_id: session_id.to_owned(),
            turn_number,
            content: content.to_owned(),
            created_at: Timestamp::now().to_string(),
        };

        let value = serde_json::to_vec(&record)
            .map_err(|e| organon::error::StoreError::StoreSerialization { source: e })?;

        let mut tx = self.db.write_tx();
        tx.insert(
            &partition,
            Self::checkpoint_key(session_id, turn_id).as_str(),
            value.as_slice(),
        );
        tx.commit()
            .map_err(|e| organon::error::StoreError::StoreIo {
                context: format!("working checkpoint commit failed: {e}"),
                source: std::io::Error::other("fjall commit error"),
            })?;

        self.prune_old(session_id, CHECKPOINT_KEEP_N)?;

        Ok(())
    }

    fn read_latest(
        &self,
        session_id: &str,
    ) -> std::result::Result<Option<organon::types::WorkingCheckpoint>, organon::error::StoreError>
    {
        let partition = self
            .partition()
            .map_err(|e| organon::error::StoreError::StoreIo {
                context: format!("working checkpoint read failed: {e}"),
                source: std::io::Error::other("fjall error"),
            })?;

        let prefix = Self::prefix_key(session_id);
        let snap = self.db.read_tx();

        // WHY: per feedback_fjall_iter_truncate_pitfall.md, never collect
        // prefix().iter() before truncating. We use rev().take(1) to get the
        // most recent entry without holding a full collection.
        let mut iter = snap.prefix(&partition, prefix.as_bytes()).rev();
        let Some(guard) = iter.next() else {
            return Ok(None);
        };

        let (key, value) = guard
            .into_inner()
            .map_err(|e| organon::error::StoreError::StoreIo {
                context: format!("working checkpoint iter failed: {e}"),
                source: std::io::Error::other("fjall iter error"),
            })?;

        let record: WorkingCheckpointRecord = serde_json::from_slice(&value).map_err(|e| {
            organon::error::StoreError::StoreSerialization {
                source: serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "decode failed for key {}: {e}",
                        String::from_utf8_lossy(&key)
                    ),
                )),
            }
        })?;

        Ok(Some(organon::types::WorkingCheckpoint {
            session_id: record.session_id,
            turn_number: record.turn_number,
            content: record.content,
            created_at: record.created_at,
        }))
    }

    fn read_recent(
        &self,
        session_id: &str,
        limit: usize,
    ) -> std::result::Result<Vec<organon::types::WorkingCheckpoint>, organon::error::StoreError>
    {
        let partition = self
            .partition()
            .map_err(|e| organon::error::StoreError::StoreIo {
                context: format!("working checkpoint read failed: {e}"),
                source: std::io::Error::other("fjall error"),
            })?;

        let prefix = Self::prefix_key(session_id);
        let snap = self.db.read_tx();

        // WHY: per feedback_fjall_iter_truncate_pitfall.md, use rev().take(N)
        // rather than collecting the full prefix iterator.
        let mut results = Vec::with_capacity(limit);
        for guard in snap.prefix(&partition, prefix.as_bytes()).rev().take(limit) {
            let (key, value) =
                guard
                    .into_inner()
                    .map_err(|e| organon::error::StoreError::StoreIo {
                        context: format!("working checkpoint iter failed: {e}"),
                        source: std::io::Error::other("fjall iter error"),
                    })?;

            let record: WorkingCheckpointRecord = serde_json::from_slice(&value).map_err(|e| {
                organon::error::StoreError::StoreSerialization {
                    source: serde_json::Error::io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "decode failed for key {}: {e}",
                            String::from_utf8_lossy(&key)
                        ),
                    )),
                }
            })?;

            results.push(organon::types::WorkingCheckpoint {
                session_id: record.session_id,
                turn_number: record.turn_number,
                content: record.content,
                created_at: record.created_at,
            });
        }

        Ok(results)
    }

    fn delete_session(
        &self,
        session_id: &str,
    ) -> std::result::Result<usize, organon::error::StoreError> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let partition = self
            .partition()
            .map_err(|e| organon::error::StoreError::StoreIo {
                context: format!("working checkpoint session delete failed: {e}"),
                source: std::io::Error::other("fjall error"),
            })?;

        let prefix = Self::prefix_key(session_id);

        // WHY: per feedback_fjall_iter_truncate_pitfall.md, collecting the
        // full prefix before removing is required here (unlike `read_latest`/
        // `read_recent`, this deletes every matching row, not just the newest
        // N) -- the same collect-then-remove shape `prune_old` already uses.
        let to_remove: Vec<_> = {
            let snap = self.db.read_tx();
            let mut keys = Vec::new();
            for guard in snap.prefix(&partition, prefix.as_bytes()) {
                let (key, _) =
                    guard
                        .into_inner()
                        .map_err(|e| organon::error::StoreError::StoreIo {
                            context: format!("working checkpoint session delete iter failed: {e}"),
                            source: std::io::Error::other("fjall iter error"),
                        })?;
                keys.push(key);
            }
            keys
        };

        let removed = to_remove.len();
        if removed > 0 {
            let mut tx = self.db.write_tx();
            for key in to_remove {
                tx.remove(&partition, &*key);
            }
            tx.commit()
                .map_err(|e| organon::error::StoreError::StoreIo {
                    context: format!("working checkpoint session delete commit failed: {e}"),
                    source: std::io::Error::other("fjall commit error"),
                })?;
        }

        Ok(removed)
    }
}

/// True when `key` ends in the pre-#4853 legacy suffix: a `:` followed by
/// exactly 20 ASCII-digit bytes (the zero-padded `turn_number` ordinal).
/// The current suffix is a 26-character Crockford-base32 ULID, so length
/// alone already distinguishes the two shapes; the digit check is defense
/// in depth against a future key format that happens to also be 20 bytes.
fn is_legacy_ordinal_key(key: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(key) else {
        return false;
    };
    match s.rsplit_once(':') {
        Some((_, suffix)) => suffix.len() == 20 && suffix.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
