//! Task-state persistence for daemon: scheduled task execution state across restarts.
//!
//! Selects a backend at compile time via feature flags:
//!
//! - `fjall` (default): pure-Rust LSM-tree store via `fjall`. Zero C deps.
//! - `sqlite`: single-writer SQLite via `rusqlite`.
//!
//! Both backends expose the same [`TaskStateStore`] type with identical methods.
//! Also provides workspace-level daemon configuration and single-instance locking.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use snafu::ResultExt as _;

use crate::error::Result;

/// Persisted execution state for a single registered task.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// -- Fjall backend -----------------------------------------------------------
// WHY: when both fjall and sqlite are active (workspace feature unification),
// prefer sqlite so TaskStateStore is unambiguous and fjall helpers are not dead
// code. Fjall module only compiles when sqlite is absent.

#[cfg(all(feature = "fjall", not(feature = "sqlite")))]
mod fjall_store;
#[cfg(all(feature = "fjall", not(feature = "sqlite")))]
pub(crate) use fjall_store::TaskStateStore;

// -- SQLite backend ----------------------------------------------------------
#[cfg(feature = "sqlite")]
mod sqlite_store;
#[cfg(feature = "sqlite")]
pub(crate) use sqlite_store::TaskStateStore;

// -- Workspace config and locking (shared across backends) -------------------

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

    /// Self-prompting configuration: daemon-initiated follow-up actions.
    ///
    /// WHY: self-prompting must be opt-in. Without explicit enablement, the
    /// daemon never sends itself follow-up prompts. Rate limiting prevents
    /// runaway loops from misconfigured attention checks.
    #[serde(default)]
    pub self_prompt: crate::self_prompt::SelfPromptConfig,
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
            self_prompt: crate::self_prompt::SelfPromptConfig::default(),
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
                "daemon.toml not found -- daemon disabled for this workspace"
            );
            return Ok(Self::default());
        }

        let contents =
            std::fs::read_to_string(&config_path).context(crate::error::MaintenanceIoSnafu {
                context: format!("reading daemon config: {}", config_path.display()),
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
/// WHY: only one daemon process should run per workspace. We use
/// `rustix::fs::flock` directly on the lock file's file descriptor.
///
/// The lock file is created at `.aletheia/daemon.lock` in the workspace root.
/// The advisory lock is bound to the file descriptor and held for as long as
/// `WorkspaceGuard` (and therefore the inner `File`) lives. Drop closes the
/// file descriptor, which releases the flock automatically.
///
/// # Bug history
///
/// Previously this used `fd_lock::RwLock<File>` and probed with `try_write()`,
/// then immediately dropped the resulting `RwLockWriteGuard`. This was a bug:
/// `RwLockWriteGuard::drop` calls `flock(fd, LOCK_UN)`, releasing the lock
/// before the `WorkspaceGuard` was even returned to the caller. Two
/// `acquire()` calls in the same process would both succeed. Tracked in #3026.
pub struct WorkspaceGuard {
    /// The lock file. Holding this open keeps the flock alive on the
    /// associated file descriptor; closing it releases the flock.
    _file: std::fs::File,
    path: PathBuf,
}

impl std::fmt::Debug for WorkspaceGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceGuard")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl WorkspaceGuard {
    /// Attempt to acquire the workspace daemon lock.
    ///
    /// # Errors
    ///
    /// Returns `TaskFailed` if the lock file cannot be created or another
    /// daemon instance already holds the lock.
    pub fn acquire(workspace_root: &Path) -> Result<Self> {
        use std::os::fd::AsFd;

        let lock_dir = workspace_root.join(".aletheia");
        if !lock_dir.exists() {
            std::fs::create_dir_all(&lock_dir).context(crate::error::MaintenanceIoSnafu {
                context: format!("creating lock directory: {}", lock_dir.display()),
            })?;
        }

        let lock_path = lock_dir.join("daemon.lock");
        let file = std::fs::File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .context(crate::error::MaintenanceIoSnafu {
                context: format!("opening lock file: {}", lock_path.display()),
            })?;

        // WHY: rustix::fs::flock binds the advisory lock to the file descriptor.
        // The lock lives for as long as `file` is open, and is released
        // automatically when `file` is dropped (which closes the fd).
        // `NonBlockingLockExclusive` returns `EWOULDBLOCK` if another process
        // (or another file descriptor in this process) already holds an
        // exclusive flock on the same inode.
        rustix::fs::flock(file.as_fd(), rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|e| {
                crate::error::TaskFailedSnafu {
                    task_id: "workspace-lock".to_owned(),
                    reason: format!(
                        "another daemon instance holds the lock at {}: {e}",
                        lock_path.display()
                    ),
                }
                .build()
            })?;

        tracing::info!(
            path = %lock_path.display(),
            "workspace daemon lock acquired"
        );
        Ok(Self {
            _file: file,
            path: lock_path,
        })
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
        // NOTE: closing `_file` releases the flock automatically.
        // We also try to clean up the lock file, but failure is not critical.
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests use std::fs to set up tempdir fixtures synchronously; tokio::fs would be unnecessary ceremony"
)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_task_state() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TaskStateStore::open(&tmp.path().join("state")).unwrap();

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
        let store = TaskStateStore::open(&tmp.path().join("state")).unwrap();

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
        let store = TaskStateStore::open(&tmp.path().join("state")).unwrap();
        let loaded = store.load_all().unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state");

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
        assert!(!config.enabled, "missing config file -> disabled");
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
        let guard1 = WorkspaceGuard::acquire(tmp.path()).expect("first acquire");
        assert!(guard1.lock_path().exists(), "lock file exists after first acquire");

        let guard2 = WorkspaceGuard::acquire(tmp.path());
        assert!(
            guard2.is_err(),
            "second acquire() in same process must fail while first guard is held"
        );

        assert!(guard1.lock_path().exists(), "lock file still exists");
    }

    #[test]
    fn workspace_guard_releases_after_first_drop_allows_reacquire() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let _guard1 = WorkspaceGuard::acquire(tmp.path()).expect("first acquire");
        }
        let _guard2 = WorkspaceGuard::acquire(tmp.path()).expect(
            "second acquire after first guard dropped should succeed",
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
        assert!(
            !lock_path.exists(),
            "lock file should be removed after guard drop"
        );
    }

    #[test]
    fn workspace_guard_creates_lock_directory() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!tmp.path().join(".aletheia").exists());
        let _guard = WorkspaceGuard::acquire(tmp.path()).unwrap();
        assert!(
            tmp.path().join(".aletheia").exists(),
            ".aletheia/ should be created by lock acquisition"
        );
    }
}
