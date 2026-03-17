/// Context-aware keybinding registry: single source of truth for help overlay and status bar hints.
use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, Overlay, SelectionContext};

pub struct Keybinding {
    pub keys: &'static str,
    pub description: &'static str,
    pub contexts: &'static [KeyContext],
    pub show_in_status_bar: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyContext {
    Global,
    Chat,
    Selection,
    Filter,
    CommandPalette,
    SessionList,
    Input,
    Overlay,
    ToolApproval,
    PlanApproval,
    Settings,
    Operations,
    MemoryInspector,
    FactDetail,
}

impl KeyContext {
    pub fn section_label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Chat => "Chat",
            Self::Selection => "Selection",
            Self::Filter => "Filter",
            Self::CommandPalette => "Command Palette",
            Self::SessionList => "Session List",
            Self::Input => "Input",
            Self::Overlay => "Overlay",
            Self::ToolApproval => "Tool Approval",
            Self::PlanApproval => "Plan Approval",
            Self::Settings => "Settings",
            Self::Operations => "Operations",
            Self::MemoryInspector => "Memory Inspector",
            Self::FactDetail => "Fact Detail",
        }
    }

    fn display_order(self) -> u8 {
        match self {
            Self::ToolApproval
            | Self::PlanApproval
            | Self::Selection
            | Self::Filter
            | Self::CommandPalette
            | Self::SessionList
            | Self::Settings
            | Self::Operations
            | Self::MemoryInspector
            | Self::FactDetail => 0,
            Self::Chat => 1,
            Self::Input => 2,
            Self::Overlay => 3,
            Self::Global => 4,
        }
    }
}

pub fn all_keybindings() -> &'static [Keybinding] {
    &[
        Keybinding {
            keys: "?",
            description: "Help",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: ":",
            description: "Command palette",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "/",
            description: "Search sessions",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Ctrl+Y",
            description: "Copy last response",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Shift+Up",
            description: "Scroll up 1 line",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Shift+Down",
            description: "Scroll down 1 line",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "PgUp/PgDn",
            description: "Page scroll",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "End",
            description: "Scroll to bottom",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Mouse",
            description: "Scroll / click agent",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "\u{2191}/\u{2193}",
            description: "Select message",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "v",
            description: "Enter selection",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "j / \u{2193}",
            description: "Next message",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "k / \u{2191}",
            description: "Previous message",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Enter",
            description: "Context actions",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "c",
            description: "Copy message",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "y",
            description: "Yank code block",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "e",
            description: "Edit and resend",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "d",
            description: "Delete message",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "o",
            description: "Open links",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "i",
            description: "Inspect tool call",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "G / End",
            description: "Jump to newest",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Home",
            description: "Jump to oldest",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Esc",
            description: "Deselect",
            contexts: &[KeyContext::Selection],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Enter",
            description: "Lock filter",
            contexts: &[KeyContext::Filter],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Esc",
            description: "Clear filter",
            contexts: &[KeyContext::Filter],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "n / N",
            description: "Next / prev match",
            contexts: &[KeyContext::Filter],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Enter",
            description: "Execute command",
            contexts: &[KeyContext::CommandPalette],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Tab",
            description: "Autocomplete",
            contexts: &[KeyContext::CommandPalette],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Esc",
            description: "Close palette",
            contexts: &[KeyContext::CommandPalette],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Enter",
            description: "Open session",
            contexts: &[KeyContext::SessionList],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "d",
            description: "Delete session",
            contexts: &[KeyContext::SessionList],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "n",
            description: "New session",
            contexts: &[KeyContext::SessionList],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Enter",
            description: "Send message",
            contexts: &[KeyContext::Input],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "@agent Tab",
            description: "Mention completion",
            contexts: &[KeyContext::Input],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+U",
            description: "Clear input line",
            contexts: &[KeyContext::Input],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+W",
            description: "Delete word",
            contexts: &[KeyContext::Input],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+K",
            description: "Delete to end of line",
            contexts: &[KeyContext::Input],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+E",
            description: "Open $EDITOR",
            contexts: &[KeyContext::Input],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Up/Down",
            description: "Input history",
            contexts: &[KeyContext::Input],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Esc",
            description: "Close",
            contexts: &[KeyContext::Overlay],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Up/Down",
            description: "Navigate",
            contexts: &[KeyContext::Overlay],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Enter",
            description: "Select",
            contexts: &[KeyContext::Overlay],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "F1",
            description: "Help",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+A",
            description: "Switch agent",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Tab / Shift+Tab",
            description: "Next / prev agent",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+F",
            description: "Toggle sidebar",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+T",
            description: "New tab",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+W",
            description: "Close tab",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "gt / gT",
            description: "Next / prev tab",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Alt+1..9",
            description: "Jump to tab",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+B",
            description: "Toggle thinking",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+I",
            description: "System status",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+N",
            description: "New session",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+S",
            description: "Session list",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+O",
            description: "Toggle ops pane",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+C/Q",
            description: "Quit",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "j / \u{2193}",
            description: "Next item",
            contexts: &[KeyContext::Operations],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "k / \u{2191}",
            description: "Previous item",
            contexts: &[KeyContext::Operations],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Enter",
            description: "Expand / collapse",
            contexts: &[KeyContext::Operations],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Tab",
            description: "Switch to chat",
            contexts: &[KeyContext::Operations],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Esc",
            description: "Switch to chat",
            contexts: &[KeyContext::Operations],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "A",
            description: "Approve tool",
            contexts: &[KeyContext::ToolApproval],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "D",
            description: "Deny tool",
            contexts: &[KeyContext::ToolApproval],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Space",
            description: "Toggle step",
            contexts: &[KeyContext::PlanApproval],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "A",
            description: "Approve all",
            contexts: &[KeyContext::PlanApproval],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "C",
            description: "Cancel plan",
            contexts: &[KeyContext::PlanApproval],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Enter",
            description: "Edit / toggle",
            contexts: &[KeyContext::Settings],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "S",
            description: "Save changes",
            contexts: &[KeyContext::Settings],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "R",
            description: "Reset changes",
            contexts: &[KeyContext::Settings],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Esc",
            description: "Close / cancel edit",
            contexts: &[KeyContext::Settings],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "j / \u{2193}",
            description: "Next fact",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "k / \u{2191}",
            description: "Previous fact",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Enter",
            description: "View detail",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "s",
            description: "Cycle sort",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "f",
            description: "Filter",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "/",
            description: "Search",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "d",
            description: "Forget fact",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Tab",
            description: "Next tab",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Esc",
            description: "Close inspector",
            contexts: &[KeyContext::MemoryInspector],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "e",
            description: "Edit confidence",
            contexts: &[KeyContext::FactDetail],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "d",
            description: "Forget",
            contexts: &[KeyContext::FactDetail],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "r",
            description: "Restore",
            contexts: &[KeyContext::FactDetail],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Esc",
            description: "Back",
            contexts: &[KeyContext::FactDetail],
            show_in_status_bar: true,
        },
    ]
}

/// Bindable action for configurable keybindings.
///
/// Covers global shortcuts (Ctrl+key combos) that have the same meaning regardless of
/// context. Mode-specific bindings (overlays, filter, selection) stay hardcoded.
#[non_exhaustive]
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
        }
    }

    fn config_key(self) -> &'static str {
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
        }
    }

    fn all() -> &'static [Action] {
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

        // Build reverse lookup.
        let mut dispatch = HashMap::new();
        for (action, keys) in &action_to_keys {
            for key in keys {
                dispatch.insert(*key, *action);
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
            (
                Action::CopyLastResponse,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('y'))],
            ),
            (
                Action::ComposeInEditor,
                vec![(KeyModifiers::CONTROL, KeyCode::Char('e'))],
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
        ]
    }
}

/// Parse a human-readable key combo string (e.g. `"Ctrl+F"`, `"Shift+Up"`, `"F1"`)
/// into a `(KeyModifiers, KeyCode)` pair.
pub(crate) fn parse_key_combo(s: &str) -> Option<(KeyModifiers, KeyCode)> {
    let parts: Vec<&str> = s.split('+').collect();
    let mut modifiers = KeyModifiers::NONE;
    let mut code_part = "";

    for part in &parts {
        let p = part.trim();
        match p.to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            "alt" => modifiers |= KeyModifiers::ALT,
            _ => code_part = p,
        }
    }

    let code = match code_part.to_lowercase().as_str() {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "f1" => KeyCode::F(1),
        "f2" => KeyCode::F(2),
        "f3" => KeyCode::F(3),
        "f4" => KeyCode::F(4),
        "f5" => KeyCode::F(5),
        "f6" => KeyCode::F(6),
        "f7" => KeyCode::F(7),
        "f8" => KeyCode::F(8),
        "f9" => KeyCode::F(9),
        "f10" => KeyCode::F(10),
        "f11" => KeyCode::F(11),
        "f12" => KeyCode::F(12),
        s if s.len() == 1 => KeyCode::Char(s.chars().next()?),
        _ => return None,
    };

    Some((modifiers, code))
}

/// Determine active contexts. Help overlay is transparent: it shows the underlying context.
pub fn current_contexts(app: &App) -> Vec<KeyContext> {
    let mut contexts = vec![KeyContext::Global];

    // WHY: Memory inspector views return early to suppress all other context bindings.
    if matches!(
        app.layout.view_stack.current(),
        crate::state::view_stack::View::FactDetail { .. }
    ) {
        contexts.push(KeyContext::FactDetail);
        return contexts;
    }
    if matches!(
        app.layout.view_stack.current(),
        crate::state::view_stack::View::MemoryInspector
    ) {
        contexts.push(KeyContext::MemoryInspector);
        return contexts;
    }

    match &app.layout.overlay {
        Some(Overlay::Help) | None => {
            if app.interaction.command_palette.active {
                contexts.push(KeyContext::CommandPalette);
            } else if app.interaction.filter.active {
                contexts.push(KeyContext::Filter);
            } else if app.interaction.selection != SelectionContext::None {
                contexts.push(KeyContext::Selection);
            } else if app.layout.ops.visible
                && app.layout.ops.focused_pane == crate::state::FocusedPane::Operations
            {
                contexts.push(KeyContext::Operations);
            } else {
                contexts.push(KeyContext::Chat);
                contexts.push(KeyContext::Input);
            }
        }
        Some(Overlay::ToolApproval(_)) => {
            contexts.push(KeyContext::ToolApproval);
            contexts.push(KeyContext::Overlay);
        }
        Some(Overlay::PlanApproval(_)) => {
            contexts.push(KeyContext::PlanApproval);
            contexts.push(KeyContext::Overlay);
        }
        Some(Overlay::Settings(_)) => {
            contexts.push(KeyContext::Settings);
        }
        Some(_) => {
            contexts.push(KeyContext::Overlay);
        }
    }

    contexts
}

/// Label for the help overlay title: reflects the source context, not the overlay itself.
pub fn context_label(app: &App) -> &'static str {
    match &app.layout.overlay {
        Some(Overlay::Help) | None => {
            if app.interaction.command_palette.active {
                "Command Palette"
            } else if app.interaction.filter.active {
                "Filter"
            } else if app.interaction.selection != SelectionContext::None {
                "Selection"
            } else if app.layout.ops.visible
                && app.layout.ops.focused_pane == crate::state::FocusedPane::Operations
            {
                "Operations"
            } else {
                "Chat"
            }
        }
        Some(Overlay::AgentPicker { .. }) => "Agent Picker",
        Some(Overlay::SessionPicker(_)) => "Session List",
        Some(Overlay::ToolApproval(_)) => "Tool Approval",
        Some(Overlay::PlanApproval(_)) => "Plan Approval",
        Some(Overlay::SystemStatus) => "System Status",
        Some(Overlay::Settings(_)) => "Settings",
        Some(Overlay::ContextActions(_)) => "Context Actions",
        Some(Overlay::DiffView(_)) => "Diff Viewer",
        Some(Overlay::SessionSearch(_)) => "Session Search",
    }
}

/// Groups keybindings by their primary context, ordered for display.
pub fn grouped_keybindings(
    contexts: &[KeyContext],
) -> Vec<(&'static str, Vec<&'static Keybinding>)> {
    let mut context_order: Vec<KeyContext> = contexts.to_vec();
    context_order.sort_by_key(|c| c.display_order());

    let mut groups: Vec<(&'static str, Vec<&'static Keybinding>)> = Vec::new();
    let mut seen_keys: Vec<&'static str> = Vec::new();

    for ctx in &context_order {
        let label = ctx.section_label();
        if groups.iter().any(|(l, _)| *l == label) {
            continue;
        }

        let bindings: Vec<&'static Keybinding> = all_keybindings()
            .iter()
            .filter(|kb| kb.contexts.contains(ctx))
            .filter(|kb| {
                if seen_keys.contains(&kb.keys) {
                    false
                } else {
                    seen_keys.push(kb.keys);
                    true
                }
            })
            .collect();

        if !bindings.is_empty() {
            groups.push((label, bindings));
        }
    }

    groups
}

/// Status bar hints: mode-specific bindings first, truncated to fit.
pub fn status_bar_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let contexts = current_contexts(app);

    let mut bindings: Vec<&Keybinding> = all_keybindings()
        .iter()
        .filter(|kb| kb.show_in_status_bar)
        .filter(|kb| kb.contexts.iter().any(|c| contexts.contains(c)))
        .collect();

    bindings.sort_by_key(|kb| {
        kb.contexts
            .iter()
            .map(|c| c.display_order())
            .min()
            .unwrap_or(255)
    });

    bindings
        .iter()
        .map(|kb| (kb.keys, kb.description))
        .take(8)
        .collect()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn all_keybindings_is_not_empty() {
        let bindings = all_keybindings();
        assert!(bindings.len() > 30);
    }

    #[test]
    fn every_keybinding_has_context() {
        for kb in all_keybindings() {
            assert!(
                !kb.contexts.is_empty(),
                "keybinding '{}' has no contexts",
                kb.keys
            );
        }
    }

    #[test]
    fn section_label_covers_all_variants() {
        let contexts = [
            KeyContext::Global,
            KeyContext::Chat,
            KeyContext::Selection,
            KeyContext::Filter,
            KeyContext::CommandPalette,
            KeyContext::SessionList,
            KeyContext::Input,
            KeyContext::Overlay,
            KeyContext::ToolApproval,
            KeyContext::PlanApproval,
            KeyContext::Settings,
        ];
        for ctx in contexts {
            let label = ctx.section_label();
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn display_order_returns_valid_values() {
        let contexts = [
            KeyContext::Global,
            KeyContext::Chat,
            KeyContext::Input,
            KeyContext::Overlay,
            KeyContext::Selection,
        ];
        for ctx in contexts {
            let order = ctx.display_order();
            assert!(order <= 4);
        }
    }

    #[test]
    fn current_contexts_default_includes_global_chat_input() {
        let app = test_app();
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Global));
        assert!(contexts.contains(&KeyContext::Chat));
        assert!(contexts.contains(&KeyContext::Input));
    }

    #[test]
    fn current_contexts_command_palette() {
        let mut app = test_app();
        app.interaction.command_palette.active = true;
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::CommandPalette));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_filter_mode() {
        let mut app = test_app();
        app.interaction.filter.active = true;
        app.interaction.filter.editing = true;
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Filter));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_selection_mode() {
        let mut app = test_app();
        app.interaction.selection = SelectionContext::UserMessage { index: 0 };
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Selection));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_tool_approval_overlay() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::ToolApproval(crate::state::ToolApprovalOverlay {
            turn_id: "t1".into(),
            tool_id: "tool1".into(),
            tool_name: "test_tool".to_string(),
            input: serde_json::Value::Null,
            risk: "low".to_string(),
            reason: "test".to_string(),
        }));
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::ToolApproval));
        assert!(contexts.contains(&KeyContext::Overlay));
    }

    #[test]
    fn current_contexts_settings_overlay() {
        let mut app = test_app();
        let settings = crate::state::settings::SettingsOverlay::from_config(&serde_json::json!({}));
        app.layout.overlay = Some(Overlay::Settings(settings));
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Settings));
        assert!(!contexts.contains(&KeyContext::Overlay));
    }

    #[test]
    fn context_label_default() {
        let app = test_app();
        assert_eq!(context_label(&app), "Chat");
    }

    #[test]
    fn context_label_command_palette() {
        let mut app = test_app();
        app.interaction.command_palette.active = true;
        assert_eq!(context_label(&app), "Command Palette");
    }

    #[test]
    fn context_label_overlay_variants() {
        let mut app = test_app();
        app.layout.overlay = Some(Overlay::Help);
        // Help overlay is transparent: shows underlying context
        assert_eq!(context_label(&app), "Chat");

        app.layout.overlay = Some(Overlay::AgentPicker { cursor: 0 });
        assert_eq!(context_label(&app), "Agent Picker");

        app.layout.overlay = Some(Overlay::SystemStatus);
        assert_eq!(context_label(&app), "System Status");
    }

    #[test]
    fn grouped_keybindings_deduplicates_keys() {
        let contexts = vec![KeyContext::Global, KeyContext::Chat, KeyContext::Input];
        let groups = grouped_keybindings(&contexts);
        let mut all_keys: Vec<&str> = Vec::new();
        for (_, bindings) in &groups {
            for kb in bindings {
                assert!(
                    !all_keys.contains(&kb.keys),
                    "duplicate key '{}' in grouped_keybindings",
                    kb.keys
                );
                all_keys.push(kb.keys);
            }
        }
    }

    #[test]
    fn grouped_keybindings_empty_contexts_returns_empty() {
        let groups = grouped_keybindings(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn status_bar_hints_max_eight() {
        let app = test_app();
        let hints = status_bar_hints(&app);
        assert!(hints.len() <= 8);
    }

    #[test]
    fn status_bar_hints_includes_chat_bindings() {
        let app = test_app();
        let hints = status_bar_hints(&app);
        let keys: Vec<&str> = hints.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"?"), "should include ? for help");
        assert!(keys.contains(&":"), "should include : for command palette");
    }

    // --- KeyMap and parse_key_combo tests ---

    #[test]
    fn parse_key_combo_ctrl_letter() {
        let (mods, code) = parse_key_combo("Ctrl+F").expect("should parse");
        assert_eq!(mods, KeyModifiers::CONTROL);
        assert_eq!(code, KeyCode::Char('f'));
    }

    #[test]
    fn parse_key_combo_shift_arrow() {
        let (mods, code) = parse_key_combo("Shift+Up").expect("should parse");
        assert_eq!(mods, KeyModifiers::SHIFT);
        assert_eq!(code, KeyCode::Up);
    }

    #[test]
    fn parse_key_combo_function_key() {
        let (mods, code) = parse_key_combo("F1").expect("should parse");
        assert_eq!(mods, KeyModifiers::NONE);
        assert_eq!(code, KeyCode::F(1));
    }

    #[test]
    fn parse_key_combo_pageup() {
        let (mods, code) = parse_key_combo("PageUp").expect("should parse");
        assert_eq!(mods, KeyModifiers::NONE);
        assert_eq!(code, KeyCode::PageUp);
    }

    #[test]
    fn parse_key_combo_invalid_returns_none() {
        assert!(parse_key_combo("InvalidKey").is_none());
    }

    #[test]
    fn parse_key_combo_ctrl_alt_combo() {
        let (mods, code) = parse_key_combo("Ctrl+Alt+X").expect("should parse");
        assert!(mods.contains(KeyModifiers::CONTROL));
        assert!(mods.contains(KeyModifiers::ALT));
        assert_eq!(code, KeyCode::Char('x'));
    }

    #[test]
    fn keymap_defaults_include_quit() {
        let keymap = KeyMap::build(&HashMap::new());
        let action = keymap.lookup(KeyModifiers::CONTROL, KeyCode::Char('c'));
        assert_eq!(action, Some(Action::Quit));
    }

    #[test]
    fn keymap_defaults_include_toggle_sidebar() {
        let keymap = KeyMap::build(&HashMap::new());
        let action = keymap.lookup(KeyModifiers::CONTROL, KeyCode::Char('f'));
        assert_eq!(action, Some(Action::ToggleSidebar));
    }

    #[test]
    fn keymap_override_replaces_default() {
        let mut overrides = HashMap::new();
        overrides.insert("toggle_sidebar".to_string(), "Ctrl+G".to_string());
        let keymap = KeyMap::build(&overrides);
        // Old binding should be gone
        assert!(
            keymap
                .lookup(KeyModifiers::CONTROL, KeyCode::Char('f'))
                .is_none()
        );
        // New binding should work
        assert_eq!(
            keymap.lookup(KeyModifiers::CONTROL, KeyCode::Char('g')),
            Some(Action::ToggleSidebar)
        );
    }

    #[test]
    fn keymap_invalid_override_ignored() {
        let mut overrides = HashMap::new();
        overrides.insert("quit".to_string(), "InvalidKey".to_string());
        let keymap = KeyMap::build(&overrides);
        // Default Ctrl+C should still work since the override was invalid
        assert_eq!(
            keymap.lookup(KeyModifiers::CONTROL, KeyCode::Char('c')),
            Some(Action::Quit)
        );
    }

    #[test]
    fn keymap_lookup_unbound_returns_none() {
        let keymap = KeyMap::build(&HashMap::new());
        assert!(
            keymap
                .lookup(KeyModifiers::CONTROL, KeyCode::Char('z'))
                .is_none()
        );
    }

    #[test]
    fn action_to_msg_round_trips() {
        // Verify every action produces a Msg without panicking.
        for action in Action::all() {
            let _msg = action.to_msg();
        }
    }

    #[test]
    fn action_config_key_unique() {
        let keys: Vec<&str> = Action::all().iter().map(|a| a.config_key()).collect();
        let mut deduped = keys.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            keys.len(),
            deduped.len(),
            "config_key values must be unique"
        );
    }
}
