//! Root component and route definitions.

use dioxus::prelude::*;

use crate::layout::Layout;
use crate::views::chat::Chat;
use crate::views::files::Files;
use crate::views::memory::Memory;
use crate::views::metrics::Metrics;
use crate::views::ops::Ops;
use crate::views::planning::Planning;
use crate::views::settings::Settings;

#[derive(Routable, Clone, Debug, PartialEq)]
pub enum Route {
    #[layout(Layout)]
    #[route("/")]
    Chat {},
    #[route("/files")]
    Files {},
    #[route("/planning")]
    Planning {},
    #[route("/memory")]
    Memory {},
    #[route("/metrics")]
    Metrics {},
    #[route("/ops")]
    Ops {},
    #[route("/settings")]
    Settings {},
}

/// Root component that mounts the router.
#[component]
pub fn App() -> Element {
    rsx! {
        Router::<Route> {}
    }
}
