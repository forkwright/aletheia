//! Keybinding helper functions: context resolution and display helpers.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, Overlay, SelectionContext};

use super::registry::{KeyContext, Keybinding, all_keybindings};

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
pub(crate) fn current_contexts(app: &App) -> Vec<KeyContext> {
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
pub(crate) fn context_label(app: &App) -> &'static str {
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
        Some(Overlay::ContextBudget) => "Context Budget",
        Some(Overlay::Settings(_)) => "Settings",
        Some(Overlay::ContextActions(_)) => "Context Actions",
        Some(Overlay::DiffView(_)) => "Diff Viewer",
        Some(Overlay::SessionSearch(_)) => "Session Search",
        Some(Overlay::DecisionCard(_)) => "Decision",
    }
}

/// Groups keybindings by their primary context, ordered for display.
pub(crate) fn grouped_keybindings(
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
pub(crate) fn status_bar_hints(app: &App) -> Vec<(&'static str, &'static str)> {
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
