//! Session tab bar for quick-switching between open agent conversations.
//!
//! Reads `Signal<TabBar>` from context. Only shows agents the user has
//! actually interacted with (those with open tabs), not all agents.

use dioxus::prelude::*;
use skene::id::{NousId, SessionId};

use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::chat::ChatSelection;

const TABS_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-1); \
    padding: var(--space-2) var(--space-4) 0; \
    background: var(--bg); \
    border-bottom: 1px solid var(--border-separator); \
    overflow-x: auto; \
    overflow-y: hidden;\
";

const TAB_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-4) var(--space-2); \
    border-radius: var(--radius-lg) var(--radius-lg) 0 0; \
    font-size: var(--text-sm); \
    color: var(--text-secondary); \
    cursor: pointer; \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-bottom: none; \
    white-space: nowrap; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TAB_ACTIVE_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-4) var(--space-2); \
    border-radius: var(--radius-lg) var(--radius-lg) 0 0; \
    font-size: var(--text-sm); \
    color: var(--text-primary); \
    cursor: pointer; \
    background: var(--bg); \
    border: 1px solid var(--accent); \
    border-bottom: 1px solid var(--bg); \
    white-space: nowrap; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const CLOSE_BTN_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    padding: 0 var(--space-1); \
    cursor: pointer; \
    border-radius: var(--radius-sm); \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const UNREAD_BADGE_STYLE: &str = "\
    width: 8px; \
    height: 8px; \
    border-radius: 50%; \
    background: var(--accent); \
    flex-shrink: 0;\
";

type TabSnapshot = (
    u64,
    NousId,
    Option<String>,
    Option<SessionId>,
    Option<i64>,
    String,
    bool,
    bool,
);

/// Session tab bar.
///
/// Reads `Signal<TabBar>` from context. Renders a tab for each open session.
/// Clicking a tab sets it as the active tab; clicking × removes it.
#[component]
pub(crate) fn SessionTabsView() -> Element {
    let tab_bar = use_context::<Signal<TabBar>>();
    let agent_store = use_context::<Signal<AgentStore>>();
    let chat_selection = use_context::<Signal<Option<ChatSelection>>>();

    let tabs: Vec<TabSnapshot> = {
        let bar = tab_bar.read();
        bar.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                (
                    t.id,
                    t.agent_id.clone(),
                    t.session_key.clone(),
                    t.session_id.clone(),
                    t.message_count,
                    t.title.clone(),
                    i == bar.active,
                    t.unread,
                )
            })
            .collect()
    };

    if tabs.is_empty() {
        return rsx! { div {} };
    }

    rsx! {
        div {
            style: "{TABS_BAR_STYLE}",
            for (tab_id , agent_id , session_key , session_id , message_count , title , is_active , has_unread) in tabs {
                {
                    let mut tab_bar_for_click = tab_bar;
                    let mut tab_bar_for_close = tab_bar;
                    let mut agent_store_for_click = agent_store;
                    let mut chat_selection_for_click = chat_selection;
                    let agent_id_for_click = agent_id.clone();
                    let session_key_for_click = session_key.clone();
                    let session_id_for_click = session_id.clone();
                    let message_count_for_click = message_count;
                    let title_for_click = title.clone();
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
                                agent_store_for_click.write().set_active(&agent_id_for_click);
                                if let Some(session_key) = session_key_for_click.clone() {
                                    let selection = match session_id_for_click.clone() {
                                        Some(session_id) => ChatSelection {
                                            agent_id: agent_id_for_click.clone(),
                                            session_id: Some(session_id),
                                            session_key,
                                            title: title_for_click.clone(),
                                            message_count: message_count_for_click,
                                        },
                                        _ => ChatSelection::new(
                                            agent_id_for_click.clone(),
                                            session_key,
                                            title_for_click.clone(),
                                        ),
                                    };
                                    chat_selection_for_click.set(Some(selection));
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
                                "\u{2715}"
                            }
                        }
                    }
                }
            }
        }
    }
}
