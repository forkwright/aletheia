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

    /// Short label for the tray tooltip suffix.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Normal => "idle",
            Self::Active => "processing",
            Self::Error => "error",
            Self::Disconnected => "disconnected",
        }
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

impl TrayState {
    /// Generate the tooltip text for the tray icon.
    #[must_use]
    pub(crate) fn tooltip(&self) -> String {
        format!(
            "Aletheia \u{2014} {} agents, {} processing",
            self.agent_count, self.processing_count
        )
    }

    /// Label for the show/hide toggle menu item.
    #[must_use]
    pub(crate) fn visibility_label(&self) -> &'static str {
        if self.window_visible {
            "Hide Window"
        } else {
            "Show Window"
        }
    }
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

impl HotkeyAction {
    /// Default key binding string for display.
    #[must_use]
    pub(crate) fn default_binding(self) -> &'static str {
        match self {
            Self::SummonWindow => "Ctrl+Shift+A",
            Self::QuickInput => "Ctrl+Shift+Space",
            Self::AbortStreaming => "Ctrl+Shift+Escape",
        }
    }

    /// Human-readable action description.
    #[must_use]
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::SummonWindow => "Show/hide window",
            Self::QuickInput => "Quick input overlay",
            Self::AbortStreaming => "Abort all streaming",
        }
    }

    /// All defined hotkey actions.
    #[must_use]
    pub(crate) fn all() -> &'static [Self] {
        &[Self::SummonWindow, Self::QuickInput, Self::AbortStreaming]
    }
}

/// Reactive state for global hotkey registration.
#[derive(Debug, Clone, Default)]
pub struct HotkeyState {
    /// Registration status for each hotkey action.
    pub registrations: Vec<(HotkeyAction, HotkeyRegistration)>,
}

impl HotkeyState {
    /// Whether any hotkey failed to register.
    #[must_use]
    pub(crate) fn has_failures(&self) -> bool {
        self.registrations
            .iter()
            .any(|(_, s)| matches!(s, HotkeyRegistration::Failed { .. }))
    }

    /// Whether global hotkeys are unavailable on this platform.
    #[must_use]
    pub(crate) fn is_unavailable(&self) -> bool {
        self.registrations
            .iter()
            .all(|(_, s)| matches!(s, HotkeyRegistration::Unavailable))
    }

    /// Count of successfully registered hotkeys.
    #[must_use]
    pub(crate) fn registered_count(&self) -> usize {
        self.registrations
            .iter()
            .filter(|(_, s)| matches!(s, HotkeyRegistration::Registered))
            .count()
    }
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

impl WindowState {
    /// Whether this state differs from `other` enough to warrant a save.
    #[must_use]
    pub(crate) fn differs_from(&self, other: &Self) -> bool {
        self != other
    }

    /// Update geometry fields from window position and size.
    pub(crate) fn update_geometry(&mut self, x: i32, y: i32, width: u32, height: u32) {
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
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
    /// Open the overlay, optionally pre-selecting an agent.
    pub(crate) fn open(&mut self, agent: Option<NousId>) {
        self.visible = true;
        self.input_text.clear();
        if let Some(id) = agent {
            self.selected_agent = Some(id);
        }
    }

    /// Close the overlay and clear input.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.input_text.clear();
    }

    /// Take the current input text, clearing the field. Returns `None` if empty.
    #[must_use]
    pub(crate) fn take_input(&mut self) -> Option<String> {
        if self.input_text.trim().is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.input_text);
        Some(text)
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

    #[test]
    fn tray_icon_labels() {
        assert_eq!(TrayIconStatus::Normal.label(), "idle");
        assert_eq!(TrayIconStatus::Active.label(), "processing");
        assert_eq!(TrayIconStatus::Error.label(), "error");
        assert_eq!(TrayIconStatus::Disconnected.label(), "disconnected");
    }

    // -- TrayState --

    #[test]
    fn tray_tooltip_format() {
        let state = TrayState {
            agent_count: 3,
            processing_count: 1,
            ..TrayState::default()
        };
        assert_eq!(state.tooltip(), "Aletheia \u{2014} 3 agents, 1 processing");
    }

    #[test]
    fn tray_visibility_label() {
        let mut state = TrayState::default();
        state.window_visible = true;
        assert_eq!(state.visibility_label(), "Hide Window");
        state.window_visible = false;
        assert_eq!(state.visibility_label(), "Show Window");
    }

    // -- HotkeyState --

    #[test]
    fn hotkey_state_no_failures_when_empty() {
        let state = HotkeyState::default();
        assert!(!state.has_failures());
        assert_eq!(state.registered_count(), 0);
    }

    #[test]
    fn hotkey_state_detects_failures() {
        let state = HotkeyState {
            registrations: vec![
                (HotkeyAction::SummonWindow, HotkeyRegistration::Registered),
                (
                    HotkeyAction::QuickInput,
                    HotkeyRegistration::Failed {
                        reason: "taken".into(),
                    },
                ),
            ],
        };
        assert!(state.has_failures());
        assert_eq!(state.registered_count(), 1);
    }

    #[test]
    fn hotkey_state_unavailable() {
        let state = HotkeyState {
            registrations: vec![
                (HotkeyAction::SummonWindow, HotkeyRegistration::Unavailable),
                (HotkeyAction::QuickInput, HotkeyRegistration::Unavailable),
                (
                    HotkeyAction::AbortStreaming,
                    HotkeyRegistration::Unavailable,
                ),
            ],
        };
        assert!(state.is_unavailable());
    }

    #[test]
    fn hotkey_action_defaults() {
        assert_eq!(HotkeyAction::SummonWindow.default_binding(), "Ctrl+Shift+A");
        assert_eq!(
            HotkeyAction::QuickInput.default_binding(),
            "Ctrl+Shift+Space"
        );
        assert_eq!(
            HotkeyAction::AbortStreaming.default_binding(),
            "Ctrl+Shift+Escape"
        );
    }

    #[test]
    fn hotkey_action_all_is_complete() {
        assert_eq!(HotkeyAction::all().len(), 3);
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

    #[test]
    fn window_state_update_geometry() {
        let mut state = WindowState::default();
        state.update_geometry(50, 75, 1920, 1080);
        assert_eq!(state.x, 50);
        assert_eq!(state.y, 75);
        assert_eq!(state.width, 1920);
        assert_eq!(state.height, 1080);
    }

    #[test]
    fn window_state_differs_from() {
        let a = WindowState::default();
        let mut b = a.clone();
        assert!(!a.differs_from(&b));
        b.width = 999;
        assert!(a.differs_from(&b));
    }

    #[test]
    fn window_state_omits_empty_sessions() {
        let state = WindowState::default();
        let serialized = toml::to_string(&state).unwrap();
        assert!(!serialized.contains("active_sessions"));
    }

    // -- QuickInputState --

    #[test]
    fn quick_input_open_and_close() {
        let mut state = QuickInputState::default();
        state.open(Some(NousId::from("syn")));
        assert!(state.visible);
        assert_eq!(state.selected_agent.as_deref(), Some("syn"));
        assert!(state.input_text.is_empty());

        state.input_text = "hello".to_string();
        state.close();
        assert!(!state.visible);
        assert!(state.input_text.is_empty());
    }

    #[test]
    fn quick_input_take_input() {
        let mut state = QuickInputState::default();
        state.input_text = "  test query  ".to_string();
        let taken = state.take_input();
        assert_eq!(taken.as_deref(), Some("  test query  "));
        assert!(state.input_text.is_empty());
    }

    #[test]
    fn quick_input_take_input_empty_returns_none() {
        let mut state = QuickInputState::default();
        assert!(state.take_input().is_none());

        state.input_text = "   ".to_string();
        assert!(state.take_input().is_none());
    }

    #[test]
    fn quick_input_open_preserves_existing_agent_when_none_given() {
        let mut state = QuickInputState {
            visible: false,
            selected_agent: Some(NousId::from("arc")),
            input_text: "old".to_string(),
        };
        state.open(None);
        assert!(state.visible);
        assert_eq!(state.selected_agent.as_deref(), Some("arc"));
        assert!(state.input_text.is_empty());
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
