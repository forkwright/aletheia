#![deny(missing_docs)]
//! Dioxus desktop streaming architecture for Aletheia.
//!
//! Provides signal-based SSE and per-message stream consumption
//! designed for reactive UI frameworks like Dioxus. The dual-stream
//! architecture mirrors the TUI's proven pattern while adapting it
//! to Dioxus's signal-driven reactivity model.

/// Aletheia API client for the desktop app (sessions, messages, agents).
pub mod api;
/// Dioxus UI components for the desktop app.
pub mod components;
/// Platform integration: system tray, global hotkeys, native menus, window state, notifications.
pub(crate) mod platform;
/// Background services: SSE connection, stream management, and state sync.
pub mod services;
/// Application state managed via Dioxus signals.
pub mod state;

pub(crate) mod app;
pub(crate) mod layout;
pub(crate) mod theme;
pub(crate) mod views;

/// Launch the desktop application.
///
/// Loads persisted window state and configures the desktop window before
/// showing it. Platform features (tray, hotkeys, menus) are initialized
/// once the connection is established.
pub fn run() {
    use dioxus::desktop::Config;

    let window_state = platform::window_state::load_or_default();

    // WHY: Apply window geometry before launch so the window appears at the
    // saved position without visible repositioning.
    let window_builder = dioxus::desktop::WindowBuilder::new()
        .with_title("Aletheia")
        .with_inner_size(dioxus::desktop::LogicalSize::new(
            window_state.width as f64,
            window_state.height as f64,
        ))
        .with_position(dioxus::desktop::LogicalPosition::new(
            window_state.x as f64,
            window_state.y as f64,
        ))
        .with_maximized(window_state.maximized);

    let config = Config::new().with_window(window_builder);

    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(app::App);
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_modules_accessible() {
        // NOTE: Validates that the public module tree compiles and links.
        let _ = super::state::events::EventState::default();
    }
}
