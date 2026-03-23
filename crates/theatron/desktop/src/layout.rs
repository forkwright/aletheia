//! Layout shell with sidebar navigation, agent presence, and content area.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::agent_sidebar::AgentSidebarView;
use crate::components::connection_indicator::ConnectionIndicatorView;
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::commands::CommandStore;

const SIDEBAR_STYLE: &str = "\
    width: 220px; \
    background: #1a1a2e; \
    color: #e0e0e0; \
    padding: 16px 0; \
    display: flex; \
    flex-direction: column; \
    gap: 4px; \
    flex-shrink: 0;\
";

const CONTENT_STYLE: &str = "\
    flex: 1; \
    display: flex; \
    flex-direction: column; \
    overflow: hidden; \
    background: #0f0f1a; \
    color: #e0e0e0;\
";

const BRAND_STYLE: &str = "\
    font-size: 18px; \
    font-weight: bold; \
    padding: 8px 16px; \
    margin-bottom: 8px; \
    color: #ffffff;\
";

const NAV_LINK_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 8px 16px; \
    border-radius: 6px; \
    color: #e0e0e0; \
    text-decoration: none; \
    font-size: 13px;\
";

const NAV_DIVIDER_STYLE: &str = "\
    height: 1px; \
    background: #2a2a3a; \
    margin: 8px 16px;\
";

/// Layout shell rendered around all routes.
///
/// Provides `Signal<AgentStore>`, `Signal<CommandStore>`, and `Signal<TabBar>`
/// as context so child views can access them. The agent sidebar is rendered
/// here so it persists across route changes.
#[component]
pub(crate) fn Layout() -> Element {
    // WHY: Provide these signals here (not app.rs) so they are scoped to the
    // connected layout and not the connect view.
    use_context_provider(|| Signal::new(AgentStore::new()));
    use_context_provider(|| Signal::new(CommandStore::new()));
    use_context_provider(|| Signal::new(TabBar::new()));

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
                div { style: "{NAV_DIVIDER_STYLE}" }
                AgentSidebarView {}
                div { style: "flex: 1;" }
                ConnectionIndicatorView {}
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
