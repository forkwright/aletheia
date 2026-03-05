/// Context-aware keybinding registry — single source of truth for help overlay and status bar hints.
use crate::app::{App, Overlay, SelectionContext};

pub struct Keybinding {
    pub keys: &'static str,
    pub description: &'static str,
    pub contexts: &'static [KeyContext],
    pub show_in_status_bar: bool,
}

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
            | Self::Settings => 0,
            Self::Chat => 1,
            Self::Input => 2,
            Self::Overlay => 3,
            Self::Global => 4,
        }
    }
}

pub fn all_keybindings() -> &'static [Keybinding] {
    &[
        // --- Chat (empty-input triggers) ---
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
            description: "Filter",
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
            description: "Scroll up",
            contexts: &[KeyContext::Chat],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Shift+Down",
            description: "Scroll down",
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
        // --- Selection ---
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
        // --- Filter ---
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
        // --- Command Palette ---
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
        // --- Session List ---
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
        // --- Input ---
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
        // --- Overlay (generic) ---
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
        // --- Global ---
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
            keys: "Ctrl+F",
            description: "Toggle sidebar",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+T",
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
            keys: "Ctrl+C/Q",
            description: "Quit",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        // --- Tool Approval ---
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
        // --- Plan Approval ---
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
        // --- Settings ---
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
    ]
}

/// Determine active contexts. Help overlay is transparent — it shows the underlying context.
pub fn current_contexts(app: &App) -> Vec<KeyContext> {
    let mut contexts = vec![KeyContext::Global];

    match &app.overlay {
        Some(Overlay::Help) | None => {
            if app.command_palette.active {
                contexts.push(KeyContext::CommandPalette);
            } else if app.filter.active {
                contexts.push(KeyContext::Filter);
            } else if app.selection != SelectionContext::None {
                contexts.push(KeyContext::Selection);
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

/// Label for the help overlay title — reflects the source context, not the overlay itself.
pub fn context_label(app: &App) -> &'static str {
    match &app.overlay {
        Some(Overlay::Help) | None => {
            if app.command_palette.active {
                "Command Palette"
            } else if app.filter.active {
                "Filter"
            } else if app.selection != SelectionContext::None {
                "Selection"
            } else {
                "Chat"
            }
        }
        Some(Overlay::AgentPicker { .. }) => "Agent Picker",
        Some(Overlay::ToolApproval(_)) => "Tool Approval",
        Some(Overlay::PlanApproval(_)) => "Plan Approval",
        Some(Overlay::SystemStatus) => "System Status",
        Some(Overlay::Settings(_)) => "Settings",
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
