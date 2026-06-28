//! Platform integration state: window persistence and quick input.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use skene::id::NousId;

/// Persisted window geometry and UI state.
///
/// Saved to `~/.config/aletheia-desktop/window-state.toml` on quit and
/// periodically (debounced). Restored on launch before the window is shown
/// to prevent visible repositioning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowState {
    /// Window X position in screen coordinates.
    #[serde(default = "default_x")]
    pub x: i32,
    /// Window Y position in screen coordinates.
    #[serde(default = "default_y")]
    pub y: i32,
    /// Window width in logical pixels.
    #[serde(default = "default_width")]
    pub width: u32,
    /// Window height in logical pixels.
    #[serde(default = "default_height")]
    pub height: u32,
    /// Whether the window was maximized.
    #[serde(default)]
    pub maximized: bool,
    /// Active view route path (e.g. "/", "/files", "/planning").
    #[serde(default = "default_active_view")]
    pub active_view: String,
    /// Whether the sidebar is collapsed.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// Sidebar width override in pixels. `None` uses the default 220px.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<u32>,
    /// Last active session ID per agent (keyed by agent ID string).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub active_sessions: HashMap<String, String>,
}

fn default_x() -> i32 {
    100
}

fn default_y() -> i32 {
    100
}

fn default_width() -> u32 {
    1200
}

fn default_height() -> u32 {
    800
}

fn default_active_view() -> String {
    "/".to_string()
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: default_x(),
            y: default_y(),
            width: default_width(),
            height: default_height(),
            maximized: false,
            active_view: default_active_view(),
            sidebar_collapsed: false,
            sidebar_width: None,
            active_sessions: HashMap::new(),
        }
    }
}

/// Reactive state for the quick input overlay.
///
/// The current desktop build does not register native global hotkeys or a tray
/// menu launcher. Components may show this overlay only through in-window state.
#[derive(Debug, Clone, Default)]
pub struct QuickInputState {
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Currently selected agent for the input.
    pub selected_agent: Option<NousId>,
    /// Current text in the input field.
    pub input_text: String,
}

impl QuickInputState {
    /// Open the overlay, optionally pre-selecting an agent.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn open(&mut self, agent: Option<NousId>) {
        self.visible = true;
        self.selected_agent = agent;
    }

    /// Take the current input text, leaving it empty. Returns `None` if the
    /// input was already empty.
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn take_input(&mut self) -> Option<String> {
        if self.input_text.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.input_text))
        }
    }

    /// Close the overlay and clear input.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.input_text.clear();
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn window_state_default() {
        let state = WindowState::default();
        assert_eq!(state.width, 1200);
        assert_eq!(state.height, 800);
        assert_eq!(state.active_view, "/");
        assert!(!state.maximized);
        assert!(!state.sidebar_collapsed);
        assert!(state.sidebar_width.is_none());
        assert!(state.active_sessions.is_empty());
    }

    #[test]
    fn window_state_round_trip_toml() {
        let mut state = WindowState {
            x: 200,
            y: 150,
            width: 1600,
            height: 900,
            maximized: true,
            active_view: "/planning".to_string(),
            sidebar_collapsed: true,
            sidebar_width: Some(300),
            ..WindowState::default()
        };
        state
            .active_sessions
            .insert("syn".to_string(), "sess-001".to_string());

        let serialized = toml::to_string_pretty(&state).unwrap();
        let deserialized: WindowState = toml::from_str(&serialized).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn window_state_partial_toml_uses_defaults() {
        let toml_str = r#"active_view = "/files""#;
        let state: WindowState = toml::from_str(toml_str).unwrap();
        assert_eq!(state.active_view, "/files");
        assert_eq!(state.width, 1200);
        assert_eq!(state.height, 800);
    }

    #[test]
    fn platform_state_excludes_unsupported_native_shell_surfaces() {
        let source = include_str!("platform.rs");

        for unsupported in [
            concat!("Tray", "State"),
            concat!("Hotkey", "State"),
            concat!("Minimize", "To", "Tray"),
        ] {
            assert!(
                !source.contains(unsupported),
                "{unsupported} must stay out of persisted/reactive state until the runtime implements it"
            );
        }
    }
}
