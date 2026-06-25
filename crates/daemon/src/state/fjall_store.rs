//! Fjall-backed task-state store.
//!
//! Pure-Rust LSM-tree storage via `fjall`. Zero C dependencies.
//!
//! # Key schema
//!
//! All keys are UTF-8 strings. Values are JSON-encoded [`TaskState`] records.
//!
//! | Partition    | Key pattern  | Value            |
//! |--------------|--------------|------------------|
//! | `ops:tasks`  | `{task_id}`  | JSON `TaskState` |

use std::path::Path;

use fjall::{KeyspaceCreateOptions, PersistMode, Readable as _, SingleWriterTxDatabase};
use snafu::ResultExt as _;

use crate::error::{self, Result};
use crate::state::TaskState;

/// Partition name for task state records.
const PARTITION: &str = "ops:tasks";

/// Fjall-backed store for task execution state.
///
/// One keyspace directory holds state for all tasks in a runner.
/// Uses `SingleWriterTxDatabase` for durability without WAL complexity.
pub struct TaskStateStore {
    db: SingleWriterTxDatabase,
}

impl TaskStateStore {
    /// Open (or create) the task state store at `path`.
    ///
    /// `path` is a directory; fjall manages its own internal files within it.
    ///
    /// # Errors
    ///
    /// Returns `Storage` if the fjall keyspace cannot be opened or the
    /// `ops:tasks` partition cannot be initialised.
    pub fn open(path: &Path) -> Result<Self> {
        let fdb = koina::fjall::FjallDb::open(path, &[PARTITION]).map_err(|e| {
            error::StorageSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        Ok(Self { db: fdb.db })
    }

    /// Flush the fjall journal to stable storage so committed writes survive
    /// power loss or an unclean shutdown.
    ///
    /// Call this after every `tx.commit()` on the operational write path.
    /// Task-state writes are low frequency (once per task completion), so the
    /// synchronous fsync cost is acceptable.
    fn ensure_durable(&self) -> Result<()> {
        self.db.persist(PersistMode::SyncAll).map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall persist task-state: {e}"),
            }
            .build()
        })
    }

    /// Load all persisted task states.
    ///
    /// # Errors
    ///
    /// Returns `Storage` on fjall I/O failure or `StoredJson` on JSON decode
    /// failure.
    pub fn load_all(&self) -> Result<Vec<TaskState>> {
        let partition = self
            .db
            .keyspace(PARTITION, KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall partition {PARTITION}: {e}"),
                }
                .build()
            })?;

        let snap = self.db.read_tx();
        let mut states = Vec::new();

        // WHY: fjall range returns Guard items; `.into_inner()` unwraps the
        // guarded key-value pair, returning an error if the guard is poisoned.
        for guard in snap.range::<&[u8], _>(&partition, ..) {
            let (_, value) = guard.into_inner().map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall iter task-state: {e}"),
                }
                .build()
            })?;

            let state: TaskState =
                serde_json::from_slice(&value).context(error::StoredJsonSnafu)?;
            states.push(state);
        }

        Ok(states)
    }

    /// Persist (upsert) the state for a task.
    ///
    /// # Errors
    ///
    /// Returns `StoredJson` on serialization failure or `Storage` on write
    /// failure.
    pub fn save(&self, state: &TaskState) -> Result<()> {
        let partition = self
            .db
            .keyspace(PARTITION, KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall partition {PARTITION}: {e}"),
                }
                .build()
            })?;

        let data = serde_json::to_vec(state).context(error::StoredJsonSnafu)?;

        let mut tx = self.db.write_tx();
        tx.insert(&partition, state.task_id.as_str(), data.as_slice());
        tx.commit().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall commit task-state for {}: {e}", state.task_id),
            }
            .build()
        })?;

        // WHY(#5752): without an explicit fsync, tx.commit() leaves the
        // task-state record in the OS page cache. A crash before the next
        // fjall flush makes `restore_state()` read stale state and
        // `check_missed_cron_catchup()` can re-fire a task that already ran.
        self.ensure_durable()?;

        Ok(())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStateStore::open(tmp.path()).unwrap();

        let state = TaskState {
            task_id: "task::cleanup".to_owned(),
            last_run_ts: Some("2026-06-22T12:00:00Z".to_owned()),
            run_count: 7,
            consecutive_failures: 1,
            ..TaskState::default()
        };

        store.save(&state).unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        let got = loaded.first().unwrap();
        assert_eq!(got.task_id, state.task_id);
        assert_eq!(got.last_run_ts, state.last_run_ts);
        assert_eq!(got.run_count, state.run_count);
        assert_eq!(got.consecutive_failures, state.consecutive_failures);
    }

    #[test]
    fn save_survives_store_reopen() {
        // WHY(#5752): this test simulates a daemon restart. The store must
        // fsync on save so the task-state record is still present after the
        // old handle is dropped and a new one opens the same directory.
        let tmp = tempfile::tempdir().unwrap();

        {
            let store = TaskStateStore::open(tmp.path()).unwrap();
            let state = TaskState {
                task_id: "task::knowledge-maintenance".to_owned(),
                last_run_ts: Some("2026-06-22T15:30:00Z".to_owned()),
                run_count: 3,
                consecutive_failures: 0,
                ..TaskState::default()
            };
            store.save(&state).unwrap();
        }

        let reopened = TaskStateStore::open(tmp.path()).unwrap();
        let loaded = reopened.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        let got = loaded.first().unwrap();
        assert_eq!(got.task_id, "task::knowledge-maintenance");
        assert_eq!(got.run_count, 3);
    }

    #[test]
    fn save_updates_existing_task_state() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStateStore::open(tmp.path()).unwrap();

        store
            .save(&TaskState {
                task_id: "task::drift".to_owned(),
                run_count: 1,
                ..TaskState::default()
            })
            .unwrap();
        store
            .save(&TaskState {
                task_id: "task::drift".to_owned(),
                run_count: 2,
                last_run_ts: Some("2026-06-22T16:00:00Z".to_owned()),
                ..TaskState::default()
            })
            .unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        let got = loaded.first().unwrap();
        assert_eq!(got.run_count, 2);
        assert_eq!(got.last_run_ts, Some("2026-06-22T16:00:00Z".to_owned()));
    }
}
