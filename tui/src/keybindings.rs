/// Context-aware keybinding registry — single source of truth for help overlay and status bar hints.
use crate::app::{App, Overlay};

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
    Input,
    Overlay,
    ToolApproval,
    PlanApproval,
}

impl KeyContext {
    pub fn section_label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Chat => "Chat",
            Self::Input => "Input",
            Self::Overlay => "Overlay",
            Self::ToolApproval => "Tool Approval",
            Self::PlanApproval => "Plan Approval",
        }
    }

    fn display_order(self) -> u8 {
        match self {
            Self::ToolApproval | Self::PlanApproval => 0,
            Self::Chat => 1,
            Self::Input => 2,
            Self::Overlay => 3,
            Self::Global => 4,
        }
    }
}

pub fn all_keybindings() -> &'static [Keybinding] {
    &[
        // --- Global ---
        Keybinding {
            keys: "?",
            description: "Help for current view",
            contexts: &[KeyContext::Global],
            show_in_status_bar: false,
        },
        Keybinding {
            keys: "Ctrl+A",
            description: "Switch agent",
            contexts: &[KeyContext::Global],
            show_in_status_bar: true,
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
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Ctrl+N",
            description: "New session",
            contexts: &[KeyContext::Global],
            show_in_status_bar: true,
        },
        Keybinding {
            keys: "Ctrl+C/Q",
            description: "Quit",
            contexts: &[KeyContext::Global],
            show_in_status_bar: true,
        },
        // --- Chat ---
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
    ]
}

pub fn current_contexts(app: &App) -> Vec<KeyContext> {
    let mut contexts = vec![KeyContext::Global];

    match &app.overlay {
        Some(Overlay::ToolApproval(_)) => {
            contexts.push(KeyContext::ToolApproval);
            contexts.push(KeyContext::Overlay);
        }
        Some(Overlay::PlanApproval(_)) => {
            contexts.push(KeyContext::PlanApproval);
            contexts.push(KeyContext::Overlay);
        }
        Some(_) => {
            contexts.push(KeyContext::Overlay);
        }
        None => {
            contexts.push(KeyContext::Chat);
            contexts.push(KeyContext::Input);
        }
    }

    contexts
}

pub fn context_label(overlay: &Option<Overlay>) -> &'static str {
    match overlay {
        None => "Chat",
        Some(Overlay::Help) => "Help",
        Some(Overlay::AgentPicker { .. }) => "Agent Picker",
        Some(Overlay::ToolApproval(_)) => "Tool Approval",
        Some(Overlay::PlanApproval(_)) => "Plan Approval",
        Some(Overlay::SystemStatus) => "System Status",
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

pub fn status_bar_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let contexts = current_contexts(app);

    all_keybindings()
        .iter()
        .filter(|kb| kb.show_in_status_bar)
        .filter(|kb| kb.contexts.iter().any(|c| contexts.contains(c)))
        .map(|kb| (kb.keys, kb.description))
        .collect()
}
