//! Action enum and KeyMap for context-aware key dispatch.
use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};

use super::helpers::parse_key_combo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    Quit,
    ToggleSidebar,
    ToggleThinking,
    ToggleOpsPane,
    TabNew,
    OpenHelp,
    OpenAgentPicker,
    OpenSystemStatus,
    MemoryOpen,
    MetricsOpen,
    PlanningOpen,
    RetrospectiveOpen,
    NewSession,
    OpenSessionPicker,
    CopyLastResponse,
    ComposeInEditor,
    ClearLine,
    DeleteToEnd,
    ScrollPageUp,
    ScrollPageDown,
    ScrollUp,
    ScrollDown,
    ScrollLineUp,
    ScrollLineDown,
    Yank,
    YankCycle,
    WordForward,
    WordBackward,
    ClearScreen,
    NewlineInsert,
}

impl Action {
    pub(crate) fn to_msg(self) -> crate::msg::Msg {
        use crate::msg::{Msg, OverlayKind};
        match self {
            Self::Quit => Msg::Quit,
            Self::ToggleSidebar => Msg::ToggleSidebar,
            Self::ToggleThinking => Msg::ToggleThinking,
            Self::ToggleOpsPane => Msg::ToggleOpsPane,
            Self::TabNew => Msg::TabNew,
            Self::OpenHelp => Msg::OpenOverlay(OverlayKind::Help),
            Self::OpenAgentPicker => Msg::OpenOverlay(OverlayKind::AgentPicker),
            Self::OpenSystemStatus => Msg::OpenOverlay(OverlayKind::SystemStatus),
            Self::MemoryOpen => Msg::MemoryOpen,
            Self::MetricsOpen => Msg::MetricsOpen,
            Self::PlanningOpen => Msg::PlanningOpen,
            Self::RetrospectiveOpen => Msg::RetrospectiveOpen,
            Self::NewSession => Msg::NewSession,
            Self::OpenSessionPicker => Msg::OpenOverlay(OverlayKind::SessionPicker),
            Self::CopyLastResponse => Msg::CopyLastResponse,
            Self::ComposeInEditor => Msg::ComposeInEditor,
            Self::ClearLine => Msg::ClearLine,
            Self::DeleteToEnd => Msg::DeleteToEnd,
            Self::ScrollPageUp => Msg::ScrollPageUp,
            Self::ScrollPageDown => Msg::ScrollPageDown,
            Self::ScrollUp => Msg::ScrollUp,
            Self::ScrollDown => Msg::ScrollDown,
            Self::ScrollLineUp => Msg::ScrollLineUp,
            Self::ScrollLineDown => Msg::ScrollLineDown,
            Self::Yank => Msg::Yank,
            Self::YankCycle => Msg::YankCycle,
            Self::WordForward => Msg::WordForward,
            Self::WordBackward => Msg::WordBackward,
            Self::ClearScreen => Msg::ClearScreen,
            Self::NewlineInsert => Msg::NewlineInsert,
        }
    }

    pub(crate) fn config_key(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::ToggleThinking => "toggle_thinking",
            Self::ToggleOpsPane => "toggle_ops_pane",
            Self::TabNew => "tab_new",
            Self::OpenHelp => "open_help",
            Self::OpenAgentPicker => "open_agent_picker",
            Self::OpenSystemStatus => "open_system_status",
            Self::MemoryOpen => "memory_open",
            Self::MetricsOpen => "metrics_open",
            Self::PlanningOpen => "planning_open",
            Self::RetrospectiveOpen => "retrospective_open",
            Self::NewSession => "new_session",
            Self::OpenSessionPicker => "open_session_picker",
            Self::CopyLastResponse => "copy_last_response",
            Self::ComposeInEditor => "compose_in_editor",
            Self::ClearLine => "clear_line",
            Self::DeleteToEnd => "delete_to_end",
            Self::ScrollPageUp => "scroll_page_up",
            Self::ScrollPageDown => "scroll_page_down",
            Self::ScrollUp => "scroll_up",
            Self::ScrollDown => "scroll_down",
            Self::ScrollLineUp => "scroll_line_up",
            Self::ScrollLineDown => "scroll_line_down",
            Self::Yank => "yank",
            Self::YankCycle => "yank_cycle",
            Self::WordForward => "word_forward",
            Self::WordBackward => "word_backward",
            Self::ClearScreen => "clear_screen",
            Self::NewlineInsert => "newline_insert",
        }
    }

    pub(crate) fn all() -> &'static [Action] {
        &[
            Self::Quit,
            Self::ToggleSidebar,
            Self::ToggleThinking,
            Self::ToggleOpsPane,
            Self::TabNew,
            Self::OpenHelp,
            Self::OpenAgentPicker,
            Self::OpenSystemStatus,
            Self::MemoryOpen,
            Self::MetricsOpen,
            Self::PlanningOpen,
            Self::RetrospectiveOpen,
            Self::NewSession,
            Self::OpenSessionPicker,
            Self::CopyLastResponse,
            Self::ComposeInEditor,
            Self::ClearLine,
            Self::DeleteToEnd,
            Self::ScrollPageUp,
            Self::ScrollPageDown,
            Self::ScrollUp,
            Self::ScrollDown,
            Self::ScrollLineUp,
            Self::ScrollLineDown,
            Self::Yank,
            Self::YankCycle,
            Self::WordForward,
            Self::WordBackward,
            Self::ClearScreen,
            Self::NewlineInsert,
        ]
    }
}

/// Configurable keymap built from defaults + TOML overrides.
///
/// Uses `(KeyModifiers, KeyCode)` as the dispatch key to avoid matching on
/// crossterm's `KeyEventKind`/`KeyEventState` fields.
pub(crate) struct KeyMap {
    dispatch: HashMap<(KeyModifiers, KeyCode), Action>,
}

impl KeyMap {
    /// Build a keymap from TOML overrides merged with defaults.
    pub(crate) fn build(overrides: &HashMap<String, String>) -> Self {
        let mut action_to_keys: HashMap<Action, Vec<(KeyModifiers, KeyCode)>> = HashMap::new();

        for &(action, ref keys) in &Self::defaults() {
            action_to_keys.entry(action).or_default().extend(keys);
        }

        // Apply overrides: replaces all default keys for the given action.
        for action in Action::all() {
            if let Some(key_str) = overrides.get(action.config_key()) {
                if let Some(parsed) = parse_key_combo(key_str) {
                    action_to_keys.insert(*action, vec![parsed]);
                } else {
                    tracing::warn!(
                        key = key_str,
                        action = action.config_key(),
                        "ignoring unrecognised keybinding"
                    );
                }
            }
        }

        // WHY: build reverse lookup in two passes -- defaults first, then overrides.
        // This ensures user overrides win when they claim a key already used by a default.
        let mut dispatch = HashMap::new();
        for (action, keys) in &action_to_keys {
            if !overrides.contains_key(action.config_key()) {
                for key in keys {
                    dispatch.insert(*key, *action);
                }
            }
        }
        for action in Action::all() {
            if overrides.contains_key(action.config_key())
                && let Some(keys) = action_to_keys.get(action)
            {
                for key in keys {
                    dispatch.insert(*key, *action);
                }
            }
        }

        Self { dispatch }
    }

    /// Look up the action bound to a `(modifiers, code)` pair.
    pub(crate) fn lookup(&self, modifiers: KeyModifiers, code: KeyCode) -> Option<Action> {
        self.dispatch.get(&(modifiers, code)).copied()
    }

    /// Human-readable form of an action's *first* default key combo (e.g.
    /// `"Ctrl+S"`, `"F1"`), or `None` if the default keymap binds nothing to it.
    ///
    /// WHY(#7222): the command-palette's shortcut badges used to be hand-typed
    /// string literals on each `Command`, independent of what was actually
    /// bound -- `:clear` claimed `Ctrl+N` (the *separate* `:new` command's real
    /// binding) and drifted silently. Deriving the badge from the same
    /// `defaults()` table `KeyMap::lookup` dispatches from means a palette
    /// entry can never claim a chord that isn't genuinely wired to its action.
    pub(crate) fn default_shortcut_display(action: Action) -> Option<String> {
        let (_, keys) = Self::defaults().into_iter().find(|(a, _)| *a == action)?;
        let (modifiers, code) = *keys.first()?;
        Some(display_key_combo(modifiers, code))
    }

    /// Default bindings; `pub(super)` so the registry drift test can walk them (#6819).
    pub(super) fn defaults() -> Vec<(Action, Vec<(KeyModifiers, KeyCode)>)> {
        vec![
            (
                Action::Quit,
                vec![
                    (KeyModifiers::CONTROL, KeyCode::Char('c')),
                    (KeyModifiers::CONTROL, KeyCode::Char('q')),
                ],
            ),
            (
                Action::ToggleSidebar,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('f'))],
            ),
            (
                Action::ToggleThinking,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('b'))],
            ),
            (
                Action::ToggleOpsPane,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('o'))],
            ),
            (
                Action::TabNew,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('t'))],
            ),
            (Action::OpenHelp, vec![(KeyModifiers::NONE, KeyCode::F(1))]),
            (
                Action::OpenAgentPicker,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('a'))],
            ),
            // WHY(#7220): Ctrl+I and Ctrl+M are CR/HT at the raw terminal-byte level
            // (0x09/0x0D), identical to Tab/Enter in every terminal that doesn't speak
            // the Kitty keyboard-enhancement protocol (koilon doesn't enable it). A
            // legacy terminal can never report these as `(CONTROL, Char('i'/'m'))`, so
            // binding them here was dead-or-worse: Ctrl+M never fired (Enter's own arm
            // intercepted the byte first) and Ctrl+I silently fell through to plain
            // Tab's `NextAgent` binding instead, switching the focused agent as a side
            // effect of trying to open System Status. F3/F4 use full CSI escape
            // sequences with no such alias, so opening these views can never again
            // decode as a different, unrelated keystroke; see
            // `no_registered_control_chord_aliases_a_reserved_key` in
            // `keybindings/mod.rs` for the regression guard.
            (
                Action::OpenSystemStatus,
                vec![(KeyModifiers::NONE, KeyCode::F(4))],
            ),
            (
                Action::MemoryOpen,
                vec![(KeyModifiers::NONE, KeyCode::F(3))],
            ),
            (
                Action::MetricsOpen,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('d'))],
            ),
            (
                Action::PlanningOpen,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('p'))],
            ),
            (
                Action::RetrospectiveOpen,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('h'))],
            ),
            (
                Action::NewSession,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('n'))],
            ),
            (
                Action::OpenSessionPicker,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('s'))],
            ),
            // WHY: Ctrl+Y reassigned to Yank (kill ring paste); CopyLastResponse
            // is available via the command palette (:copy-response).
            (Action::CopyLastResponse, vec![]),
            (
                Action::ComposeInEditor,
                vec![
                    (KeyModifiers::CONTROL, KeyCode::Char('e')),
                    (KeyModifiers::CONTROL, KeyCode::Char('g')),
                ],
            ),
            (
                Action::ClearLine,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('u'))],
            ),
            (
                Action::DeleteToEnd,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('k'))],
            ),
            (
                Action::ScrollPageUp,
                vec![(KeyModifiers::NONE, KeyCode::PageUp)],
            ),
            (
                Action::ScrollPageDown,
                vec![(KeyModifiers::NONE, KeyCode::PageDown)],
            ),
            (
                Action::ScrollLineUp,
                vec![(KeyModifiers::SHIFT, KeyCode::Up)],
            ),
            (
                Action::ScrollLineDown,
                vec![(KeyModifiers::SHIFT, KeyCode::Down)],
            ),
            (
                Action::Yank,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('y'))],
            ),
            (
                Action::YankCycle,
                vec![(KeyModifiers::ALT, KeyCode::Char('y'))],
            ),
            (
                Action::WordForward,
                vec![(KeyModifiers::ALT, KeyCode::Char('f'))],
            ),
            (
                Action::WordBackward,
                vec![(KeyModifiers::ALT, KeyCode::Char('b'))],
            ),
            (
                Action::ClearScreen,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('l'))],
            ),
            (
                Action::NewlineInsert,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('j'))],
            ),
        ]
    }
}

/// Render a `(modifiers, code)` combo the way the Help overlay and command
/// palette display keys (e.g. `"Ctrl+S"`, `"F1"`, `"Shift+Up"`).
fn display_key_combo(modifiers: KeyModifiers, code: KeyCode) -> String {
    let mut parts: Vec<String> = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    let key = match code {
        KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "Shift+Tab".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        other => format!("{other:?}"),
    };
    parts.push(key);
    parts.join("+")
}
