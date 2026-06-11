//! Top bar with brand and connection/theme controls.
//!
//! Sits above the main content area. The agent roster lives solely in the
//! sidebar ([`crate::components::agent_sidebar`]); the top bar stays lean.

use dioxus::prelude::*;

const TOPBAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    height: 52px; \
    padding: 0 var(--space-4); \
    background: var(--bg-surface); \
    border-bottom: 1px solid var(--border-separator); \
    flex-shrink: 0; \
    gap: var(--space-4);\
";

const BRAND_STYLE: &str = "\
    font-family: var(--font-display); \
    font-size: var(--text-lg); \
    font-weight: var(--weight-semibold); \
    color: var(--accent); \
    flex-shrink: 0; \
    letter-spacing: 0.02em;\
";

const CONTROLS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    flex-shrink: 0; \
    margin-left: auto;\
";

/// Top bar with brand and controls.
#[component]
pub(crate) fn TopBar() -> Element {
    rsx! {
        div {
            style: "{TOPBAR_STYLE}",
            role: "banner",
            "aria-label": "Top bar",

            // Brand
            span { style: "{BRAND_STYLE}", "Aletheia" }

            // Controls
            div {
                style: "{CONTROLS_STYLE}",
                crate::components::theme_toggle::ThemeToggle {}
                crate::components::connection_indicator::ConnectionIndicatorView {}
            }
        }
    }
}
