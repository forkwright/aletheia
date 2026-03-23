//! Settings state: server configurations, appearance, keybindings, and wizard flow.

use std::collections::HashMap;

use crate::theme::ThemeMode;

/// A single saved server connection entry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ServerEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) auth_token: Option<String>,
    pub(crate) last_connected: Option<String>,
}

/// In-memory store of saved server connections.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ServerConfigStore {
    pub(crate) active_id: Option<String>,
    pub(crate) servers: Vec<ServerEntry>,
}

impl ServerConfigStore {
    /// Add a new server; returns the assigned id.
    pub(crate) fn add(
        &mut self,
        name: String,
        url: String,
        auth_token: Option<String>,
    ) -> String {
        let id = gen_server_id();
        self.servers.push(ServerEntry {
            id: id.clone(),
            name,
            url,
            auth_token,
            last_connected: None,
        });
        id
    }

    /// Remove a server by id.
    ///
    /// Returns false and does nothing if `id` is the active server.
    pub(crate) fn remove(&mut self, id: &str) -> bool {
        if self.active_id.as_deref() == Some(id) {
            return false;
        }
        self.servers.retain(|s| s.id != id);
        true
    }

    /// Update name, URL, and token for an existing server.
    ///
    /// Returns false if `id` is not found.
    pub(crate) fn update(
        &mut self,
        id: &str,
        name: String,
        url: String,
        auth_token: Option<String>,
    ) -> bool {
        if let Some(entry) = self.servers.iter_mut().find(|s| s.id == id) {
            entry.name = name;
            entry.url = url;
            entry.auth_token = auth_token;
            true
        } else {
            false
        }
    }

    /// Set the active server by id. Returns false if id is not found.
    pub(crate) fn set_active(&mut self, id: &str) -> bool {
        if self.servers.iter().any(|s| s.id == id) {
            self.active_id = Some(id.to_string());
            true
        } else {
            false
        }
    }

    /// Get the currently active server entry.
    pub(crate) fn active(&self) -> Option<&ServerEntry> {
        self.active_id
            .as_deref()
            .and_then(|id| self.servers.iter().find(|s| s.id == id))
    }
}

/// Transient health status for the server management panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ServerHealth {
    #[default]
    Unchecked,
    Healthy,
    Degraded,
    Unreachable,
}

impl ServerHealth {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unchecked => "—",
            Self::Healthy => "Healthy",
            Self::Degraded => "Degraded",
            Self::Unreachable => "Unreachable",
        }
    }

    pub(crate) fn color(self) -> &'static str {
        match self {
            Self::Unchecked => "#666",
            Self::Healthy => "#4ade80",
            Self::Degraded => "#facc15",
            Self::Unreachable => "#f87171",
        }
    }
}

/// UI density scale preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum UiDensity {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

impl UiDensity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
            Self::Spacious => "Spacious",
        }
    }

    /// Base spacing unit in pixels.
    pub(crate) fn spacing_px(self) -> u8 {
        match self {
            Self::Compact => 4,
            Self::Comfortable => 8,
            Self::Spacious => 12,
        }
    }
}

/// Persisted appearance preferences.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AppearanceSettings {
    pub(crate) theme: String,
    pub(crate) font_size: u8,
    pub(crate) density: UiDensity,
    pub(crate) accent_color: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            font_size: 14,
            density: UiDensity::Comfortable,
            accent_color: "#5b6af0".to_string(),
        }
    }
}

impl AppearanceSettings {
    /// Resolve the theme string to a `ThemeMode`.
    pub(crate) fn theme_mode(&self) -> ThemeMode {
        match self.theme.as_str() {
            "dark" => ThemeMode::Dark,
            "light" => ThemeMode::Light,
            _ => ThemeMode::System,
        }
    }

    /// Set font size, clamped to [12, 20].
    pub(crate) fn set_font_size(&mut self, size: u8) {
        self.font_size = size.clamp(12, 20);
    }
}

/// A key combination: optional modifiers plus a named key.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct KeyCombo {
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
    pub(crate) shift: bool,
    pub(crate) key: String,
}

impl KeyCombo {
    /// Human-readable representation, e.g. "Ctrl+Shift+K".
    pub(crate) fn display(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if !self.key.is_empty() {
            parts.push(self.key.as_str());
        }
        parts.join("+")
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.key.is_empty()
    }
}

/// Category grouping for keybinding actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum KeyCategory {
    Global,
    Chat,
    Navigation,
    Panels,
    Memory,
    Planning,
}

impl KeyCategory {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Chat => "Chat",
            Self::Navigation => "Navigation",
            Self::Panels => "Panels",
            Self::Memory => "Memory",
            Self::Planning => "Planning",
        }
    }

    pub(crate) fn all() -> &'static [Self] {
        &[
            Self::Global,
            Self::Chat,
            Self::Navigation,
            Self::Panels,
            Self::Memory,
            Self::Planning,
        ]
    }
}

/// A named keybinding action with its default combo.
#[derive(Debug, Clone)]
pub(crate) struct KeyAction {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) category: KeyCategory,
    pub(crate) default: KeyCombo,
}

/// Stores user-defined overrides for keybinding actions.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct KeybindingStore {
    pub(crate) overrides: HashMap<String, KeyCombo>,
}

impl KeybindingStore {
    /// Effective combo for an action: override if present, else default.
    pub(crate) fn effective<'a>(&'a self, action: &'a KeyAction) -> &'a KeyCombo {
        self.overrides.get(action.id).unwrap_or(&action.default)
    }

    /// Set a custom combo for an action.
    pub(crate) fn set(&mut self, action_id: &str, combo: KeyCombo) {
        self.overrides.insert(action_id.to_string(), combo);
    }

    /// Reset an action to its default by removing the override.
    pub(crate) fn reset(&mut self, action_id: &str) {
        self.overrides.remove(action_id);
    }

    /// Reset all overrides for actions in the given category.
    pub(crate) fn reset_category(&mut self, category: KeyCategory, actions: &[KeyAction]) {
        for action in actions {
            if action.category == category {
                self.overrides.remove(action.id);
            }
        }
    }

    /// Find any action already using `combo`, excluding `excluding_id`.
    pub(crate) fn conflict<'a>(
        &self,
        combo: &KeyCombo,
        excluding_id: &str,
        actions: &'a [KeyAction],
    ) -> Option<&'a KeyAction> {
        actions
            .iter()
            .find(|a| a.id != excluding_id && self.effective(a) == combo)
    }
}

/// Setup wizard step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WizardStep {
    #[default]
    Server,
    Auth,
    Discovery,
    Appearance,
    Ready,
}

impl WizardStep {
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Server => 0,
            Self::Auth => 1,
            Self::Discovery => 2,
            Self::Appearance => 3,
            Self::Ready => 4,
        }
    }

    pub(crate) fn total() -> usize {
        5
    }

    pub(crate) fn next(self) -> Option<Self> {
        match self {
            Self::Server => Some(Self::Auth),
            Self::Auth => Some(Self::Discovery),
            Self::Discovery => Some(Self::Appearance),
            Self::Appearance => Some(Self::Ready),
            Self::Ready => None,
        }
    }

    pub(crate) fn prev(self) -> Option<Self> {
        match self {
            Self::Server => None,
            Self::Auth => Some(Self::Server),
            Self::Discovery => Some(Self::Auth),
            Self::Appearance => Some(Self::Discovery),
            Self::Ready => Some(Self::Appearance),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::Auth => "Auth",
            Self::Discovery => "Discovery",
            Self::Appearance => "Appearance",
            Self::Ready => "Ready",
        }
    }
}

/// Transient data collected while stepping through the setup wizard.
#[derive(Debug, Clone, Default)]
pub(crate) struct WizardData {
    pub(crate) server_url: String,
    pub(crate) server_name: String,
    pub(crate) auth_token: String,
    pub(crate) skip_auth: bool,
    pub(crate) discovered_agents: Vec<String>,
    pub(crate) selected_theme: String,
    pub(crate) selected_density: UiDensity,
}

/// Preset accent color swatches: (label, hex).
pub(crate) const ACCENT_PRESETS: &[(&str, &str)] = &[
    ("Iris", "#5b6af0"),
    ("Jade", "#10b981"),
    ("Amber", "#f59e0b"),
    ("Rose", "#f43f5e"),
    ("Sky", "#0ea5e9"),
    ("Brass", "#9A7B4F"),
    ("Slate", "#64748b"),
    ("Violet", "#8b5cf6"),
];

/// Generate a stable id for a new server entry.
fn gen_server_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("srv_{ms:013x}")
}

/// Default set of keybinding actions.
pub(crate) fn default_actions() -> Vec<KeyAction> {
    vec![
        KeyAction {
            id: "quick_search",
            label: "Quick search",
            category: KeyCategory::Global,
            default: KeyCombo {
                ctrl: true,
                key: "k".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "open_settings",
            label: "Open settings",
            category: KeyCategory::Global,
            default: KeyCombo {
                ctrl: true,
                key: ",".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "new_session",
            label: "New session",
            category: KeyCategory::Chat,
            default: KeyCombo {
                ctrl: true,
                key: "n".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "send_message",
            label: "Send message",
            category: KeyCategory::Chat,
            default: KeyCombo {
                key: "Enter".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "focus_input",
            label: "Focus input",
            category: KeyCategory::Chat,
            default: KeyCombo {
                key: "/".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "nav_chat",
            label: "Go to Chat",
            category: KeyCategory::Navigation,
            default: KeyCombo {
                ctrl: true,
                key: "1".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "nav_planning",
            label: "Go to Planning",
            category: KeyCategory::Navigation,
            default: KeyCombo {
                ctrl: true,
                key: "2".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "nav_memory",
            label: "Go to Memory",
            category: KeyCategory::Navigation,
            default: KeyCombo {
                ctrl: true,
                key: "3".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "nav_files",
            label: "Go to Files",
            category: KeyCategory::Navigation,
            default: KeyCombo {
                ctrl: true,
                key: "4".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "toggle_sidebar",
            label: "Toggle sidebar",
            category: KeyCategory::Panels,
            default: KeyCombo {
                ctrl: true,
                key: "b".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "toggle_details",
            label: "Toggle details pane",
            category: KeyCategory::Panels,
            default: KeyCombo {
                ctrl: true,
                key: ".".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "memory_search",
            label: "Search memory",
            category: KeyCategory::Memory,
            default: KeyCombo {
                ctrl: true,
                alt: true,
                key: "m".to_string(),
                ..Default::default()
            },
        },
        KeyAction {
            id: "new_plan",
            label: "New plan",
            category: KeyCategory::Planning,
            default: KeyCombo {
                ctrl: true,
                alt: true,
                key: "p".to_string(),
                ..Default::default()
            },
        },
    ]
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn server_store_add_increases_count() {
        let mut store = ServerConfigStore::default();
        store.add("Local".to_string(), "http://localhost:3000".to_string(), None);
        assert_eq!(store.servers.len(), 1);
    }

    #[test]
    fn server_store_remove_non_active() {
        let mut store = ServerConfigStore::default();
        let id = store.add("A".to_string(), "http://a".to_string(), None);
        assert!(store.remove(&id));
        assert!(store.servers.is_empty());
    }

    #[test]
    fn server_store_cannot_remove_active() {
        let mut store = ServerConfigStore::default();
        let id = store.add("A".to_string(), "http://a".to_string(), None);
        store.set_active(&id);
        assert!(!store.remove(&id));
        assert_eq!(store.servers.len(), 1);
    }

    #[test]
    fn server_store_set_active_unknown_returns_false() {
        let mut store = ServerConfigStore::default();
        assert!(!store.set_active("no-such-id"));
    }

    #[test]
    fn server_store_active_returns_entry() {
        let mut store = ServerConfigStore::default();
        let id = store.add("X".to_string(), "http://x".to_string(), None);
        store.set_active(&id);
        assert_eq!(store.active().unwrap().name, "X");
    }

    #[test]
    fn server_store_update_changes_fields() {
        let mut store = ServerConfigStore::default();
        let id = store.add("Old".to_string(), "http://old".to_string(), None);
        store.update(
            &id,
            "New".to_string(),
            "http://new".to_string(),
            Some("tok".to_string()),
        );
        let entry = store.servers.iter().find(|s| s.id == id).unwrap();
        assert_eq!(entry.name, "New");
        assert_eq!(entry.url, "http://new");
        assert_eq!(entry.auth_token.as_deref(), Some("tok"));
    }

    #[test]
    fn appearance_theme_mode_round_trip() {
        let mut s = AppearanceSettings::default();
        s.theme = "dark".to_string();
        assert_eq!(s.theme_mode(), ThemeMode::Dark);
        s.theme = "light".to_string();
        assert_eq!(s.theme_mode(), ThemeMode::Light);
        s.theme = "system".to_string();
        assert_eq!(s.theme_mode(), ThemeMode::System);
    }

    #[test]
    fn appearance_font_size_clamped() {
        let mut s = AppearanceSettings::default();
        s.set_font_size(10);
        assert_eq!(s.font_size, 12);
        s.set_font_size(25);
        assert_eq!(s.font_size, 20);
        s.set_font_size(16);
        assert_eq!(s.font_size, 16);
    }

    #[test]
    fn ui_density_spacing_increases() {
        assert!(UiDensity::Compact.spacing_px() < UiDensity::Comfortable.spacing_px());
        assert!(UiDensity::Comfortable.spacing_px() < UiDensity::Spacious.spacing_px());
    }

    #[test]
    fn keybinding_override_and_reset() {
        let actions = default_actions();
        let mut store = KeybindingStore::default();
        let custom = KeyCombo {
            ctrl: true,
            key: "x".to_string(),
            ..Default::default()
        };
        store.set("quick_search", custom.clone());
        let action = actions.iter().find(|a| a.id == "quick_search").unwrap();
        assert_eq!(store.effective(action), &custom);
        store.reset("quick_search");
        assert_eq!(store.effective(action), &action.default);
    }

    #[test]
    fn keybinding_conflict_detection() {
        let actions = default_actions();
        let store = KeybindingStore::default();
        // The default combo for actions[0] should conflict with actions[0] itself
        // but NOT when we exclude actions[0].id.
        let combo = actions[0].default.clone();
        let conflict = store.conflict(&combo, "no-such-id", &actions);
        assert!(conflict.is_some());
        let no_conflict = store.conflict(&combo, actions[0].id, &actions);
        assert!(no_conflict.map_or(true, |c| c.id != actions[0].id));
    }

    #[test]
    fn keybinding_reset_category() {
        let actions = default_actions();
        let mut store = KeybindingStore::default();
        let custom = KeyCombo {
            ctrl: true,
            key: "z".to_string(),
            ..Default::default()
        };
        store.set("nav_chat", custom);
        store.reset_category(KeyCategory::Navigation, &actions);
        let action = actions.iter().find(|a| a.id == "nav_chat").unwrap();
        assert_eq!(store.effective(action), &action.default);
    }

    #[test]
    fn wizard_step_progression() {
        assert_eq!(WizardStep::Server.next(), Some(WizardStep::Auth));
        assert_eq!(WizardStep::Server.prev(), None);
        assert_eq!(WizardStep::Ready.next(), None);
        assert_eq!(WizardStep::Ready.prev(), Some(WizardStep::Appearance));
    }

    #[test]
    fn wizard_step_index_bounds() {
        assert_eq!(WizardStep::Server.index(), 0);
        assert_eq!(WizardStep::Ready.index(), WizardStep::total() - 1);
    }

    #[test]
    fn key_combo_display() {
        let combo = KeyCombo {
            ctrl: true,
            shift: true,
            key: "K".to_string(),
            alt: false,
        };
        assert_eq!(combo.display(), "Ctrl+Shift+K");
    }

    #[test]
    fn key_combo_empty_check() {
        assert!(KeyCombo::default().is_empty());
        let full = KeyCombo {
            key: "a".to_string(),
            ..Default::default()
        };
        assert!(!full.is_empty());
    }
}
