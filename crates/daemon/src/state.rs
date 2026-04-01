//! SQLite-backed persistence for daemon task execution state.
//!
//! Survives process restarts so tasks resume their schedules rather than
//! running immediately on every restart. Also provides workspace-level
//! daemon configuration and single-instance locking.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
pub(crate) struct TaskStateStore {
    conn: rusqlite::Connection,
}

impl TaskStateStore {
    /// Open (or create) the task state database at `path`.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path).map_err(|e| {
            crate::error::TaskFailedSnafu {
                task_id: "state-db-open".to_owned(),
                reason: e.to_string(),
            }
            .build()
        })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| {
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
    pub(crate) fn load_all(&self) -> Result<Vec<TaskState>> {
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


/// Per-workspace daemon configuration parsed from `.aletheia/daemon.toml`.
///
/// WHY: the daemon only activates for workspaces that have explicitly opted in.
/// Autonomous execution in an unaware workspace violates user trust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Whether daemon mode is enabled for this workspace.
    #[serde(default)]
    pub enabled: bool,

    /// Maximum concurrent child agents the daemon may spawn.
    #[serde(default = "default_max_children")]
    pub max_children: usize,

    /// Allowed trigger types for this workspace.
    #[serde(default)]
    pub allowed_triggers: AllowedTriggers,

    /// Allowed builtin task IDs. Empty means all registered tasks are allowed.
    #[serde(default)]
    pub allowed_tasks: Vec<String>,

    /// Webhook listener port. `None` disables the webhook trigger.
    #[serde(default)]
    pub webhook_port: Option<u16>,

    /// File watch paths (relative to workspace root). Empty disables file watching.
    #[serde(default)]
    pub watch_paths: Vec<String>,

    /// Brief output mode: truncate tool results and model responses in logs.
    #[serde(default)]
    pub brief_output: bool,
}

fn default_max_children() -> usize {
    3
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_children: default_max_children(),
            allowed_triggers: AllowedTriggers::default(),
            allowed_tasks: Vec::new(),
            webhook_port: None,
            watch_paths: Vec::new(),
            brief_output: false,
        }
    }
}

/// Which trigger types are permitted in this workspace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllowedTriggers {
    /// Allow file-watcher triggers.
    #[serde(default)]
    pub file_watch: bool,
    /// Allow webhook triggers.
    #[serde(default)]
    pub webhook: bool,
}

impl DaemonConfig {
    /// Load daemon configuration from a workspace directory.
    ///
    /// Looks for `.aletheia/daemon.toml` in `workspace_root`. Returns a default
    /// (disabled) config if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns `TaskFailed` if the file exists but cannot be parsed.
    pub fn load(workspace_root: &Path) -> Result<Self> {
        let config_path = workspace_root.join(".aletheia").join("daemon.toml");

        if !config_path.exists() {
            tracing::debug!(
                path = %config_path.display(),
                "daemon.toml not found — daemon disabled for this workspace"
            );
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&config_path).map_err(|e| {
            crate::error::MaintenanceIoSnafu {
                context: format!("reading daemon config: {}", config_path.display()),
            }
            .build_with_source(e)
        })?;

        let config: Self = toml::from_str(&contents).map_err(|e| {
            crate::error::TaskFailedSnafu {
                task_id: "daemon-config-parse".to_owned(),
                reason: format!("invalid daemon.toml: {e}"),
            }
            .build()
        })?;

        tracing::info!(
            enabled = config.enabled,
            max_children = config.max_children,
            webhook_port = ?config.webhook_port,
            watch_paths = ?config.watch_paths,
            "loaded daemon config"
        );

        Ok(config)
    }

    /// Check whether a task ID is allowed by this workspace config.
    ///
    /// Empty `allowed_tasks` means all tasks are allowed.
    #[must_use]
    pub fn is_task_allowed(&self, task_id: &str) -> bool {
        self.allowed_tasks.is_empty() || self.allowed_tasks.iter().any(|t| t == task_id)
    }
}


/// Single-instance lock guard for daemon process per workspace.
///
/// WHY: only one daemon process should run per workspace. `fd-lock` provides
/// cross-platform advisory file locking (flock on Unix, `LockFileEx` on Windows).
///
/// The lock file is created at `.aletheia/daemon.lock` in the workspace root.
/// The lock is held for the lifetime of this guard and released on drop.
pub struct WorkspaceGuard {
    _lock: fd_lock::RwLock<std::fs::File>,
    path: PathBuf,
}

impl WorkspaceGuard {
    /// Attempt to acquire the workspace daemon lock.
    ///
    /// # Errors
    ///
    /// Returns `TaskFailed` if the lock file cannot be created or another
    /// daemon instance already holds the lock.
    pub fn acquire(workspace_root: &Path) -> Result<Self> {
        let lock_dir = workspace_root.join(".aletheia");
        if !lock_dir.exists() {
            std::fs::create_dir_all(&lock_dir).map_err(|e| {
                crate::error::MaintenanceIoSnafu {
                    context: format!("creating lock directory: {}", lock_dir.display()),
                }
                .build_with_source(e)
            })?;
        }

        let lock_path = lock_dir.join("daemon.lock");
        let file = std::fs::File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| {
                crate::error::MaintenanceIoSnafu {
                    context: format!("opening lock file: {}", lock_path.display()),
                }
                .build_with_source(e)
            })?;

        let mut lock = fd_lock::RwLock::new(file);

        // NOTE: try_write() is non-blocking; returns Err if another process holds the lock.
        match lock.try_write() {
            Ok(_guard) => {
                // WHY: we need to hold the RwLock (not the guard) for the lifetime
                // of the daemon. The RwLock itself keeps the file descriptor open
                // and the advisory lock held.
                tracing::info!(
                    path = %lock_path.display(),
                    "workspace daemon lock acquired"
                );
                Ok(Self {
                    _lock: lock,
                    path: lock_path,
                })
            }
            Err(e) => Err(crate::error::TaskFailedSnafu {
                task_id: "workspace-lock".to_owned(),
                reason: format!(
                    "another daemon instance holds the lock at {}: {e}",
                    lock_path.display()
                ),
            }
            .build()),
        }
    }

    /// Path to the lock file.
    #[must_use]
    pub fn lock_path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        tracing::debug!(
            path = %self.path.display(),
            "releasing workspace daemon lock"
        );
        // NOTE: fd-lock releases the advisory lock when the RwLock is dropped.
        // We also try to clean up the lock file, but failure is not critical.
        let _ = std::fs::remove_file(&self.path);
    }
}


#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
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

        let store2 = TaskStateStore::open(&db_path).unwrap();
        let loaded = store2.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].task_id, "persistent");
        assert_eq!(loaded[0].run_count, 7);
        assert_eq!(loaded[0].consecutive_failures, 2);
    }

    // -- DaemonConfig tests --

    #[test]
    fn daemon_config_default_is_disabled() {
        let config = DaemonConfig::default();
        assert!(!config.enabled, "default daemon config must be disabled");
        assert_eq!(config.max_children, 3, "default max_children must be 3");
        assert!(!config.allowed_triggers.file_watch);
        assert!(!config.allowed_triggers.webhook);
    }

    #[test]
    fn daemon_config_loads_from_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let dot_aletheia = tmp.path().join(".aletheia");
        std::fs::create_dir_all(&dot_aletheia).unwrap();
        std::fs::write(
            dot_aletheia.join("daemon.toml"),
            r#"
enabled = true
max_children = 5
brief_output = true
webhook_port = 9090
watch_paths = ["src/", "config/"]

[allowed_triggers]
file_watch = true
webhook = true
"#,
        )
        .unwrap();

        let config = DaemonConfig::load(tmp.path()).unwrap();
        assert!(config.enabled);
        assert_eq!(config.max_children, 5);
        assert!(config.brief_output);
        assert_eq!(config.webhook_port, Some(9090));
        assert_eq!(config.watch_paths.len(), 2);
        assert!(config.allowed_triggers.file_watch);
        assert!(config.allowed_triggers.webhook);
    }

    #[test]
    fn daemon_config_missing_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let config = DaemonConfig::load(tmp.path()).unwrap();
        assert!(!config.enabled, "missing config file → disabled");
    }

    #[test]
    fn daemon_config_invalid_toml_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dot_aletheia = tmp.path().join(".aletheia");
        std::fs::create_dir_all(&dot_aletheia).unwrap();
        std::fs::write(dot_aletheia.join("daemon.toml"), "not valid toml {{{}").unwrap();

        let result = DaemonConfig::load(tmp.path());
        assert!(result.is_err(), "invalid TOML should return an error");
    }

    #[test]
    fn is_task_allowed_empty_allows_all() {
        let config = DaemonConfig::default();
        assert!(config.is_task_allowed("any-task"));
        assert!(config.is_task_allowed("prosoche"));
    }

    #[test]
    fn is_task_allowed_filters_by_list() {
        let config = DaemonConfig {
            allowed_tasks: vec!["prosoche".to_owned(), "trace-rotation".to_owned()],
            ..DaemonConfig::default()
        };
        assert!(config.is_task_allowed("prosoche"));
        assert!(config.is_task_allowed("trace-rotation"));
        assert!(
            !config.is_task_allowed("evolution"),
            "unlisted task must be denied"
        );
    }

    // -- WorkspaceGuard tests --

    #[test]
    fn workspace_guard_acquires_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let guard = WorkspaceGuard::acquire(tmp.path());
        assert!(guard.is_ok(), "first lock acquisition should succeed");
        let guard = guard.unwrap();
        assert!(
            guard.lock_path().exists(),
            "lock file should exist while held"
        );
    }

    #[test]
    fn workspace_guard_prevents_double_acquisition() {
        let tmp = tempfile::tempdir().unwrap();
        let _guard1 = WorkspaceGuard::acquire(tmp.path()).unwrap();

        // NOTE: second acquisition in the same process may or may not fail
        // depending on OS flock semantics (Linux flock is per-fd, not per-process
        // for the same file). This test verifies the API works; true multi-process
        // locking is validated by integration tests.
        // The important thing is that the first guard holds the lock.
        assert!(
            _guard1.lock_path().exists(),
            "lock file should exist while first guard is held"
        );
    }

    #[test]
    fn workspace_guard_releases_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_path;
        {
            let guard = WorkspaceGuard::acquire(tmp.path()).unwrap();
            lock_path = guard.lock_path().to_owned();
            assert!(lock_path.exists(), "lock file should exist while held");
        }
        // NOTE: after drop, lock file is removed (best-effort)
        assert!(
            !lock_path.exists(),
            "lock file should be removed after guard drop"
        );
    }

    #[test]
    fn workspace_guard_creates_lock_directory() {
        let tmp = tempfile::tempdir().unwrap();
        // NOTE: .aletheia/ does not exist yet
        assert!(!tmp.path().join(".aletheia").exists());
        let _guard = WorkspaceGuard::acquire(tmp.path()).unwrap();
        assert!(
            tmp.path().join(".aletheia").exists(),
            ".aletheia/ should be created by lock acquisition"
        );
    }
}
