//! Window state persistence for the desktop application.
//!
//! Saves and restores window geometry, active view, and sidebar state
//! to `~/.config/aletheia-desktop/window-state.toml`. Writes are debounced
//! to avoid excessive disk I/O during window drag/resize operations.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use snafu::{ResultExt, Snafu};
use tokio::sync::Notify;
use tracing::Instrument;

use crate::state::platform::WindowState;

/// Debounce interval for window state saves.
const DEBOUNCE_INTERVAL: Duration = Duration::from_secs(2);

/// Errors from window state persistence.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub(crate) enum WindowStateError {
    /// Failed to determine the config directory.
    #[snafu(display("failed to determine config directory"))]
    NoConfigDir,

    /// Failed to create the config directory.
    #[snafu(display("failed to create directory {}: {source}", path.display()))]
    CreateDir {
        /// Directory path that could not be created.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to read the state file.
    #[snafu(display("failed to read {}: {source}", path.display()))]
    ReadFile {
        /// File path that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to write the state file.
    #[snafu(display("failed to write {}: {source}", path.display()))]
    WriteFile {
        /// File path that could not be written.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Failed to parse the TOML state file.
    #[snafu(display("failed to parse window state: {source}"))]
    Parse {
        /// Underlying TOML deserialization error.
        source: toml::de::Error,
    },

    /// Failed to serialize window state to TOML.
    #[snafu(display("failed to serialize window state: {source}"))]
    Serialize {
        /// Underlying TOML serialization error.
        source: toml::ser::Error,
    },
}

/// Resolve the window state file path: `~/.config/aletheia-desktop/window-state.toml`.
fn state_path() -> Result<PathBuf, WindowStateError> {
    let dir = dirs::config_dir().ok_or(WindowStateError::NoConfigDir)?;
    Ok(dir.join("aletheia-desktop").join("window-state.toml"))
}

/// Load window state from disk, returning defaults if the file does not exist.
pub(crate) fn load() -> Result<WindowState, WindowStateError> {
    let path = state_path()?;

    if !path.exists() {
        return Ok(WindowState::default());
    }

    let content = std::fs::read_to_string(&path).context(ReadFileSnafu { path: &path })?;
    let state: WindowState = toml::from_str(&content).context(ParseSnafu)?;
    Ok(state)
}

/// Load window state, returning defaults on any error.
#[must_use]
pub(crate) fn load_or_default() -> WindowState {
    match load() {
        Ok(state) => state,
        Err(e) => {
            tracing::warn!("failed to load window state, using defaults: {e}");
            WindowState::default()
        }
    }
}

/// Save window state to disk synchronously.
fn save_sync(state: &WindowState) -> Result<(), WindowStateError> {
    save_to(&state_path()?, state)
}

/// Save window state to an explicit path.
///
/// WHY separate from [`save_sync`]: `state_path` resolves the real user config
/// directory, so a test exercising it would write to the developer's own
/// `~/.config`. Splitting the path resolution out is what lets the write itself
/// be covered.
fn save_to(path: &Path, state: &WindowState) -> Result<(), WindowStateError> {
    let parent = path.parent().ok_or(WindowStateError::NoConfigDir)?;

    std::fs::create_dir_all(parent).context(CreateDirSnafu {
        path: parent.to_path_buf(),
    })?;

    let content = toml::to_string_pretty(state).context(SerializeSnafu)?;

    // WHY: the mode restricts the window-state file to owner-only on unix, and
    // is applied to the replacement before the rename so the file is never
    // briefly visible at the default mode. It is ignored on Windows, where the
    // file lands in `%APPDATA%` under user-private default ACLs.
    bathron::atomic::write_atomic(path, content.as_bytes(), Some(0o600)).map_err(|source| {
        WindowStateError::WriteFile {
            path: path.to_path_buf(),
            source: std::io::Error::other(source),
        }
    })?;

    Ok(())
}

/// Guard that aborts the background flush task when the last clone drops.
///
/// WHY: `JoinHandle` is not `Clone`, but `DebouncedWriter` must be `Clone`
/// for Dioxus hooks. Wrapping the `AbortHandle` in `Arc<AbortOnDrop>` gives
/// automatic cleanup: the task is aborted only when every clone is gone.
struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Debounced window state writer.
///
/// Buffers state changes and flushes to disk at most once per
/// [`DEBOUNCE_INTERVAL`]. Call [`mark_dirty`](Self::mark_dirty) whenever
/// the window state changes; the background task handles the rest.
///
/// The background flush task is automatically aborted when the last
/// `DebouncedWriter` clone is dropped.
// WHY: Clone is derived because Dioxus `use_hook` requires `Clone + 'static`.
// All fields are `Arc`-wrapped, so cloning is cheap (reference count bump).
#[derive(Clone)]
pub(crate) struct DebouncedWriter {
    state: Arc<Mutex<WindowState>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern -- sync-only state snapshot, never held across await (#3988)
    dirty: Arc<Notify>,
    /// Whether there are unsaved changes.
    has_pending: Arc<std::sync::atomic::AtomicBool>,
    /// Aborts the background flush task when the last `DebouncedWriter` clone drops.
    _flush_guard: Arc<AbortOnDrop>,
}

impl DebouncedWriter {
    /// Create a new debounced writer and spawn the background flush task.
    ///
    /// The background task runs until the last `DebouncedWriter` clone is dropped,
    /// at which point it is aborted via the `AbortOnDrop` guard.
    #[must_use]
    pub(crate) fn new(initial: WindowState) -> Self {
        let state = Arc::new(Mutex::new(initial)); // kanon:ignore RUST/no-arc-mutex-anti-pattern -- sync-only state snapshot, never held across await (#3988)
        let dirty = Arc::new(Notify::new());
        let has_pending = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // WHY: Spawn a tokio task (not Dioxus coroutine) so it runs independently
        // of component lifecycle and can flush on app shutdown. The AbortHandle
        // is stored so the task is cancelled when all DebouncedWriter clones drop.
        let handle = tokio::spawn({
            let state = Arc::clone(&state);
            let dirty = Arc::clone(&dirty);
            let has_pending = Arc::clone(&has_pending);
            async move {
                loop {
                    dirty.notified().await;
                    tokio::time::sleep(DEBOUNCE_INTERVAL).await;

                    if has_pending.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        let snapshot = {
                            let guard = state.lock().unwrap_or_else(|e| e.into_inner());
                            guard.clone()
                        };
                        if let Err(e) = save_sync(&snapshot) {
                            tracing::warn!("failed to save window state: {e}");
                        }
                    }
                }
            }
            .instrument(tracing::debug_span!("window_state_flush"))
        });

        Self {
            state,
            dirty,
            has_pending,
            _flush_guard: Arc::new(AbortOnDrop(handle.abort_handle())),
        }
    }

    /// Update the buffered state. The write will be flushed after the debounce interval.
    pub(crate) fn update(&self, f: impl FnOnce(&mut WindowState)) {
        {
            let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut guard);
        }
        self.mark_dirty();
    }

    /// Mark the state as dirty, scheduling a debounced flush.
    pub(crate) fn mark_dirty(&self) {
        self.has_pending
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.dirty.notify_one();
    }

    /// Return a clone of the current buffered state.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    #[must_use]
    pub(crate) fn snapshot(&self) -> WindowState {
        let guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.clone()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn round_trip_toml() {
        let mut state = WindowState {
            x: 50,
            y: 75,
            width: 1920,
            height: 1080,
            maximized: true,
            active_view: "/metrics".to_string(),
            sidebar_collapsed: true,
            sidebar_width: Some(280),
            ..WindowState::default()
        };
        state
            .active_sessions
            .insert("syn".into(), "sess-abc".into());

        let serialized = toml::to_string_pretty(&state).unwrap();
        let deserialized: WindowState = toml::from_str(&serialized).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn save_and_load_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.toml");

        let state = WindowState {
            x: 200,
            y: 100,
            width: 1400,
            height: 900,
            active_view: "/planning".to_string(),
            ..WindowState::default()
        };

        save_to(&path, &state).unwrap();

        let loaded_content = std::fs::read_to_string(&path).unwrap();
        let loaded: WindowState = toml::from_str(&loaded_content).unwrap();
        assert_eq!(loaded.x, 200);
        assert_eq!(loaded.width, 1400);
        assert_eq!(loaded.active_view, "/planning");
    }

    /// The written file is owner-only, and stays so when it replaces an
    /// existing file — the mode has to be applied to each replacement, not just
    /// to the first create.
    #[cfg(unix)]
    #[test]
    fn saved_state_is_owner_only_on_every_write() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window-state.toml");

        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        save_to(&path, &WindowState::default()).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "expected 0o600, got {:o}", mode & 0o777);
    }

    #[test]
    fn empty_toml_uses_defaults() {
        let state: WindowState = toml::from_str("").unwrap();
        assert_eq!(state, WindowState::default());
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let toml_str = r#"
width = 1600
active_view = "/ops"
"#;
        let state: WindowState = toml::from_str(toml_str).unwrap();
        assert_eq!(state.width, 1600);
        assert_eq!(state.height, 800); // default
        assert_eq!(state.active_view, "/ops");
        assert_eq!(state.x, 100); // default
    }

    #[tokio::test]
    async fn debounced_writer_update_and_snapshot() {
        let initial = WindowState::default();
        let writer = DebouncedWriter::new(initial);

        writer.update(|s| {
            s.width = 1920;
            s.active_view = "/files".to_string();
        });

        let snap = writer.snapshot();
        assert_eq!(snap.width, 1920);
        assert_eq!(snap.active_view, "/files");
    }

    #[test]
    fn state_path_is_under_aletheia_desktop() {
        // NOTE: This test verifies the path structure, not the actual directory.
        if let Ok(path) = state_path() {
            let path_str = path.to_string_lossy();
            assert!(path_str.contains("aletheia-desktop"));
            assert!(path_str.ends_with("window-state.toml"));
        }
    }
}
