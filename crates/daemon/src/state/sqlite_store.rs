//! SQLite-backed task-state store (preserved for migration parity).
//!
//! Single-writer SQLite via `rusqlite`. Preserved behind the `sqlite` feature
//! flag for backward compatibility while the fjall backend is adopted.
//!
//! # Schema
//!
//! ```sql
//! CREATE TABLE task_state (
//!     task_id              TEXT PRIMARY KEY NOT NULL,
//!     last_run_ts          TEXT,
//!     run_count            INTEGER NOT NULL DEFAULT 0,
//!     consecutive_failures INTEGER NOT NULL DEFAULT 0
//! );
//! ```

use std::path::Path;

use crate::error::Result;
use crate::state::TaskState;

/// SQLite-backed store for task execution state.
///
/// One `task_state.db` file holds state for all tasks in a runner.
/// Single-writer: no WAL needed.
pub(crate) struct TaskStateStore {
    conn: rusqlite::Connection,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "open + create_schema are called from #[cfg(test)] state::tests and runner_tests; production wiring lives in the binary crate"
    )
)]
impl TaskStateStore {
    /// Open (or create) the task state database at `path`.
    ///
    /// For the SQLite backend, `path` is used as a directory base; appends
    /// `.db` extension when missing so callers can use the same bare path as
    /// the fjall backend.
    ///
    /// # Errors
    ///
    /// Returns `TaskFailed` if the database cannot be opened or the schema
    /// cannot be created.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        // WHY: the sqlite backend uses a `.db` file; append extension when the
        // caller passes a bare path (matching the fjall backend convention).
        let db_path = if path.extension().is_some() {
            path.to_path_buf()
        } else {
            path.with_extension("db")
        };

        let conn = rusqlite::Connection::open(&db_path).map_err(|e: rusqlite::Error| {
            crate::error::TaskFailedSnafu {
                task_id: "state-db-open".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e: rusqlite::Error| {
                crate::error::TaskFailedSnafu {
                    task_id: "state-db-open".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;
        Self::create_schema(&conn)?;
        Ok(Self { conn })
    }

    fn create_schema(conn: &rusqlite::Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS task_state (
                task_id              TEXT PRIMARY KEY NOT NULL,
                last_run_ts          TEXT,
                run_count            INTEGER NOT NULL DEFAULT 0,
                consecutive_failures INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(|e: rusqlite::Error| {
            crate::error::TaskFailedSnafu {
                task_id: "state-db-schema".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;
        Ok(())
    }

    /// Load all persisted task states.
    ///
    /// # Errors
    ///
    /// Returns `TaskFailed` on SQLite query failure.
    pub(crate) fn load_all(&self) -> Result<Vec<TaskState>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT task_id, last_run_ts, run_count, consecutive_failures
                 FROM task_state",
            )
            .map_err(|e: rusqlite::Error| {
                crate::error::TaskFailedSnafu {
                    task_id: "state-db-prepare".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;

        let rows = stmt
            .query_map([], |row: &rusqlite::Row| {
                Ok(TaskState {
                    task_id: row.get(0)?,
                    last_run_ts: row.get(1)?,
                    run_count: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                    consecutive_failures: u32::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                })
            })
            .map_err(|e: rusqlite::Error| {
                crate::error::TaskFailedSnafu {
                    task_id: "state-db-query".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;

        let mut states = Vec::new();
        for row in rows {
            states.push(row.map_err(|e: rusqlite::Error| {
                crate::error::TaskFailedSnafu {
                    task_id: "state-db-row".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?);
        }
        Ok(states)
    }

    /// Persist (upsert) the state for a task.
    ///
    /// # Errors
    ///
    /// Returns `TaskFailed` on SQLite write failure.
    pub(crate) fn save(&self, state: &TaskState) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO task_state
                 (task_id, last_run_ts, run_count, consecutive_failures)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    state.task_id,
                    state.last_run_ts,
                    i64::try_from(state.run_count).unwrap_or(i64::MAX),
                    i64::from(state.consecutive_failures),
                ],
            )
            .map_err(|e: rusqlite::Error| {
                crate::error::TaskFailedSnafu {
                    task_id: state.task_id.clone(),
                    reason: format!("save task state: {e}"),
                }
                .build()
            })?;
        Ok(())
    }
}
