//! Session tab bar for quick-switching between open agent conversations.
//!
//! Reads `Signal<TabBar>` from context. Only shows agents the user has
//! actually interacted with (those with open tabs), not all agents.

use dioxus::prelude::*;

use crate::state::app::TabBar;

const TABS_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 4px; \
    padding: 8px 16px 0; \
    background: var(--bg); \
    border-bottom: 1px solid var(--border-separator); \
    overflow-x: auto;\
";

const TAB_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 6px; \
    padding: 6px 14px 7px; \
    border-radius: 6px 6px 0 0; \
    font-size: 13px; \
    color: var(--text-secondary); \
    cursor: pointer; \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-bottom: none; \
    white-space: nowrap;\
";

const TAB_ACTIVE_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 6px; \
    padding: 6px 14px 7px; \
    border-radius: 6px 6px 0 0; \
    font-size: 13px; \
    color: var(--text-primary); \
    cursor: pointer; \
    background: var(--bg); \
    border: 1px solid var(--accent); \
    border-bottom: 1px solid var(--bg); \
    white-space: nowrap;\
";

const CLOSE_BTN_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: 11px; \
    padding: 0 2px; \
    cursor: pointer; \
    border-radius: 3px;\
";

const UNREAD_BADGE_STYLE: &str = "\
    width: 7px; \
    height: 7px; \
    border-radius: 50%; \
    background: var(--accent); \
    flex-shrink: 0;\
";

/// Session tab bar.
///
/// Reads `Signal<TabBar>` from context. Renders a tab for each open session.
/// Clicking a tab sets it as the active tab; clicking × removes it.
#[component]
pub(crate) fn SessionTabsView() -> Element {
    let tab_bar = use_context::<Signal<TabBar>>();

    let tabs: Vec<(u64, String, bool, bool)> = {
        let bar = tab_bar.read();
        bar.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, t.title.clone(), i == bar.active, t.unread))
            .collect()
    };

    if tabs.is_empty() {
        return rsx! { div {} };
    }

    rsx! {
        div {
            style: "{TABS_BAR_STYLE}",
            for (tab_id , title , is_active , has_unread) in tabs {
                {
                    let mut tab_bar_for_click = tab_bar;
                    let mut tab_bar_for_close = tab_bar;
                    let style = if is_active { TAB_ACTIVE_STYLE } else { TAB_STYLE };

                    rsx! {
                        div {
                            key: "{tab_id}",
                            style: "{style}",
                            onclick: move |_| {
                                let mut bar = tab_bar_for_click.write();
                                if let Some(idx) = bar.tabs.iter().position(|t| t.id == tab_id) {
                                    bar.active = idx;
                                }
                            },
                            if has_unread {
                                div { style: "{UNREAD_BADGE_STYLE}" }
                            }
                            span { "{title}" }
                            span {
                                style: "{CLOSE_BTN_STYLE}",
                                title: "Close tab",
                                onclick: move |evt| {
                                    evt.stop_propagation();
                                    let mut bar = tab_bar_for_close.write();
                                    if let Some(idx) = bar.tabs.iter().position(|t| t.id == tab_id) {
                                        bar.close(idx);
                                    }
                                },
                                "×"
                            }
                        }
                    }
                }
            }
        }
    }
}
