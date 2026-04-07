//! Layout shell with sidebar navigation, agent presence, and content area.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::agent_sidebar::AgentSidebarView;
use crate::components::connection_indicator::ConnectionIndicatorView;
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::commands::CommandStore;
use crate::state::navigation::NavAction;
use crate::state::pipeline::RoutingState;
use crate::state::view_preservation::ViewPreservationStore;

const SIDEBAR_STYLE: &str = "\
    width: 220px; \
    background: var(--bg-sidebar, var(--bg-surface)); \
    color: var(--text-primary); \
    padding: var(--space-4) 0; \
    display: flex; \
    flex-direction: column; \
    gap: var(--space-1); \
    flex-shrink: 0;\
";

const CONTENT_STYLE: &str = "\
    flex: 1; \
    display: flex; \
    flex-direction: column; \
    overflow: hidden; \
    background: var(--bg); \
    color: var(--text-primary);\
";

const BRAND_STYLE: &str = "\
    font-size: var(--text-lg); \
    font-weight: var(--weight-bold); \
    padding: var(--space-2) var(--space-4); \
    margin-bottom: var(--space-2); \
    color: var(--text-heading, var(--text-primary));\
";

const NAV_LINK_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-4); \
    border-radius: var(--radius-md); \
    color: var(--text-primary); \
    text-decoration: none; \
    font-size: var(--text-sm);\
";

const NAV_DIVIDER_STYLE: &str = "\
    height: 1px; \
    background: var(--border-separator); \
    margin: var(--space-2) var(--space-4);\
";

/// Layout shell rendered around all routes.
///
/// Provides `Signal<AgentStore>`, `Signal<CommandStore>`, and `Signal<TabBar>`
/// as context so child views can access them. The agent sidebar is rendered
/// here so it persists across route changes.
///
/// Global keyboard shortcuts (Ctrl+1--7, Ctrl+K, Escape) are handled here
/// via `onkeydown` on the root layout div.
#[component]
pub(crate) fn Layout() -> Element {
    // WHY: AgentStore is provided by ConnectedApp (app.rs) so tray sync and
    // all routed views share the same store instance.
    use_context_provider(|| Signal::new(CommandStore::new()));
    use_context_provider(|| Signal::new(TabBar::new()));
    use_context_provider(|| Signal::new(Option::<NavAction>::None));

    // WHY: View preservation store survives route changes so views can
    // save scroll position and input drafts before unmounting and restore
    // on return. This eliminates the 23-minute context-switch tax (#2411).
    use_context_provider(|| Signal::new(ViewPreservationStore::new()));

    // WHY: Routing state signal drives the transparent routing indicator
    // in the chat view. Updated by the SSE event processing pipeline.
    use_context_provider(|| Signal::new(Option::<RoutingState>::None));

    // Command palette open state -- shared with the keyboard handler.
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
