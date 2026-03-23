//! Root component that gates on connection state.
//!
//! When disconnected, shows the connect view. When connected, shows the
//! main layout with the router. This is the entry point for the Dioxus
//! component tree.

use dioxus::prelude::*;

use crate::layout::Layout;
use crate::services::{config, settings_config};
use crate::services::toast::provide_toast_context;
use crate::state::connection::ConnectionState;
use crate::theme::ThemeProvider;
use crate::views::chat::Chat;
use crate::views::connect::ConnectView;
use crate::views::files::Files;
use crate::views::memory::Memory;
use crate::views::metrics::Metrics;
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
        #[route("/ops")]
        Ops {},
        #[route("/sessions")]
        Sessions {},
        #[route("/settings")]
        Settings {},
}

/// Root component.
///
/// Provides connection state, config, settings, and toast store as context
/// signals, then gates on wizard → connect → connected.
#[component]
pub(crate) fn App() -> Element {
    let loaded_settings = use_hook(settings_config::load_or_default);
    let loaded_config = use_hook(config::load_or_default);
    let initial_theme = loaded_settings.appearance_settings().theme_mode();
    let first_run = use_hook(settings_config::is_first_run);

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
/// Starts the SSE coroutine and renders the router with toast overlay.
#[component]
fn ConnectedApp() -> Element {
    let config = use_context::<Signal<crate::state::connection::ConnectionConfig>>();

    // WHY: Start SSE coroutine here (not in App) so it only runs when connected
    // and has access to the finalized connection config.
    crate::services::sse_coroutine::start_sse_coroutine(&config.read());

    rsx! {
        crate::components::toast_container::ToastContainer {}
        Router::<Route> {}
    }
}
