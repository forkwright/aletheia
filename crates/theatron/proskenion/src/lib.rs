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
/// Log-to-file initialisation (daily-rolling, non-blocking).
pub(crate) mod logging;
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
/// Initialises log-to-file, loads persisted window state, and configures the
/// desktop window before showing it. Closing the window exits the process
/// cleanly — no minimize-to-tray, no hidden background process.
///
/// Pass `verbose = true` (e.g. from a `--verbose` CLI flag) to also emit logs
/// to stderr. When `RUST_LOG` is set in the environment stderr output is added
/// automatically regardless.
pub fn run(verbose: bool) {
    // WHY: Keep the guard alive for the process lifetime so the non-blocking
    // writer thread flushes pending log records before the file is closed.
    let _log_guard = logging::init(verbose);

    tracing::info!("starting proskenion");

    // WHY: reqwest with rustls-no-provider requires an explicit crypto provider
    // install before any Client is constructed, otherwise it panics with
    // "No provider set" (#2363).
    let _ = rustls::crypto::ring::default_provider().install_default();

    use dioxus::desktop::{Config, WindowCloseBehaviour};

    let window_state = platform::window_state::load_or_default();

    // WHY: Apply window geometry before launch so the window appears at the
    // saved position without visible repositioning.
    let window_builder = dioxus::desktop::WindowBuilder::new()
        .with_title("Aletheia")
        .with_inner_size(dioxus::desktop::LogicalSize::new(
            f64::from(window_state.width),
            f64::from(window_state.height),
        ))
        .with_position(dioxus::desktop::LogicalPosition::new(
            f64::from(window_state.x),
            f64::from(window_state.y),
        ))
        .with_maximized(window_state.maximized);

    // WHY: Passing `None` removes the default OS menu bar (Window/Edit/Help)
    // that Dioxus injects via `MenuBuilderState::Unset`. The app's intentional
    // menu structure lives in `platform::menus` and will be wired in separately.
    //
    // WHY: `WindowCloseBehaviour::WindowCloses` ensures that clicking the close
    // button exits the process cleanly. The app must not linger as a background
    // process with no window — no tray icon is shown, so there would be no way
    // to recover it. SSE disconnect and window-state persistence happen during
    // the normal Dioxus shutdown sequence before the process exits.
    //
    // WHY: Dioxus desktop does not auto-inject CSS from the asset directory.
    // We must add <link> tags via custom_head so the webview loads our design
    // token system (tokens.css), theme definitions (themes.css), and base
    // resets/animations (base.css).
    let custom_head = r#"
        <link rel="stylesheet" href="styles/tokens.css">
        <link rel="stylesheet" href="styles/themes.css">
        <link rel="stylesheet" href="styles/base.css">
    "#;

    let config = Config::new()
        .with_window(window_builder)
        .with_menu(None::<dioxus::desktop::muda::Menu>)
        .with_close_behaviour(WindowCloseBehaviour::WindowCloses)
        .with_custom_head(custom_head.to_string());

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
