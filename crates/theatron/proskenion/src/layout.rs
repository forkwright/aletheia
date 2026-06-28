//! Layout shell with sidebar navigation, agent presence, and content area.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::help_overlay::HelpOverlay;
use crate::components::topbar::TopBar;
use crate::state::commands::{CommandStore, CommandUiState};
use crate::state::navigation::NavAction;
use crate::state::pipeline::RoutingState;
use crate::state::view_preservation::ViewPreservationStore;

use crate::components::agent_sidebar::AgentSidebarView;

const SIDEBAR_BASE_STYLE: &str = "\
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
    transition: width var(--duration-slow) var(--ease-out-expo);\
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
    color: var(--text-primary);\
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

    // Command UI state -- shared with the keyboard handler and chat view.
    let mut command_ui = use_signal(CommandUiState::default);
    use_context_provider(|| command_ui);
    // Sidebar collapsed state -- default to expanded for better first impression.
    let sidebar_collapsed = use_signal(|| false);

    let command_store = use_context::<Signal<CommandStore>>();
    let keyboard_handler = crate::services::keybindings::use_global_keyboard(
        command_ui,
        command_store,
        sidebar_collapsed,
    );

    let sidebar_width = if *sidebar_collapsed.read() {
        "var(--sidebar-width-collapsed)"
    } else {
        "var(--sidebar-width)"
    };

    rsx! {
        div {
            // WHY: paint the shell root explicitly — body resolves dark-theme
            // tokens (data-theme lives on an inner div), so any unpainted gap
            // would render near-black on the light theme.
            style: "display: flex; height: 100vh; background: var(--bg); color: var(--text-primary); font-family: var(--font-body, system-ui, -apple-system, sans-serif);",
            // NOTE: tabindex="-1" + onkeydown lets the root div capture keyboard events.
            tabindex: "-1",
            onkeydown: keyboard_handler,
            "aria-label": "Aletheia application",

            nav {
                style: "width: {sidebar_width}; {SIDEBAR_BASE_STYLE}",
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
                // ── Workspace section ──
                if !*sidebar_collapsed.read() {
                    div { class: "sidebar-section-label", "Workspace" }
                }
                NavItem { to: Route::Chat {}, icon: "💬", label: "Chat", shortcut: "Ctrl+1", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Files {}, icon: "📁", label: "Theke", shortcut: "Ctrl+2", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Planning {}, icon: "📋", label: "Planning", shortcut: "Ctrl+3", collapsed: *sidebar_collapsed.read() }

                // ── Knowledge section ──
                // WHY: the section label carries its own top-rule (.sidebar-section-label),
                // so no explicit divider is needed — the ruled label is the separator.
                // When collapsed (no labels) the divider provides the section break.
                if *sidebar_collapsed.read() {
                    div { class: "sidebar-divider" }
                } else {
                    div { class: "sidebar-section-label", "Knowledge" }
                }
                NavItem { to: Route::Memory {}, icon: "🧠", label: "Memory", shortcut: "Ctrl+4", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Metrics {}, icon: "📊", label: "Metrics", shortcut: "Ctrl+5", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Sessions {}, icon: "📑", label: "Sessions", shortcut: "Ctrl+7", collapsed: *sidebar_collapsed.read() }

                // ── System section ──
                if *sidebar_collapsed.read() {
                    div { class: "sidebar-divider" }
                } else {
                    div { class: "sidebar-section-label", "System" }
                }
                NavItem { to: Route::Ops {}, icon: "⚙\u{fe0f}", label: "Ops", shortcut: "Ctrl+6", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Meta {}, icon: "💡", label: "Insights", shortcut: "", collapsed: *sidebar_collapsed.read() }
                NavItem { to: Route::Settings {}, icon: "🔧", label: "Settings", shortcut: "", collapsed: *sidebar_collapsed.read() }

                // WHY: agent presence persists across route changes; the roster
                // sits below nav so it reads as a distinct shaded panel.
                AgentSidebarView { collapsed: *sidebar_collapsed.read() }
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

            // Help overlay (F1 or /help)
            HelpOverlay {
                visible: command_ui.read().help_visible,
                on_close: move |()| {
                    command_ui.write().help_visible = false;
                },
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
    collapsed: bool,
) -> Element {
    let title = if shortcut.is_empty() {
        label.to_string()
    } else {
        format!("{label} ({shortcut})")
    };
    // WHY: .nav-link reserves a transparent 3px active edge so activation does
    // not shift the row sideways (no reflow); .nav-link-collapsed layers the
    // centered icon-only overrides on top. Active state comes from the
    // Link-emitted aria-current="page" matched by the .nav-link CSS rule.
    let class = if collapsed {
        "nav-link nav-link-collapsed"
    } else {
        "nav-link"
    };
    rsx! {
        Link {
            to,
            class: "{class}",
            "aria-label": "{title}",
            title: "{title}",
            span { "aria-hidden": "true", "{icon}" }
            if !collapsed {
                span { "{label}" }
            }
        }
    }
}
