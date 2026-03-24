//! Platform integration: system tray, global hotkeys, native menus, window state, notifications.
//!
//! Each submodule provides framework-agnostic logic that the Dioxus integration
//! layer in `app.rs` wires into the reactive component tree.
//! [`native_notify::send_native`] dispatches freedesktop.org D-Bus
//! notifications on Linux, falling back gracefully if the daemon is absent.

/// Global hotkey registration and summon toggle logic.
pub(crate) mod hotkeys;
/// Native application menu bar structure and action mapping.
pub(crate) mod menus;
/// D-Bus notification dispatch with graceful fallback.
pub(crate) mod native_notify;
/// Notification payload and urgency types.
pub(crate) mod notifications;
/// System tray icon state derivation and context menu generation.
pub(crate) mod tray;
/// Window geometry and UI state persistence with debounced writes.
pub(crate) mod window_state;
