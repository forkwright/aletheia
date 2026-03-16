//! Layout shell with sidebar navigation and content area.

use dioxus::prelude::*;

use crate::app::Route;

const SIDEBAR_STYLE: &str = "\
    width: 220px; \
    background: #1a1a2e; \
    color: #e0e0e0; \
    padding: 16px; \
    display: flex; \
    flex-direction: column; \
    gap: 4px; \
    flex-shrink: 0;\
";

const CONTENT_STYLE: &str = "\
    flex: 1; \
    padding: 24px; \
    overflow-y: auto; \
    background: #0f0f1a; \
    color: #e0e0e0;\
";

const BRAND_STYLE: &str = "\
    font-size: 18px; \
    font-weight: bold; \
    padding: 8px 12px; \
    margin-bottom: 16px; \
    color: #ffffff;\
";

const NAV_LINK_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 8px 12px; \
    border-radius: 6px; \
    color: #e0e0e0; \
    text-decoration: none;\
";

/// Layout shell rendered around all routes.
#[component]
pub(crate) fn Layout() -> Element {
    rsx! {
        div {
            style: "display: flex; height: 100vh; font-family: system-ui, -apple-system, sans-serif;",
            nav {
                style: "{SIDEBAR_STYLE}",
                div { style: "{BRAND_STYLE}", "Aletheia" }
                NavItem { to: Route::Chat {}, icon: "[C]", label: "Chat" }
                NavItem { to: Route::Files {}, icon: "[F]", label: "Files" }
                NavItem { to: Route::Planning {}, icon: "[P]", label: "Planning" }
                NavItem { to: Route::Memory {}, icon: "[M]", label: "Memory" }
                NavItem { to: Route::Metrics {}, icon: "[X]", label: "Metrics" }
                NavItem { to: Route::Ops {}, icon: "[O]", label: "Ops" }
                NavItem { to: Route::Settings {}, icon: "[S]", label: "Settings" }
            }
            main {
                style: "{CONTENT_STYLE}",
                Outlet::<Route> {}
            }
        }
    }
}

#[component]
fn NavItem(to: Route, icon: &'static str, label: &'static str) -> Element {
    rsx! {
        Link {
            to,
            style: "{NAV_LINK_STYLE}",
            span { "{icon}" }
            span { "{label}" }
        }
    }
}
