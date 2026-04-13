//! Global keyboard navigation handler.
//!
//! Wired into the layout root div (`onkeydown`). Dispatches view-switching
//! shortcuts (Ctrl+1--7), command-palette toggle (Ctrl+K), and exposes the
//! key-dispatch enum consumed by views that need further key handling.
//!
//! # Shortcuts
//!
//! | Key           | Action                        |
//! |---------------|-------------------------------|
//! | Ctrl+1        | Navigate → Chat               |
//! | Ctrl+2        | Navigate → Files              |
//! | Ctrl+3        | Navigate → Planning           |
//! | Ctrl+4        | Navigate → Memory             |
//! | Ctrl+5        | Navigate → Metrics            |
//! | Ctrl+6        | Navigate → Ops                |
//! | Ctrl+7        | Navigate → Sessions           |
//! | Ctrl+K        | Open command palette          |
//! | Ctrl+B        | Toggle sidebar                |
//! | Ctrl+Shift+C  | Navigate → Chat (alt)         |
//! | Ctrl+Shift+F  | Navigate → Files (alt)        |
//! | Ctrl+F or /   | Focus in-view search          |
//! | F1            | Toggle help overlay           |
//! | Escape        | Dismiss modal / deselect      |
//! | Arrow keys    | List navigation               |
//! | Enter         | Confirm focused item          |

use dioxus::prelude::*;

use crate::app::Route;

/// A keyboard action decoded from a raw keydown event.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub(crate) enum KeyAction {
    /// Switch to a numbered view (1-indexed).
    NavigateTo(Route),
    /// Open the command palette.
    OpenPalette,
    /// Toggle sidebar collapsed/expanded.
    ToggleSidebar,
    /// Open / focus an in-view search bar.
    FocusSearch,
    /// Dismiss modal, close palette, deselect list item.
    Dismiss,
    /// Move selection up in a list.
    ListUp,
    /// Move selection down in a list.
    ListDown,
    /// Confirm the focused list item.
    ListConfirm,
    /// Toggle the help overlay (F1).
    ToggleHelp,
    /// A key with no mapped action.
    Unhandled,
}

/// Decode raw key name and modifier flags into a [`KeyAction`].
///
/// Separated from the Dioxus event type so it can be unit-tested without
/// constructing a `KeyboardData`.
pub(crate) fn decode_key_raw(key: &str, ctrl: bool, shift: bool) -> KeyAction {
    if ctrl && shift {
        // Ctrl+Shift letter shortcuts — alternative navigation bindings.
        return match key {
            "C" | "c" => KeyAction::NavigateTo(Route::Chat {}),
            "F" | "f" => KeyAction::NavigateTo(Route::Files {}),
            _ => KeyAction::Unhandled,
        };
    }

    if ctrl {
        return match key {
            "1" => KeyAction::NavigateTo(Route::Chat {}),
            "2" => KeyAction::NavigateTo(Route::Files {}),
            "3" => KeyAction::NavigateTo(Route::Planning {}),
            "4" => KeyAction::NavigateTo(Route::Memory {}),
            "5" => KeyAction::NavigateTo(Route::Metrics {}),
            "6" => KeyAction::NavigateTo(Route::Ops {}),
            "7" => KeyAction::NavigateTo(Route::Sessions {}),
            "k" | "K" => KeyAction::OpenPalette,
            "b" | "B" => KeyAction::ToggleSidebar,
            "f" | "F" => KeyAction::FocusSearch,
            _ => KeyAction::Unhandled,
        };
    }

    match key {
        "F1" => KeyAction::ToggleHelp,
        "Escape" => KeyAction::Dismiss,
        "ArrowUp" => KeyAction::ListUp,
        "ArrowDown" => KeyAction::ListDown,
        "Enter" => KeyAction::ListConfirm,
        "/" => KeyAction::FocusSearch,
        _ => KeyAction::Unhandled,
    }
}

/// Decode a Dioxus keyboard event into a [`KeyAction`].
pub(crate) fn decode_key(event: &KeyboardData) -> KeyAction {
    let mods = event.modifiers();
    let key = event.key().to_string();
    decode_key_raw(&key, mods.ctrl(), mods.shift())
}

/// Install a `onkeydown` handler on the layout root that handles global
/// view-switching and palette shortcuts.
///
/// Call this inside the `Layout` component. Returns an event handler closure
/// to attach to the root `div`.
pub(crate) fn use_global_keyboard(
    mut palette_open: Signal<bool>,
    mut sidebar_collapsed: Signal<bool>,
    mut help_visible: Signal<bool>,
) -> impl FnMut(Event<KeyboardData>) {
    move |evt: Event<KeyboardData>| {
        match decode_key(&evt.data()) {
            KeyAction::NavigateTo(route) => {
                let nav = navigator();
                nav.push(route);
            }
            KeyAction::OpenPalette => {
                let current = *palette_open.read();
                palette_open.set(!current);
            }
            KeyAction::ToggleSidebar => {
                let current = *sidebar_collapsed.read();
                sidebar_collapsed.set(!current);
            }
            KeyAction::ToggleHelp => {
                let current = *help_visible.read();
                help_visible.set(!current);
            }
            KeyAction::FocusSearch => {
                // NOTE: Each view handles search focus internally.
                // We dispatch Ctrl+F to let views react via their own key handlers.
            }
            KeyAction::Dismiss => {
                if *help_visible.read() {
                    help_visible.set(false);
                } else if *palette_open.read() {
                    palette_open.set(false);
                }
            }
            _ => {} // NOTE: unbound key combinations are intentionally ignored
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_1_navigates_to_chat() {
        assert_eq!(
            decode_key_raw("1", true, false),
            KeyAction::NavigateTo(Route::Chat {})
        );
    }

    #[test]
    fn ctrl_2_navigates_to_files() {
        assert_eq!(
            decode_key_raw("2", true, false),
            KeyAction::NavigateTo(Route::Files {})
        );
    }

    #[test]
    fn ctrl_7_navigates_to_sessions() {
        assert_eq!(
            decode_key_raw("7", true, false),
            KeyAction::NavigateTo(Route::Sessions {})
        );
    }

    #[test]
    fn ctrl_k_opens_palette() {
        assert_eq!(decode_key_raw("k", true, false), KeyAction::OpenPalette);
        assert_eq!(decode_key_raw("K", true, false), KeyAction::OpenPalette);
    }

    #[test]
    fn ctrl_b_toggles_sidebar() {
        assert_eq!(decode_key_raw("b", true, false), KeyAction::ToggleSidebar);
        assert_eq!(decode_key_raw("B", true, false), KeyAction::ToggleSidebar);
    }

    #[test]
    fn ctrl_f_focuses_search() {
        assert_eq!(decode_key_raw("f", true, false), KeyAction::FocusSearch);
        assert_eq!(decode_key_raw("F", true, false), KeyAction::FocusSearch);
    }

    #[test]
    fn escape_dismisses() {
        assert_eq!(decode_key_raw("Escape", false, false), KeyAction::Dismiss);
    }

    #[test]
    fn arrow_keys_navigate_list() {
        assert_eq!(decode_key_raw("ArrowUp", false, false), KeyAction::ListUp);
        assert_eq!(
            decode_key_raw("ArrowDown", false, false),
            KeyAction::ListDown
        );
    }

    #[test]
    fn enter_confirms_list_item() {
        assert_eq!(
            decode_key_raw("Enter", false, false),
            KeyAction::ListConfirm
        );
    }

    #[test]
    fn slash_focuses_search() {
        assert_eq!(decode_key_raw("/", false, false), KeyAction::FocusSearch);
    }

    #[test]
    fn unhandled_key_returns_unhandled() {
        assert_eq!(decode_key_raw("z", false, false), KeyAction::Unhandled);
        assert_eq!(decode_key_raw("Tab", false, false), KeyAction::Unhandled);
    }

    #[test]
    fn f1_toggles_help() {
        assert_eq!(decode_key_raw("F1", false, false), KeyAction::ToggleHelp);
    }

    #[test]
    fn ctrl_shift_c_navigates_to_chat() {
        assert_eq!(
            decode_key_raw("C", true, true),
            KeyAction::NavigateTo(Route::Chat {})
        );
        assert_eq!(
            decode_key_raw("c", true, true),
            KeyAction::NavigateTo(Route::Chat {})
        );
    }

    #[test]
    fn ctrl_shift_f_navigates_to_files() {
        assert_eq!(
            decode_key_raw("F", true, true),
            KeyAction::NavigateTo(Route::Files {})
        );
        assert_eq!(
            decode_key_raw("f", true, true),
            KeyAction::NavigateTo(Route::Files {})
        );
    }
}
