//! SQLite-backed persistence for daemon task execution state.
//!
//! Survives process restarts so tasks resume their schedules rather than
//! running immediately on every restart.

use std::path::Path;

use crate::error::Result;

/// Persisted execution state for a single registered task.
#[derive(Debug, Clone)]
pub struct TaskState {
    /// Task ID matching `TaskDef::id`.
    pub task_id: String,
    /// ISO 8601 timestamp of the last execution (success or failure).
    pub last_run_ts: Option<String>,
    /// Total completed executions.
    pub run_count: u64,
    /// Consecutive failures since the last success.
    pub consecutive_failures: u32,
}

/// SQLite-backed store for task execution state.
///
/// One `task_state.db` file holds state for all tasks in a runner.
/// Single-writer: no WAL needed.
pub struct TaskStateStore {
    conn: rusqlite::Connection,
}

impl TaskStateStore {
    /// Open (or create) the task state database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path).map_err(|e| {
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
        .map_err(|e| {
            crate::error::TaskFailedSnafu {
                task_id: "state-db-schema".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;
        Ok(())
    }

    /// Load all persisted task states.
    pub fn load_all(&self) -> Result<Vec<TaskState>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT task_id, last_run_ts, run_count, consecutive_failures
                 FROM task_state",
            )
            .map_err(|e| {
                crate::error::TaskFailedSnafu {
                    task_id: "state-db-prepare".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;

        let rows = stmt
            .query_map([], |row| {
                Ok(TaskState {
                    task_id: row.get(0)?,
                    last_run_ts: row.get(1)?,
                    run_count: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                    consecutive_failures: u32::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                })
            })
            .map_err(|e| {
                crate::error::TaskFailedSnafu {
                    task_id: "state-db-query".to_owned(),
                    reason: e.to_string(),
                }
                .build()
            })?;

        let mut states = Vec::new();
        for row in rows {
            states.push(row.map_err(|e| {
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
    pub fn save(&self, state: &TaskState) -> Result<()> {
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
            .map_err(|e| {
                crate::error::TaskFailedSnafu {
                    task_id: state.task_id.clone(),
                    reason: format!("save task state: {e}"),
                }
                .build()
            })?;
        Ok(())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_task_state() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStateStore::open(&tmp.path().join("state.db")).unwrap();

        let state = TaskState {
            task_id: "test-task".to_owned(),
            last_run_ts: Some("2026-01-01T00:00:00Z".to_owned()),
            run_count: 42,
            consecutive_failures: 1,
        };
        store.save(&state).unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].task_id, "test-task");
        assert_eq!(loaded[0].run_count, 42);
        assert_eq!(loaded[0].consecutive_failures, 1);
        assert_eq!(
            loaded[0].last_run_ts.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
    }

    #[test]
    fn upsert_updates_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStateStore::open(&tmp.path().join("state.db")).unwrap();

        let state = TaskState {
            task_id: "t1".to_owned(),
            last_run_ts: None,
            run_count: 1,
            consecutive_failures: 0,
        };
        store.save(&state).unwrap();

        let updated = TaskState {
            task_id: "t1".to_owned(),
            last_run_ts: Some("2026-03-01T12:00:00Z".to_owned()),
            run_count: 5,
            consecutive_failures: 0,
        };
        store.save(&updated).unwrap();

        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].run_count, 5);
    }

    #[test]
    fn empty_store_returns_empty_vec() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStateStore::open(&tmp.path().join("state.db")).unwrap();
        let loaded = store.load_all().unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state.db");

        {
            let store = TaskStateStore::open(&db_path).unwrap();
            store
                .save(&TaskState {
                    task_id: "persistent".to_owned(),
                    last_run_ts: Some("2026-03-13T10:00:00Z".to_owned()),
                    run_count: 7,
                    consecutive_failures: 2,
                })
                .unwrap();
        }

        // Re-open: state should survive.
        let store2 = TaskStateStore::open(&db_path).unwrap();
        let loaded = store2.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].task_id, "persistent");
        assert_eq!(loaded[0].run_count, 7);
        assert_eq!(loaded[0].consecutive_failures, 2);
    }
}
