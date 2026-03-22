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

        // Populate defaults.
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

        // WHY: build reverse lookup in two passes — defaults first, then overrides.
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

    fn defaults() -> Vec<(Action, Vec<(KeyModifiers, KeyCode)>)> {
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
            (
                Action::OpenSystemStatus,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('i'))],
            ),
            (
                Action::MemoryOpen,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('m'))],
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
