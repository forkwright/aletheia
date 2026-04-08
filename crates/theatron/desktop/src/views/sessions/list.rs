//! Session list component: paginated rows with sort options.

use dioxus::prelude::*;
use theatron_core::id::SessionId;

use crate::state::sessions::{
    SessionListStore, SessionSelectionStore, SessionSort, format_relative_time,
    session_display_status, status_color,
};

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    padding: 10px 12px; \
    border-bottom: 1px solid #222; \
    cursor: pointer; \
    transition: background 0.1s;\
";

const ROW_HOVER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    padding: 10px 12px; \
    border-bottom: 1px solid #222; \
    cursor: pointer; \
    background: #1a1a2e;\
";

const TITLE_STYLE: &str = "\
    flex: 1; \
    font-size: 14px; \
    color: #e0e0e0; \
    overflow: hidden; \
    text-overflow: ellipsis; \
    white-space: nowrap;\
";

const AGENT_BADGE_STYLE: &str = "\
    display: inline-block; \
    padding: 2px 8px; \
    border-radius: 4px; \
    font-size: 11px; \
    background: #2a2a4a; \
    color: #7a7aff; \
    white-space: nowrap;\
";

const META_STYLE: &str = "\
    font-size: 11px; \
    color: #888; \
    white-space: nowrap;\
";

const STATUS_DOT_STYLE: &str = "\
    width: 8px; \
    height: 8px; \
    border-radius: 50%; \
    flex-shrink: 0;\
";

const SORT_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 8px 12px; \
    border-bottom: 1px solid #2a2a3a; \
    font-size: 12px; \
    color: #888;\
";

const SORT_BTN_STYLE: &str = "\
    background: none; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 2px 8px; \
    font-size: 11px; \
    color: #aaa; \
    cursor: pointer;\
";

const SORT_BTN_ACTIVE_STYLE: &str = "\
    background: #2a2a4a; \
    border: 1px solid #4a4aff; \
    border-radius: 4px; \
    padding: 2px 8px; \
    font-size: 11px; \
    color: #7a7aff; \
    cursor: pointer;\
";

const CHECKBOX_STYLE: &str = "\
    width: 16px; \
    height: 16px; \
    accent-color: #4a4aff; \
    cursor: pointer; \
    flex-shrink: 0;\
";

const LIST_CONTAINER_STYLE: &str = "\
    flex: 1; \
    overflow-y: auto;\
";

const EMPTY_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: 12px; \
    color: #555;\
";

const LOAD_MORE_STYLE: &str = "\
    display: flex; \
    justify-content: center; \
    padding: 12px; \
    border-top: 1px solid #222;\
";

const LOAD_MORE_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 12px; \
    cursor: pointer;\
";

/// Sort bar above the session list.
#[component]
pub(crate) fn SessionSortBar(
    list_store: Signal<SessionListStore>,
    on_sort_change: EventHandler<SessionSort>,
) -> Element {
    let current_sort = list_store.read().sort;

    rsx! {
        div {
            style: "{SORT_BAR_STYLE}",
            span { "Sort:" }
            for sort in SessionSort::ALL {
                button {
                    style: if current_sort == *sort { "{SORT_BTN_ACTIVE_STYLE}" } else { "{SORT_BTN_STYLE}" },
                    onclick: {
                        let s = *sort;
                        move |_| on_sort_change.call(s)
                    },
                    "{sort.label()}"
                }
            }
        }
    }
}

/// A single session row in the list.
#[component]
pub(crate) fn SessionRow(
    session_id: SessionId,
    title: String,
    agent_name: String,
    message_count: u32,
    updated_at: String,
    status: String,
    is_selected: bool,
    on_click: EventHandler<SessionId>,
    on_toggle_select: EventHandler<SessionId>,
) -> Element {
    let mut hovered = use_signal(|| false);
    let is_hovered = *hovered.read();

    let status_dot_color = status_color(&status);
    let relative_time = format_relative_time(&updated_at);

    let id_for_click = session_id.clone();
    let id_for_toggle = session_id.clone();

    rsx! {
        div {
            style: if is_hovered { "{ROW_HOVER_STYLE}" } else { "{ROW_STYLE}" },
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| hovered.set(false),
            onclick: {
                let id = id_for_click;
                move |_| on_click.call(id.clone())
            },
            // Checkbox
            input {
                r#type: "checkbox",
                style: "{CHECKBOX_STYLE}",
                checked: is_selected,
                onclick: move |evt: Event<MouseData>| {
                    evt.stop_propagation();
                },
                onchange: {
                    let id = id_for_toggle;
                    move |_| on_toggle_select.call(id.clone())
                },
            }
            // Status dot
            div {
                style: "{STATUS_DOT_STYLE} background: {status_dot_color};",
            }
            // Title
            div {
                style: "{TITLE_STYLE}",
                "{title}"
            }
            // Agent badge
            span { style: "{AGENT_BADGE_STYLE}", "{agent_name}" }
            // Message count
            span { style: "{META_STYLE}", "{message_count} msgs" }
            // Last activity
            span { style: "{META_STYLE}", "{relative_time}" }
        }
    }
}

/// The main session list component.
#[component]
pub(crate) fn SessionList(
    list_store: Signal<SessionListStore>,
    mut selection_store: Signal<SessionSelectionStore>,
    on_select_session: EventHandler<SessionId>,
    on_sort_change: EventHandler<SessionSort>,
    on_load_more: EventHandler<()>,
) -> Element {
    let store = list_store.read();
    let sessions = &store.sessions;
    let has_more = store.has_more;

    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100%;",
            // Sort bar
            SessionSortBar {
                list_store,
                on_sort_change,
            }
            // Select all bar
            if !sessions.is_empty() {
                div {
                    style: "display: flex; align-items: center; gap: 8px; padding: 4px 12px; border-bottom: 1px solid #1a1a2e; font-size: 12px; color: #888;",
                    input {
                        r#type: "checkbox",
                        style: "{CHECKBOX_STYLE}",
                        checked: selection_store.read().select_all,
                        onchange: move |_| {
                            let all_ids: Vec<SessionId> = list_store.read().sessions
                                .iter()
                                .map(|s| s.id.clone())
                                .collect();
                            selection_store.write().toggle_all(&all_ids);
                        },
                    }
                    span { "{sessions.len()} sessions" }
                    if let Some(total) = store.total_count {
                        span { " of {total}" }
                    }
                }
            }
            // Session rows
            if sessions.is_empty() {
                div {
                    style: "{EMPTY_STYLE}",
                    div { style: "font-size: 48px;", "[S]" }
                    div { style: "font-size: 16px;", "No sessions found" }
                    if list_store.read().has_active_filters() {
                        div { style: "font-size: 13px;",
                            "Try adjusting your filters or search query."
                        }
                    }
                }
            } else {
                div {
                    style: "{LIST_CONTAINER_STYLE}",
                    for session in sessions.iter() {
                        SessionRow {
                            key: "{session.id}",
                            session_id: session.id.clone(),
                            title: session.label().to_string(),
                            agent_name: session.nous_id.to_string(),
                            message_count: session.message_count,
                            updated_at: session.updated_at.clone().unwrap_or_default(),
                            status: session_display_status(session).to_string(),
                            is_selected: selection_store.read().is_selected(&session.id),
                            on_click: move |id: SessionId| on_select_session.call(id),
                            on_toggle_select: move |id: SessionId| {
                                selection_store.write().toggle(&id);
                            },
                        }
                    }
                    // Load more
                    if has_more {
                        div {
                            style: "{LOAD_MORE_STYLE}",
                            button {
                                style: "{LOAD_MORE_BTN}",
                                onclick: move |_| on_load_more.call(()),
                                "Load more..."
                            }
                        }
                    }
                }
            }
        }
    }
}
