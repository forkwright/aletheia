//! Root component that gates on connection state.
//!
//! When disconnected, shows the connect view. When connected, shows the
//! main layout with the router. Platform integration (tray state, hotkeys,
//! window persistence, quick input) is wired here.

use dioxus::prelude::*;
use themelion::theme::ThemeProvider;

use crate::layout::Layout;
use crate::platform;
use crate::services::toast::provide_toast_context;
use crate::services::{config, settings_config};
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::chat::ChatSelection;
use crate::state::commands::CommandStore;
use crate::state::connection::ConnectionState;
use crate::state::notifications::{DndState, NotificationHistory};
use crate::state::planning::PlanningCapabilities;
use crate::state::platform::{CloseBehavior, HotkeyState, QuickInputState, TrayState, WindowState};
use crate::state::tool_metrics::DateRange;
use crate::views::chat::Chat;
#[cfg(debug_assertions)]
use crate::views::component_library::ComponentLibrary;
use crate::views::connect::ConnectView;
use crate::views::files::Files;
use crate::views::memory::Memory;
use crate::views::meta::Meta;
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
        #[route("/meta")]
        Meta {},
        #[route("/settings")]
        Settings {},
        #[cfg(debug_assertions)]
        #[route("/component-library")]
        ComponentLibrary {},
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
                height: 100%; padding: var(--space-6); gap: var(--space-4); \
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
    // WHY: Detect first run before any load_or_default call that might touch
    // disk. A clean XDG_CONFIG_HOME must surface the setup wizard.
    let first_run = use_hook(settings_config::is_first_run);
    // WHY: Loading connection config may migrate a legacy desktop.toml active
    // server into settings.toml; do that before deriving the settings store.
    let loaded_config = use_hook(config::load_or_default);
    let loaded_settings = use_hook(settings_config::load_or_default);
    let loaded_window_state = use_hook(platform::window_state::load_or_default);

    let connection_state = use_signal(ConnectionState::default);
    let connection_config = use_signal(|| loaded_config);
    let server_store = use_signal(|| loaded_settings.server_store());
    let appearance = use_signal(|| loaded_settings.appearance_settings());
    let keybindings = use_signal(|| loaded_settings.keybinding_store());
    let is_first_run = use_signal(|| first_run);

    // NOTE: Saved theme preference; defaults to Dark if unset or unrecognized.
    let initial_theme = themelion::theme::ThemeMode::from_slug(appearance.read().theme.as_str())
        .unwrap_or(themelion::theme::ThemeMode::Dark);

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
            initial_mode: Some(initial_theme), // Force dark mode
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

    // NOTE: Notification preferences load from disk; DND and history are
    // ephemeral and reset on each app launch.
    use_context_provider(|| Signal::new(config::load_notification_prefs()));
    use_context_provider(|| Signal::new(NotificationHistory::default()));
    use_context_provider(|| Signal::new(DndState::default()));

    // WHY: AgentStore must be provided before start_tray_sync (which consumes it)
    // and before the Router (whose layout.rs also needs it). Providing here ensures
    // both the tray sync coroutine and all routed views see the same store.
    use_context_provider(|| Signal::new(AgentStore::new()));
    // WHY: Provide here (not layout.rs) so they are scoped to the connected state
    // and not the connect view.
    use_context_provider(|| Signal::new(CommandStore::new()));
    use_context_provider(|| Signal::new(TabBar::new()));
    use_context_provider(|| Signal::new(None::<ChatSelection>));
    // WHY: Planning surfaces are capability-driven so the public desktop build
    // does not advertise planning modules that Pylon cannot back.
    use_context_provider(|| Signal::new(PlanningCapabilities::default_public()));

    // WHY: Start SSE coroutine here (not in App) so it only runs when connected
    // and has access to the finalized connection config.
    crate::services::sse_coroutine::start_sse_coroutine(&config.read());

    // WHY: Fetch agents immediately on connection so the sidebar nous roster
    // is populated. Without this, agents only appear when the Ops view is
    // visited — the roster would be empty on first launch.
    {
        let cfg = config.read().clone();
        let mut agents: Signal<AgentStore> = use_context();
        use_future(move || {
            let server_url = cfg.server_url.clone();
            let auth_token = cfg.auth_token.clone();
            let request_policy = cfg.request_policy.clone();
            async move {
                let Ok(client) = skene::api::client::ApiClient::with_request_policy(
                    &server_url,
                    auth_token,
                    request_policy,
                ) else {
                    return;
                };
                if let Ok(list) = client.agents().await {
                    agents.write().load_from_api(list);
                }
            }
        });
    }

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
    let connection_state: Signal<ConnectionState> = use_context();

    // WHY: Use use_effect so this re-runs whenever the agent store or connection
    // state signals change. Degraded readiness is surfaced as a warning tray
    // status rather than disconnected.
    use_effect(move || {
        let agent_store = agents.read();
        let connection_state = connection_state.read();
        let new_tray = platform::tray::derive_tray_state(&agent_store, &connection_state, true);
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

#[cfg(test)]
mod tests {
    use super::Route;

    /// Classify a route as operator-facing production navigation.
    ///
    /// This match is intentionally exhaustive so adding a new `Route` variant
    /// forces an explicit decision about whether it belongs in the operator nav.
    fn is_operator_route(route: Route) -> bool {
        match route {
            Route::Chat {}
            | Route::Files {}
            | Route::Planning {}
            | Route::PlanningProject { .. }
            | Route::Memory {}
            | Route::Metrics {}
            | Route::MetricsToolDetail { .. }
            | Route::Ops {}
            | Route::Sessions {}
            | Route::Meta {}
            | Route::Settings {} => true,
            #[cfg(debug_assertions)]
            Route::ComponentLibrary {} => false,
        }
    }

    #[test]
    fn production_route_inventory_excludes_component_library() {
        assert!(is_operator_route(Route::Chat {}));
        assert!(is_operator_route(Route::Files {}));
        assert!(is_operator_route(Route::Planning {}));
        assert!(is_operator_route(Route::PlanningProject {
            project_id: "test".to_string(),
        }));
        assert!(is_operator_route(Route::Memory {}));
        assert!(is_operator_route(Route::Metrics {}));
        assert!(is_operator_route(Route::MetricsToolDetail {
            tool_name: "test".to_string(),
        }));
        assert!(is_operator_route(Route::Ops {}));
        assert!(is_operator_route(Route::Sessions {}));
        assert!(is_operator_route(Route::Meta {}));
        assert!(is_operator_route(Route::Settings {}));

        #[cfg(debug_assertions)]
        assert!(!is_operator_route(Route::ComponentLibrary {}));
    }
}
