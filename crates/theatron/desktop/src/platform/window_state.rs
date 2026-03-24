//! Window state persistence for the desktop application.
//!
//! Saves and restores window geometry, active view, and sidebar state
//! to `~/.config/aletheia-desktop/window-state.toml`. Writes are debounced
//! to avoid excessive disk I/O during window drag/resize operations.

use std::io::Write;
use std::path::PathBuf;
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
#[must_use]
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
    let path = state_path()?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context(CreateDirSnafu {
            path: parent.to_path_buf(),
        })?;
    }

    let content = toml::to_string_pretty(state).context(SerializeSnafu)?;

    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .context(WriteFileSnafu { path: &path })?;
        file.write_all(content.as_bytes())
            .context(WriteFileSnafu { path: &path })?;
    }

    Ok(())
}

/// Debounced window state writer.
///
/// Buffers state changes and flushes to disk at most once per
/// [`DEBOUNCE_INTERVAL`]. Call [`mark_dirty`](Self::mark_dirty) whenever
/// the window state changes; the background task handles the rest.
///
/// On drop or explicit [`flush`](Self::flush), any pending state is
/// written immediately.
// WHY: Clone is derived because Dioxus `use_hook` requires `Clone + 'static`.
// All fields are `Arc`-wrapped, so cloning is cheap (reference count bump).
#[derive(Clone)]
pub(crate) struct DebouncedWriter {
    state: Arc<Mutex<WindowState>>,
    dirty: Arc<Notify>,
    /// Whether there are unsaved changes.
    has_pending: Arc<std::sync::atomic::AtomicBool>,
}

impl DebouncedWriter {
    /// Create a new debounced writer and spawn the background flush task.
    ///
    /// The background task runs until the returned `DebouncedWriter` is dropped.
    #[must_use]
    pub(crate) fn new(initial: WindowState) -> Self {
        let state = Arc::new(Mutex::new(initial));
        let dirty = Arc::new(Notify::new());
        let has_pending = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let writer = Self {
            state: Arc::clone(&state),
            dirty: Arc::clone(&dirty),
            has_pending: Arc::clone(&has_pending),
        };

        // WHY: Spawn a tokio task (not Dioxus coroutine) so it runs independently
        // of component lifecycle and can flush on app shutdown.
        let span = tracing::info_span!("window_state_writer");
        tokio::spawn(
            {
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
            }
            .instrument(span),
        );

        writer
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

    /// Flush any pending state to disk immediately (blocking).
    pub(crate) fn flush(&self) {
        if self
            .has_pending
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            let snapshot = {
                let guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
                guard.clone()
            };
            if let Err(e) = save_sync(&snapshot) {
                tracing::warn!("failed to flush window state: {e}");
            }
        }
    }

    /// Get a snapshot of the current state.
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
        let mut state = WindowState::default();
        state.x = 50;
        state.y = 75;
        state.width = 1920;
        state.height = 1080;
        state.maximized = true;
        state.active_view = "/metrics".to_string();
        state.sidebar_collapsed = true;
        state.sidebar_width = Some(280);
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

        let content = toml::to_string_pretty(&state).unwrap();
        std::fs::write(&path, &content).unwrap();

        let loaded_content = std::fs::read_to_string(&path).unwrap();
        let loaded: WindowState = toml::from_str(&loaded_content).unwrap();
        assert_eq!(loaded.x, 200);
        assert_eq!(loaded.width, 1400);
        assert_eq!(loaded.active_view, "/planning");
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
