//! Root component that gates on connection state.
//!
//! When disconnected, shows the connect view. When connected, shows the
//! main layout with the router. Platform integration (tray state, hotkeys,
//! window persistence, quick input) is wired here.

use dioxus::prelude::*;

use crate::layout::Layout;
use crate::platform;
use crate::services::toast::provide_toast_context;
use crate::services::{config, settings_config};
use crate::state::agents::AgentStore;
use crate::state::connection::ConnectionState;
use crate::state::notifications::{DndState, NotificationHistory};
use crate::state::platform::{CloseBehavior, HotkeyState, QuickInputState, TrayState, WindowState};
use crate::state::tool_metrics::DateRange;
use crate::theme::ThemeProvider;
use crate::views::chat::Chat;
use crate::views::connect::ConnectView;
use crate::views::files::Files;
use crate::views::memory::Memory;
use crate::views::metrics::Metrics;
use crate::views::metrics::tool_detail::ToolDetailView;
use crate::views::ops::Ops;
use crate::views::planning::{Planning, PlanningProject};
use crate::views::sessions::Sessions;
use crate::views::settings::Settings;
use crate::views::settings::wizard::SetupWizard;

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub(crate) enum Route {
    #[layout(Layout)]
        #[route("/")]
        Chat {},
        #[route("/files")]
        Files {},
        #[route("/planning")]
        Planning {},
        #[route("/planning/:project_id")]
        PlanningProject { project_id: String },
        #[route("/memory")]
        Memory {},
        #[route("/metrics")]
        Metrics {},
        #[route("/metrics/tools/:tool_name")]
        MetricsToolDetail { tool_name: String },
        #[route("/ops")]
        Ops {},
        #[route("/sessions")]
        Sessions {},
        #[route("/settings")]
        Settings {},
}

/// Route component for `/metrics/tools/:tool_name`.
///
/// Wraps `ToolDetailView` so the tool drill-down is accessible via URL.
#[component]
fn MetricsToolDetail(tool_name: String) -> Element {
    let nav = use_navigator();
    rsx! {
        div {
            style: "\
                display: flex; flex-direction: column; \
                height: 100%; padding: 24px; gap: 16px; \
                overflow-y: auto;",
            ToolDetailView {
                tool_name,
                date_range: DateRange::default(),
                on_back: move |_| { nav.push(Route::Metrics {}); },
            }
        }
    }
}

/// Root component.
///
/// Provides connection state, config, settings, platform state signals, and
/// toast store as context. Gates on wizard -> connect -> connected.
#[component]
pub(crate) fn App() -> Element {
    let loaded_settings = use_hook(settings_config::load_or_default);
    let loaded_config = use_hook(config::load_or_default);
    let initial_theme = loaded_settings.appearance_settings().theme_mode();
    let first_run = use_hook(settings_config::is_first_run);
    let loaded_window_state = use_hook(platform::window_state::load_or_default);

    let connection_state = use_signal(ConnectionState::default);
    let connection_config = use_signal(|| loaded_config);
    let server_store = use_signal(|| loaded_settings.server_store());
    let appearance = use_signal(|| loaded_settings.appearance_settings());
    let keybindings = use_signal(|| loaded_settings.keybinding_store());
    let is_first_run = use_signal(|| first_run);

    // NOTE: Provide signals as context so all views can access them.
    use_context_provider(|| connection_state);
    use_context_provider(|| connection_config);
    use_context_provider(|| server_store);
    use_context_provider(|| appearance);
    use_context_provider(|| keybindings);
    use_context_provider(|| is_first_run);
    provide_toast_context();

    // NOTE: Platform state signals available to all components.
    use_context_provider(|| Signal::new(TrayState::default()));
    use_context_provider(|| Signal::new(HotkeyState::default()));
    use_context_provider(|| Signal::new(loaded_window_state));
    use_context_provider(|| Signal::new(QuickInputState::default()));
    use_context_provider(|| Signal::new(CloseBehavior::default()));

    let needs_wizard = *is_first_run.read();
    let needs_connect = connection_state.read().needs_connect_view();

    rsx! {
        ThemeProvider {
            initial_mode: Some(initial_theme),
            if needs_wizard {
                SetupWizard {}
            } else if needs_connect {
                ConnectView {
                    connection_state,
                    connection_config,
                }
            } else {
                ConnectedApp {}
            }
        }
    }
}

/// Inner component rendered when connected.
///
/// Starts the SSE coroutine, spawns platform integration coroutines
/// (tray state sync, window state persistence), and renders the router
/// with toast overlay and quick input overlay.
#[component]
fn ConnectedApp() -> Element {
    let config = use_context::<Signal<crate::state::connection::ConnectionConfig>>();

    // Provide notification signals. Preferences are loaded from disk; DND and
    // history are ephemeral and reset on each app launch.
    use_context_provider(|| Signal::new(config::load_notification_prefs()));
    use_context_provider(|| Signal::new(NotificationHistory::default()));
    use_context_provider(|| Signal::new(DndState::default()));

    // WHY: Start SSE coroutine here (not in App) so it only runs when connected
    // and has access to the finalized connection config.
    crate::services::sse_coroutine::start_sse_coroutine(&config.read());

    // NOTE: Start platform integration coroutines.
    start_tray_sync();
    start_window_state_writer();

    rsx! {
        crate::components::toast_container::ToastContainer {}
        crate::components::quick_input::QuickInputOverlay {}
        Router::<Route> {}
    }
}

/// Coroutine that syncs tray state from agent store and event state.
///
/// Runs a reactive effect that recomputes tray state whenever agent
/// statuses or connection state change.
fn start_tray_sync() {
    let agents: Signal<AgentStore> = use_context();
    let mut tray: Signal<TrayState> = use_context();

    // WHY: Use use_effect so this re-runs whenever the agent store signal changes.
    use_effect(move || {
        let agent_store = agents.read();
        // NOTE: Derive connection state from SSE connection presence.
        let connected = !agent_store.is_empty();
        let new_tray = platform::tray::derive_tray_state(&agent_store, connected, true);
        tray.set(new_tray);
    });
}

/// Coroutine that persists window state changes with debouncing.
///
/// Spawns a [`DebouncedWriter`] and updates it when the window state
/// signal changes.
fn start_window_state_writer() {
    let window_state: Signal<WindowState> = use_context();

    // WHY: Initialize the debounced writer once and keep it alive for
    // the lifetime of the connected app. The writer's background task
    // flushes to disk every 2 seconds when dirty.
    let writer =
        use_hook(|| platform::window_state::DebouncedWriter::new(window_state.read().clone()));

    use_effect(move || {
        let state = window_state.read().clone();
        writer.update(|w| *w = state);
    });
}
