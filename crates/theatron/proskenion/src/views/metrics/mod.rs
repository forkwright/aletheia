//! Metrics dashboard: token usage and cost views with charts and breakdowns.

mod agent_breakdown;
mod agent_costs;
mod costs;
mod model_breakdown;
mod tokens;
pub(crate) mod tool_detail;
mod tool_duration;
mod tool_frequency;
mod tool_results;
mod tools;

use dioxus::prelude::*;

use crate::state::metrics::MetricsTab;

const TAB_BAR_STYLE: &str = "\
    display: flex; \
    gap: var(--space-1); \
    border-bottom: 1px solid var(--border); \
    padding: 0 var(--space-4); \
    margin-bottom: var(--space-4);\
";

const TAB_ACTIVE_STYLE: &str = "\
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    color: var(--text-primary); \
    background: transparent; \
    border: none; \
    border-bottom: 2px solid var(--accent); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    font-family: var(--font-mono);\
";

const TAB_INACTIVE_STYLE: &str = "\
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    color: var(--text-muted); \
    background: transparent; \
    border: none; \
    border-bottom: 2px solid transparent; \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    font-family: var(--font-mono);\
";

/// Metrics dashboard root: tab bar + delegated tab view.
#[component]
pub(crate) fn Metrics() -> Element {
    let mut active_tab = use_signal(|| MetricsTab::Tokens);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100%; overflow: hidden;",

            div {
                style: "{TAB_BAR_STYLE}",
                button {
                    style: if *active_tab.read() == MetricsTab::Tokens { TAB_ACTIVE_STYLE } else { TAB_INACTIVE_STYLE },
                    onclick: move |_| active_tab.set(MetricsTab::Tokens),
                    "Tokens"
                }
                button {
                    style: if *active_tab.read() == MetricsTab::Costs { TAB_ACTIVE_STYLE } else { TAB_INACTIVE_STYLE },
                    onclick: move |_| active_tab.set(MetricsTab::Costs),
                    "Costs"
                }
            }

            div {
                style: "flex: 1; overflow-y: auto; padding: 0 var(--space-4) var(--space-4);",
                match *active_tab.read() {
                    MetricsTab::Tokens => rsx! { tokens::Tokens {} },
                    MetricsTab::Costs => rsx! { costs::Costs {} },
                }
            }
        }
    }
}
