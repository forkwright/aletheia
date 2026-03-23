//! Root component that gates on connection state.
//!
//! When disconnected, shows the connect view. When connected, shows the
//! main layout with the router. This is the entry point for the Dioxus
//! component tree.

use dioxus::prelude::*;

use crate::layout::Layout;
use crate::services::config;
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
use crate::views::settings::Settings;

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
        #[route("/settings")]
        Settings {},
}

/// Root component.
///
/// Provides connection state, config, and toast store as context signals,
/// then renders either the connect view or the main content based on
/// connection state.
#[component]
pub(crate) fn App() -> Element {
    let loaded_config = use_hook(config::load_or_default);
    let connection_state = use_signal(ConnectionState::default);
    let connection_config = use_signal(|| loaded_config);

    // NOTE: Provide signals as context so all views can access them.
    use_context_provider(|| connection_state);
    use_context_provider(|| connection_config);
    provide_toast_context();

    let needs_connect = connection_state.read().needs_connect_view();

    rsx! {
        ThemeProvider {
            if needs_connect {
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
