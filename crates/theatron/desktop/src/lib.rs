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
/// Background services: SSE connection, stream management, and state sync.
pub mod services;
/// Application state managed via Dioxus signals.
pub mod state;

pub(crate) mod app;
pub(crate) mod layout;
pub(crate) mod theme;
pub(crate) mod views;

/// Launch the desktop application.
pub fn run() {
    dioxus::launch(app::App);
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_modules_accessible() {
        // NOTE: Validates that the public module tree compiles and links.
        let _ = super::state::events::EventState::default();
    }
}
