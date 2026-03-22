//! Keybinding registry: types and static table of all keybindings.
//!
//! Context-aware keybinding registry: single source of truth for help overlay and status bar hints.

pub(crate) struct Keybinding {
    pub(crate) keys: &'static str,
    pub(crate) description: &'static str,
    pub(crate) contexts: &'static [KeyContext],
    pub(crate) show_in_status_bar: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyContext {
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
    pub(crate) fn section_label(self) -> &'static str {
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

    pub(super) fn display_order(self) -> u8 {
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

pub(crate) fn all_keybindings() -> &'static [Keybinding] {
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
            description: "Search history",
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
