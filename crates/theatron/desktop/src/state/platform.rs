//! Platform integration state: tray, hotkeys, window persistence, quick input.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use theatron_core::id::NousId;

use super::agents::AgentStatus;

/// Aggregate agent status for the system tray icon.
///
/// Priority ordering: Disconnected > Error > Active > Normal. The tray icon
/// reflects the most urgent status across all agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TrayIconStatus {
    /// All agents healthy, none processing.
    #[default]
    Normal,
    /// At least one agent is actively processing a turn.
    Active,
    /// At least one agent is in an error state.
    Error,
    /// Disconnected from the pylon server.
    Disconnected,
}

impl TrayIconStatus {
    /// Derive aggregate status from individual agent statuses and connection state.
    #[must_use]
    pub(crate) fn from_agents(statuses: &[AgentStatus], connected: bool) -> Self {
        if !connected {
            return Self::Disconnected;
        }
        if statuses.iter().any(|s| matches!(s, AgentStatus::Error)) {
            return Self::Error;
        }
        if statuses.iter().any(|s| matches!(s, AgentStatus::Active)) {
            return Self::Active;
        }
        Self::Normal
    }
}

/// Reactive state for the system tray.
#[derive(Debug, Clone, Default)]
pub struct TrayState {
    /// Aggregate icon status derived from all agent states.
    pub icon_status: TrayIconStatus,
    /// Total agent count for tooltip.
    pub agent_count: usize,
    /// Number of agents currently processing a turn.
    pub processing_count: usize,
    /// Whether the main window is currently visible.
    pub window_visible: bool,
}



/// Registration status for a global hotkey.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HotkeyRegistration {
    /// Hotkey registered and active.
    Registered,
    /// Registration failed (key combination taken by another app, or platform limitation).
    Failed {
        /// Human-readable failure reason.
        reason: String,
    },
    /// Platform does not support global hotkeys (e.g. Wayland without portal).
    Unavailable,
}

/// Identifiers for the registered global hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HotkeyAction {
    /// Toggle window visibility (summon/dismiss).
    SummonWindow,
    /// Open the quick input overlay.
    QuickInput,
    /// Abort all active streaming responses.
    AbortStreaming,
}



/// Reactive state for global hotkey registration.
#[derive(Debug, Clone, Default)]
pub struct HotkeyState {
    /// Registration status for each hotkey action.
    pub registrations: Vec<(HotkeyAction, HotkeyRegistration)>,
}

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
    /// Close the overlay and clear input.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.input_text.clear();
    }
}

/// Close behavior when the user clicks the window close button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CloseBehavior {
    /// Minimize to system tray instead of quitting.
    #[default]
    MinimizeToTray,
    /// Quit the application.
    Quit,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    // -- TrayIconStatus --

    #[test]
    fn tray_icon_all_idle_connected() {
        let statuses = vec![AgentStatus::Idle, AgentStatus::Idle];
        assert_eq!(
            TrayIconStatus::from_agents(&statuses, true),
            TrayIconStatus::Normal
        );
    }

    #[test]
    fn tray_icon_any_active() {
        let statuses = vec![AgentStatus::Idle, AgentStatus::Active];
        assert_eq!(
            TrayIconStatus::from_agents(&statuses, true),
            TrayIconStatus::Active
        );
    }

    #[test]
    fn tray_icon_error_takes_priority_over_active() {
        let statuses = vec![AgentStatus::Active, AgentStatus::Error];
        assert_eq!(
            TrayIconStatus::from_agents(&statuses, true),
            TrayIconStatus::Error
        );
    }

    #[test]
    fn tray_icon_disconnected_takes_highest_priority() {
        let statuses = vec![AgentStatus::Error, AgentStatus::Active];
        assert_eq!(
            TrayIconStatus::from_agents(&statuses, false),
            TrayIconStatus::Disconnected
        );
    }

    #[test]
    fn tray_icon_empty_agents_connected() {
        assert_eq!(
            TrayIconStatus::from_agents(&[], true),
            TrayIconStatus::Normal
        );
    }

    // -- WindowState --

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
        let mut state = WindowState::default();
        state.x = 200;
        state.y = 150;
        state.width = 1600;
        state.height = 900;
        state.maximized = true;
        state.active_view = "/planning".to_string();
        state.sidebar_collapsed = true;
        state.sidebar_width = Some(300);
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

    // -- CloseBehavior --

    #[test]
    fn close_behavior_default_is_minimize_to_tray() {
        assert_eq!(CloseBehavior::default(), CloseBehavior::MinimizeToTray);
    }

    #[test]
    fn close_behavior_round_trip_toml() {
        // WHY: TOML requires table structure -- bare enums can't be top-level.
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            close: CloseBehavior,
        }

        let wrapper = Wrapper {
            close: CloseBehavior::Quit,
        };
        let serialized = toml::to_string(&wrapper).unwrap();
        assert!(serialized.contains("quit"));
        let deserialized: Wrapper = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.close, CloseBehavior::Quit);

        let wrapper_default = Wrapper {
            close: CloseBehavior::MinimizeToTray,
        };
        let serialized = toml::to_string(&wrapper_default).unwrap();
        assert!(serialized.contains("minimize_to_tray"));
    }
}
