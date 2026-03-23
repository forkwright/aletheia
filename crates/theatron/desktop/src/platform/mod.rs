//! Platform integration: system tray, global hotkeys, native menus, window state.
//!
//! Each submodule provides framework-agnostic logic that the Dioxus integration
//! layer in `app.rs` wires into the reactive component tree.

/// Global hotkey registration and summon toggle logic.
pub(crate) mod hotkeys;
/// Native application menu bar structure and action mapping.
pub(crate) mod menus;
/// System tray icon state derivation and context menu generation.
pub(crate) mod tray;
/// Window geometry and UI state persistence with debounced writes.
pub(crate) mod window_state;
