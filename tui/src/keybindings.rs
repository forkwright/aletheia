/// Context-aware keybinding registry — single source of truth for help overlay and status bar hints.
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
            | Self::Operations => 0,
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
        Keybinding {
            keys: "v",
            description: "Enter selection",
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
        // --- Operations pane ---
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
            } else if app.ops.visible
                && app.ops.focused_pane == crate::state::FocusedPane::Operations
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
            } else if app.ops.visible
                && app.ops.focused_pane == crate::state::FocusedPane::Operations
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
        app.command_palette.active = true;
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::CommandPalette));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_filter_mode() {
        let mut app = test_app();
        app.filter.active = true;
        app.filter.editing = true;
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Filter));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_selection_mode() {
        let mut app = test_app();
        app.selection = SelectionContext::UserMessage { index: 0 };
        let contexts = current_contexts(&app);
        assert!(contexts.contains(&KeyContext::Selection));
        assert!(!contexts.contains(&KeyContext::Chat));
    }

    #[test]
    fn current_contexts_tool_approval_overlay() {
        let mut app = test_app();
        app.overlay = Some(Overlay::ToolApproval(crate::state::ToolApprovalOverlay {
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
        app.overlay = Some(Overlay::Settings(settings));
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
        app.command_palette.active = true;
        assert_eq!(context_label(&app), "Command Palette");
    }

    #[test]
    fn context_label_overlay_variants() {
        let mut app = test_app();
        app.overlay = Some(Overlay::Help);
        // Help overlay is transparent — shows underlying context
        assert_eq!(context_label(&app), "Chat");

        app.overlay = Some(Overlay::AgentPicker { cursor: 0 });
        assert_eq!(context_label(&app), "Agent Picker");

        app.overlay = Some(Overlay::SystemStatus);
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
}
