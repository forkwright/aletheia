//! Top bar with connection/theme controls.
//!
//! Sits above the main content area. Brand and agent roster both live
//! solely in the sidebar ([`crate::layout`], [`crate::components::agent_sidebar`]);
//! the top bar stays lean.

use dioxus::prelude::*;

/// Top bar with controls.
#[component]
pub(crate) fn TopBar() -> Element {
    rsx! {
        div {
            class: "app-topbar",
            role: "banner",
            "aria-label": "Top bar",

            // Controls
            div {
                class: "app-topbar-controls",
                crate::components::theme_toggle::ThemeToggle {}
                crate::components::connection_indicator::ConnectionIndicatorView {}
            }
        }
    }
}
