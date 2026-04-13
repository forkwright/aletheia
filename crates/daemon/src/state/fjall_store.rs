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

use fjall::{KeyspaceCreateOptions, Readable as _, SingleWriterTxDatabase};
use snafu::{IntoError as _, ResultExt as _};

use crate::error::{self, Result};
use crate::state::TaskState;

/// Fjall-backed store for task execution state.
///
/// One keyspace directory holds state for all tasks in a runner.
/// Uses `SingleWriterTxDatabase` for durability without WAL complexity.
pub(crate) struct TaskStateStore {
    db: SingleWriterTxDatabase,
}

/// Partition name for task state records.
const PARTITION: &str = "ops:tasks";

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "open + methods are called from #[cfg(test)] state::tests and runner_tests; production wiring lives in the binary crate"
    )
)]
impl TaskStateStore {
    /// Open (or create) the task state store at `path`.
    ///
    /// `path` is a directory; fjall manages its own internal files within it.
    ///
    /// # Errors
    ///
    /// Returns `Storage` if the fjall keyspace cannot be opened or the
    /// `ops:tasks` partition cannot be initialised.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path).map_err(|e| {
            crate::error::MaintenanceIoSnafu {
                context: format!("creating task-state directory: {}", path.display()),
            }
            .into_error(e)
        })?;

        let db = SingleWriterTxDatabase::builder(path).open().map_err(|e| {
            error::StorageSnafu {
                message: format!("fjall open task-state store: {e}"),
            }
            .build()
        })?;

        // Eagerly open the partition so it exists before any read/write.
        db.keyspace(PARTITION, KeyspaceCreateOptions::default)
            .map_err(|e| {
                error::StorageSnafu {
                    message: format!("fjall open partition {PARTITION}: {e}"),
                }
                .build()
            })?;

        Ok(Self { db })
    }

    /// Load all persisted task states.
    ///
    /// # Errors
    ///
    /// Returns `Storage` on fjall I/O failure or `StoredJson` on JSON decode
    /// failure.
    pub(crate) fn load_all(&self) -> Result<Vec<TaskState>> {
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
    pub(crate) fn save(&self, state: &TaskState) -> Result<()> {
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

        Ok(())
    }
}
