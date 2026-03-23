//! Global hotkey registration and action dispatch.
//!
//! Maps platform hotkey events to application actions. The actual hotkey
//! registration uses the Dioxus desktop global shortcut API; this module
//! provides the action mapping, summon toggle logic, and registration
//! result tracking.

use crate::state::platform::{HotkeyAction, HotkeyRegistration, HotkeyState};

/// Window visibility states for summon toggle logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowVisibility {
    /// Window is hidden (minimized to tray or not visible).
    Hidden,
    /// Window is visible but does not have focus.
    VisibleUnfocused,
    /// Window is visible and has keyboard focus.
    VisibleFocused,
}

/// Result of the summon toggle action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SummonResult {
    /// Show the window and bring to focus.
    Show,
    /// Focus the already-visible window.
    Focus,
    /// Hide the focused window to tray.
    Hide,
}

/// Determine the summon action based on current window state.
///
/// Toggle behavior:
/// - Hidden -> Show and focus
/// - Visible but unfocused -> Focus
/// - Visible and focused -> Hide to tray
#[must_use]
pub(crate) fn summon_toggle(visibility: WindowVisibility) -> SummonResult {
    match visibility {
        WindowVisibility::Hidden => SummonResult::Show,
        WindowVisibility::VisibleUnfocused => SummonResult::Focus,
        WindowVisibility::VisibleFocused => SummonResult::Hide,
    }
}

/// Build the initial hotkey state with all actions set to a given status.
///
/// Used when global hotkeys are unavailable on the platform (e.g. Wayland
/// without the global shortcuts portal).
#[must_use]
pub(crate) fn all_unavailable() -> HotkeyState {
    HotkeyState {
        registrations: HotkeyAction::all()
            .iter()
            .map(|&action| (action, HotkeyRegistration::Unavailable))
            .collect(),
    }
}

/// Build the hotkey state from a list of (action, success) registration results.
#[must_use]
pub(crate) fn from_results(results: Vec<(HotkeyAction, Result<(), String>)>) -> HotkeyState {
    HotkeyState {
        registrations: results
            .into_iter()
            .map(|(action, result)| {
                let status = match result {
                    Ok(()) => HotkeyRegistration::Registered,
                    Err(reason) => HotkeyRegistration::Failed { reason },
                };
                (action, status)
            })
            .collect(),
    }
}

/// Parse a hotkey binding string into modifier and key components.
///
/// Accepts format like "Ctrl+Shift+A", "Ctrl+Shift+Space", "Ctrl+Shift+Escape".
/// Returns `(modifiers, key)` where modifiers is a sorted list.
#[must_use]
pub(crate) fn parse_binding(binding: &str) -> (Vec<&str>, &str) {
    let parts: Vec<&str> = binding.split('+').collect();
    if parts.is_empty() {
        return (Vec::new(), "");
    }
    let (modifiers, key) = parts.split_at(parts.len() - 1);
    (modifiers.to_vec(), key.first().copied().unwrap_or(""))
}

/// Validate that a binding string has at least one modifier and a key.
#[must_use]
pub(crate) fn is_valid_binding(binding: &str) -> bool {
    let (modifiers, key) = parse_binding(binding);
    !modifiers.is_empty() && !key.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summon_hidden_shows() {
        assert_eq!(summon_toggle(WindowVisibility::Hidden), SummonResult::Show);
    }

    #[test]
    fn summon_unfocused_focuses() {
        assert_eq!(
            summon_toggle(WindowVisibility::VisibleUnfocused),
            SummonResult::Focus
        );
    }

    #[test]
    fn summon_focused_hides() {
        assert_eq!(
            summon_toggle(WindowVisibility::VisibleFocused),
            SummonResult::Hide
        );
    }

    #[test]
    fn all_unavailable_state() {
        let state = all_unavailable();
        assert!(state.is_unavailable());
        assert_eq!(state.registrations.len(), HotkeyAction::all().len());
    }

    #[test]
    fn from_results_mixed() {
        let results = vec![
            (HotkeyAction::SummonWindow, Ok(())),
            (
                HotkeyAction::QuickInput,
                Err("already registered".to_string()),
            ),
            (HotkeyAction::AbortStreaming, Ok(())),
        ];
        let state = from_results(results);
        assert_eq!(state.registered_count(), 2);
        assert!(state.has_failures());
    }

    #[test]
    fn from_results_all_success() {
        let results = vec![
            (HotkeyAction::SummonWindow, Ok(())),
            (HotkeyAction::QuickInput, Ok(())),
            (HotkeyAction::AbortStreaming, Ok(())),
        ];
        let state = from_results(results);
        assert_eq!(state.registered_count(), 3);
        assert!(!state.has_failures());
    }

    #[test]
    fn parse_binding_ctrl_shift_a() {
        let (mods, key) = parse_binding("Ctrl+Shift+A");
        assert_eq!(mods, vec!["Ctrl", "Shift"]);
        assert_eq!(key, "A");
    }

    #[test]
    fn parse_binding_ctrl_shift_space() {
        let (mods, key) = parse_binding("Ctrl+Shift+Space");
        assert_eq!(mods, vec!["Ctrl", "Shift"]);
        assert_eq!(key, "Space");
    }

    #[test]
    fn parse_binding_single_key() {
        let (mods, key) = parse_binding("F1");
        assert!(mods.is_empty());
        assert_eq!(key, "F1");
    }

    #[test]
    fn is_valid_binding_checks() {
        assert!(is_valid_binding("Ctrl+Shift+A"));
        assert!(is_valid_binding("Ctrl+Space"));
        assert!(!is_valid_binding("A")); // no modifier
        assert!(!is_valid_binding("")); // empty
    }
}
