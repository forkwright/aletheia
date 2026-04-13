//! Layout shell with sidebar navigation, agent presence, and content area.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::help_overlay::HelpOverlay;
use crate::components::topbar::TopBar;
use crate::state::navigation::NavAction;
use crate::state::pipeline::RoutingState;
use crate::state::view_preservation::ViewPreservationStore;

const SIDEBAR_COLLAPSED_STYLE: &str = "\
    width: 48px; \
    background: var(--bg-surface); \
    color: var(--text-primary); \
    padding: var(--space-4) 0; \
    display: flex; \
    flex-direction: column; \
    gap: var(--space-1); \
    flex-shrink: 0; \
    border-right: 1px solid var(--border-separator); \
    overflow-y: auto; \
    overflow-x: hidden; \
    transition: width var(--duration-slow, 350ms) cubic-bezier(0.16, 1, 0.3, 1);\
";

const SIDEBAR_EXPANDED_STYLE: &str = "\
    width: 220px; \
    background: var(--bg-surface); \
    color: var(--text-primary); \
    padding: var(--space-4) 0; \
    display: flex; \
    flex-direction: column; \
    gap: var(--space-1); \
    flex-shrink: 0; \
    border-right: 1px solid var(--border-separator); \
    overflow-y: auto; \
    overflow-x: hidden; \
    transition: width var(--duration-slow, 350ms) cubic-bezier(0.16, 1, 0.3, 1);\
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
    font-size: var(--text-sm); \
    white-space: nowrap; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick);\
";

const NAV_LINK_ICON_ONLY_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    padding: var(--space-2) var(--space-3); \
    border-radius: var(--radius-md); \
    color: var(--text-secondary); \
    text-decoration: none; \
    font-size: var(--text-md); \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick);\
";


/// Layout shell rendered around all routes.
///
/// Provides `Signal<NavAction>` as context so child views can access it.
/// The agent sidebar is rendered here so it persists across route changes.
///
/// Global keyboard shortcuts (Ctrl+1--7, Ctrl+K, Ctrl+B, Escape) are handled
/// here via `onkeydown` on the root layout div.
#[component]
pub(crate) fn Layout() -> Element {
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
    // Sidebar collapsed state -- default to expanded for better first impression.
    let sidebar_collapsed = use_signal(|| false);
    // Help overlay visibility -- toggled by F1.
    let help_visible = use_signal(|| false);

    let keyboard_handler = crate::services::keybindings::use_global_keyboard(
        palette_open,
        sidebar_collapsed,
        help_visible,
    );

    let sidebar_style = if *sidebar_collapsed.read() {
        SIDEBAR_COLLAPSED_STYLE
    } else {
        SIDEBAR_EXPANDED_STYLE
    };

    rsx! {
        div {
            style: "display: flex; height: 100vh; font-family: var(--font-body, system-ui, -apple-system, sans-serif);",
            // NOTE: tabindex="-1" + onkeydown lets the root div capture keyboard events.
            tabindex: "-1",
            onkeydown: keyboard_handler,
            "aria-label": "Aletheia application",

            nav {
                style: "{sidebar_style}",
                role: "navigation",
                "aria-label": "Main navigation",
                if !*sidebar_collapsed.read() {
                    div { style: "{BRAND_STYLE}", "Aletheia" }
                } else {
                    div {
                        style: "font-size: var(--text-lg); font-weight: var(--weight-bold); padding: var(--space-2) 0; margin-bottom: var(--space-2); text-align: center; color: var(--accent);",
                        "A"
                    }
                }
                // -- WORKSPACE section --
                if !*sidebar_collapsed.read() {
                    div {
                        style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); text-transform: uppercase; letter-spacing: 0.04em; color: var(--text-muted); padding: var(--space-3) var(--space-4) var(--space-1);",
                        "Workspace"
                    }
                }
                NavItem { to: Route::Chat {}, icon: "💬", label: "Chat", shortcut: "Ctrl+1", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Files {}, icon: "📁", label: "Files", shortcut: "Ctrl+2", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Planning {}, icon: "📋", label: "Planning", shortcut: "Ctrl+3", collapsed: *sidebar_collapsed.read() }

                // -- Divider --
                div { style: "height: 1px; background: var(--border-separator); margin: var(--space-2) var(--space-3);" }

                // -- KNOWLEDGE section --
                if !*sidebar_collapsed.read() {
                    div {
                        style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); text-transform: uppercase; letter-spacing: 0.04em; color: var(--text-muted); padding: var(--space-3) var(--space-4) var(--space-1);",
                        "Knowledge"
                    }
                }
                NavItem { to: Route::Memory {}, icon: "🧠", label: "Memory", shortcut: "Ctrl+4", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Metrics {}, icon: "📊", label: "Metrics", shortcut: "Ctrl+5", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Sessions {}, icon: "📑", label: "Sessions", shortcut: "Ctrl+7", collapsed: *sidebar_collapsed.read() }

                // -- Divider --
                div { style: "height: 1px; background: var(--border-separator); margin: var(--space-2) var(--space-3);" }

                // -- SYSTEM section --
                if !*sidebar_collapsed.read() {
                    div {
                        style: "font-size: var(--text-xs); font-weight: var(--weight-semibold); text-transform: uppercase; letter-spacing: 0.04em; color: var(--text-muted); padding: var(--space-3) var(--space-4) var(--space-1);",
                        "System"
                    }
                }
                NavItem { to: Route::Ops {}, icon: "⚙\u{fe0f}", label: "Ops", shortcut: "Ctrl+6", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Meta {}, icon: "💡", label: "Insights", shortcut: "", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Settings {}, icon: "🔧", label: "Settings", shortcut: "", collapsed: *sidebar_collapsed.read() }
            }
            // Content area: topbar + main
            div {
                style: "flex: 1; display: flex; flex-direction: column; overflow: hidden;",
                TopBar {}
                main {
                    style: "{CONTENT_STYLE}",
                    role: "main",
                    "aria-label": "Main content",
                    Outlet::<Route> {}
                }
            }

            // Help overlay (F1)
            HelpOverlay { visible: help_visible }
        }
    }
}

#[component]
fn NavItem(
    to: Route,
    icon: &'static str,
    label: &'static str,
    shortcut: &'static str,
    collapsed: bool,
) -> Element {
    let title = if shortcut.is_empty() {
        label.to_string()
    } else {
        format!("{label} ({shortcut})")
    };
    let style = if collapsed {
        NAV_LINK_ICON_ONLY_STYLE
    } else {
        NAV_LINK_STYLE
    };
    rsx! {
        Link {
            to,
            style: "{style}",
            "aria-label": "{title}",
            title: "{title}",
            span { "aria-hidden": "true", "{icon}" }
            if !collapsed {
                span { "{label}" }
            }
        }
    }
}
