//! Layout shell with sidebar navigation, agent presence, and content area.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::agent_sidebar::AgentSidebarView;
use crate::components::connection_indicator::ConnectionIndicatorView;
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::commands::CommandStore;
use crate::state::navigation::NavAction;

const SIDEBAR_STYLE: &str = "\
    width: 220px; \
    background: var(--bg-sidebar, #1a1a2e); \
    color: var(--text-primary, #e0e0e0); \
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
    background: var(--bg, #0f0f1a); \
    color: var(--text-primary, #e0e0e0);\
";

const BRAND_STYLE: &str = "\
    font-size: 18px; \
    font-weight: bold; \
    padding: 8px 16px; \
    margin-bottom: 8px; \
    color: var(--text-heading, #ffffff);\
";

const NAV_LINK_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 8px 16px; \
    border-radius: 6px; \
    color: var(--text-primary, #e0e0e0); \
    text-decoration: none; \
    font-size: 13px;\
";

const NAV_DIVIDER_STYLE: &str = "\
    height: 1px; \
    background: var(--border-subtle, #2a2a3a); \
    margin: 8px 16px;\
";

/// Layout shell rendered around all routes.
///
/// Provides `Signal<AgentStore>`, `Signal<CommandStore>`, and `Signal<TabBar>`
/// as context so child views can access them. The agent sidebar is rendered
/// here so it persists across route changes.
///
/// Global keyboard shortcuts (Ctrl+1–7, Ctrl+K, Escape) are handled here
/// via `onkeydown` on the root layout div.
#[component]
pub(crate) fn Layout() -> Element {
    // WHY: Provide these signals here (not app.rs) so they are scoped to the
    // connected layout and not the connect view.
    use_context_provider(|| Signal::new(AgentStore::new()));
    use_context_provider(|| Signal::new(CommandStore::new()));
    use_context_provider(|| Signal::new(TabBar::new()));
    use_context_provider(|| Signal::new(Option::<NavAction>::None));

    // Command palette open state — shared with the keyboard handler.
    let palette_open = use_signal(|| false);

    let keyboard_handler = crate::services::keybindings::use_global_keyboard(palette_open);

    rsx! {
        div {
            style: "display: flex; height: 100vh; font-family: var(--font-body, system-ui, -apple-system, sans-serif);",
            // NOTE: tabindex="-1" + onkeydown lets the root div capture keyboard events.
            tabindex: "-1",
            onkeydown: keyboard_handler,
            "aria-label": "Aletheia application",

            nav {
                style: "{SIDEBAR_STYLE}",
                role: "navigation",
                "aria-label": "Main navigation",
                div { style: "{BRAND_STYLE}", "Aletheia" }
                NavItem { to: Route::Chat {}, icon: "[C]", label: "Chat", shortcut: "Ctrl+1" }
                NavItem { to: Route::Files {}, icon: "[F]", label: "Files", shortcut: "Ctrl+2" }
                NavItem { to: Route::Planning {}, icon: "[P]", label: "Planning", shortcut: "Ctrl+3" }
                NavItem { to: Route::Memory {}, icon: "[M]", label: "Memory", shortcut: "Ctrl+4" }
                NavItem { to: Route::Metrics {}, icon: "[X]", label: "Metrics", shortcut: "Ctrl+5" }
                NavItem { to: Route::Ops {}, icon: "[O]", label: "Ops", shortcut: "Ctrl+6" }
                NavItem { to: Route::Sessions {}, icon: "[T]", label: "Sessions", shortcut: "Ctrl+7" }
                NavItem { to: Route::Meta {}, icon: "[I]", label: "Insights", shortcut: "" }
                NavItem { to: Route::Settings {}, icon: "[S]", label: "Settings", shortcut: "" }
                div { style: "{NAV_DIVIDER_STYLE}", role: "separator" }
                AgentSidebarView {}
                div { style: "flex: 1;" }
                ConnectionIndicatorView {}
            }
            main {
                style: "{CONTENT_STYLE}",
                role: "main",
                "aria-label": "Main content",
                Outlet::<Route> {}
            }
        }
    }
}

#[component]
fn NavItem(
    to: Route,
    icon: &'static str,
    label: &'static str,
    shortcut: &'static str,
) -> Element {
    let title = if shortcut.is_empty() {
        label.to_string()
    } else {
        format!("{label} ({shortcut})")
    };
    rsx! {
        Link {
            to,
            style: "{NAV_LINK_STYLE}",
            "aria-label": "{title}",
            title: "{title}",
            span { "aria-hidden": "true", "{icon}" }
            span { "{label}" }
        }
    }
}
