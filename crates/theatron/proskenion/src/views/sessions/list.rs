//! Session list component: nous-grouped rows with sort options and a system-session filter.

use std::collections::HashSet;

use dioxus::prelude::*;
use skene::api::types::Session;
use skene::id::SessionId;

use crate::state::sessions::{
    SessionListStore, SessionSelectionStore, SessionSort, format_relative_time,
    session_display_status, status_color,
};

// WHY: row hover lives in CSS (`.session-row:hover` in base.css), not in a
// Dioxus signal. A signal-driven hover re-rendered the row subtree on every
// pointer enter/leave (visible jank); a CSS :hover reflows nothing and never
// re-renders.

const TITLE_STYLE: &str = "\
    flex: 1; \
    font-size: var(--text-base); \
    color: var(--text-primary); \
    overflow: hidden; \
    text-overflow: ellipsis; \
    white-space: nowrap;\
";

const META_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
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
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-3); \
    border-bottom: 1px solid var(--border); \
    font-size: var(--text-xs); \
    color: var(--text-secondary);\
";

const SORT_BTN_STYLE: &str = "\
    background: none; \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const SORT_BTN_ACTIVE_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border-selected); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const CHECKBOX_STYLE: &str = "\
    width: 16px; \
    height: 16px; \
    accent-color: var(--accent); \
    cursor: pointer; \
    flex-shrink: 0; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
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
    gap: var(--space-3); \
    color: var(--text-muted);\
";

const LOAD_MORE_STYLE: &str = "\
    display: flex; \
    justify-content: center; \
    padding: var(--space-3); \
    border-top: 1px solid var(--border-separator);\
";

const LOAD_MORE_BTN: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const SELECT_ALL_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) var(--space-3); \
    border-bottom: 1px solid var(--border-separator); \
    font-size: var(--text-xs); \
    color: var(--text-secondary);\
";

const GROUP_HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-3); \
    background: var(--bg-surface); \
    border-bottom: 1px solid var(--border-separator); \
    cursor: pointer; \
    user-select: none; \
    position: sticky; \
    top: 0; \
    z-index: 1; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const GROUP_CARET_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    flex-shrink: 0;\
";

const GROUP_NAME_STYLE: &str = "\
    font-size: var(--text-sm); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary); \
    overflow: hidden; \
    text-overflow: ellipsis; \
    white-space: nowrap;\
";

const GROUP_COUNT_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted);\
";

const SYSTEM_TOGGLE_STYLE: &str = "\
    display: inline-flex; \
    align-items: center; \
    gap: var(--space-1); \
    margin-left: auto; \
    color: var(--text-secondary); \
    cursor: pointer; \
    white-space: nowrap;\
";

/// Session-key prefixes that mark background/system bookkeeping rather than conversation.
const SYSTEM_KEY_PREFIXES: &[&str] = &[
    "daemon:",
    "cron:",
    "eval-",
    "web:",
    "test",
    "diagnostic",
    "cross:",
];

/// Whether a session is a system/background session (hidden by default).
fn is_system_session(session: &Session) -> bool {
    SYSTEM_KEY_PREFIXES
        .iter()
        .any(|prefix| session.key.starts_with(prefix))
}

/// Group sessions by owning nous, preserving the store's sort order within
/// groups and ordering groups by first appearance (top item of current sort).
fn group_by_nous(sessions: Vec<Session>) -> Vec<(String, Vec<Session>)> {
    let mut groups: Vec<(String, Vec<Session>)> = Vec::new();
    for session in sessions {
        let nous = session.nous_id.to_string();
        if let Some((_, rows)) = groups.iter_mut().find(|(id, _)| *id == nous) {
            rows.push(session);
        } else {
            groups.push((nous, vec![session]));
        }
    }
    groups
}

/// Sort bar above the session list, with the system-session visibility toggle.
#[component]
pub(crate) fn SessionSortBar(
    list_store: Signal<SessionListStore>,
    on_sort_change: EventHandler<SessionSort>,
    mut show_system: Signal<bool>,
    system_count: usize,
) -> Element {
    let current_sort = list_store.read().sort;
    let system_shown = *show_system.read();

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
            label {
                style: "{SYSTEM_TOGGLE_STYLE}",
                title: "Show system sessions (daemon, cron, eval, web, test, diagnostic, cross, background)",
                input {
                    r#type: "checkbox",
                    style: "{CHECKBOX_STYLE}",
                    checked: system_shown,
                    onchange: move |_| {
                        let shown = *show_system.read();
                        show_system.set(!shown);
                    },
                }
                span { "System" }
                if system_count > 0 {
                    span { style: "{GROUP_COUNT_STYLE}", "({system_count})" }
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
    message_count: i64,
    updated_at: String,
    status: String,
    is_selected: bool,
    on_click: EventHandler<SessionId>,
    on_toggle_select: EventHandler<SessionId>,
) -> Element {
    let status_dot_color = status_color(&status);
    let relative_time = format_relative_time(&updated_at);

    let id_for_click = session_id.clone();
    let id_for_toggle = session_id.clone();

    rsx! {
        div {
            class: "session-row",
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
            // Message count
            span { style: "{META_STYLE}", "{message_count} msgs" }
            // Last activity
            span { style: "{META_STYLE}", "{relative_time}" }
        }
    }
}

/// The main session list component: sessions grouped under collapsible nous sections.
#[component]
pub(crate) fn SessionList(
    list_store: Signal<SessionListStore>,
    mut selection_store: Signal<SessionSelectionStore>,
    on_select_session: EventHandler<SessionId>,
    on_sort_change: EventHandler<SessionSort>,
    on_load_more: EventHandler<()>,
) -> Element {
    let show_system = use_signal(|| false);
    let collapsed = use_signal(HashSet::<String>::new);

    let store = list_store.read();
    let system_shown = *show_system.read();
    let system_count = store
        .sessions
        .iter()
        .filter(|s| is_system_session(s))
        .count();
    let visible: Vec<Session> = store
        .sessions
        .iter()
        .filter(|s| system_shown || !is_system_session(s))
        .cloned()
        .collect();
    let visible_count = visible.len();
    let hidden_count = if system_shown { 0 } else { system_count };
    let has_more = store.has_more;
    let total_count = store.total_count;
    let has_filters = store.has_active_filters();
    drop(store);

    let collapsed_set = collapsed.read().clone();
    let groups: Vec<(String, bool, Vec<Session>)> = group_by_nous(visible)
        .into_iter()
        .map(|(nous, rows)| {
            let is_collapsed = collapsed_set.contains(&nous);
            (nous, is_collapsed, rows)
        })
        .collect();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100%;",
            // Sort bar + system toggle
            SessionSortBar {
                list_store,
                on_sort_change,
                show_system,
                system_count,
            }
            // Select all bar
            if visible_count > 0 {
                div {
                    style: "{SELECT_ALL_BAR_STYLE}",
                    input {
                        r#type: "checkbox",
                        style: "{CHECKBOX_STYLE}",
                        checked: selection_store.read().select_all,
                        onchange: move |_| {
                            let shown = *show_system.read();
                            let visible_ids: Vec<SessionId> = list_store.read().sessions
                                .iter()
                                .filter(|s| shown || !is_system_session(s))
                                .map(|s| s.id.clone())
                                .collect();
                            selection_store.write().toggle_all(&visible_ids);
                        },
                    }
                    span { "{visible_count} sessions" }
                    if let Some(total) = total_count {
                        span { " of {total}" }
                    }
                    if hidden_count > 0 {
                        span {
                            style: "color: var(--text-muted);",
                            "({hidden_count} system hidden)"
                        }
                    }
                }
            }
            // Grouped session rows
            if visible_count == 0 {
                div {
                    style: "{EMPTY_STYLE}",
                    div { style: "font-size: var(--text-3xl);", "[S]" }
                    div { style: "font-size: var(--text-md);", "No sessions found" }
                    if hidden_count > 0 {
                        div { style: "font-size: var(--text-sm);",
                            "{hidden_count} system sessions hidden — toggle System to show them."
                        }
                    } else if has_filters {
                        div { style: "font-size: var(--text-sm);",
                            "Try adjusting your filters or search query."
                        }
                    }
                }
            } else {
                div {
                    style: "{LIST_CONTAINER_STYLE}",
                    for (nous , is_collapsed , rows) in groups {
                        div {
                            key: "{nous}",
                            // Nous section header
                            div {
                                style: "{GROUP_HEADER_STYLE}",
                                role: "button",
                                "aria-expanded": if is_collapsed { "false" } else { "true" },
                                onclick: {
                                    let nous_id = nous.clone();
                                    let mut collapsed = collapsed;
                                    move |_| {
                                        let mut set = collapsed.write();
                                        if !set.remove(&nous_id) {
                                            set.insert(nous_id.clone());
                                        }
                                    }
                                },
                                span {
                                    style: "{GROUP_CARET_STYLE}",
                                    if is_collapsed { "\u{25B8}" } else { "\u{25BE}" }
                                }
                                span { style: "{GROUP_NAME_STYLE}", "{nous}" }
                                span { style: "{GROUP_COUNT_STYLE}", "{rows.len()}" }
                            }
                            if !is_collapsed {
                                for session in rows {
                                    SessionRow {
                                        key: "{session.id}",
                                        session_id: session.id.clone(),
                                        title: session.label().to_string(),
                                        message_count: session.message_count,
                                        updated_at: session.updated_at.clone(),
                                        status: session_display_status(&session).to_string(),
                                        is_selected: selection_store.read().is_selected(&session.id),
                                        on_click: move |id: SessionId| on_select_session.call(id),
                                        on_toggle_select: move |id: SessionId| {
                                            selection_store.write().toggle(&id);
                                        },
                                    }
                                }
                            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn session(nous: &str, key: &str) -> Session {
        Session {
            id: SessionId::from(key),
            nous_id: skene::id::NousId::from(nous),
            key: key.to_string(),
            status: "active".to_string(),
            model: None,
            message_count: 1,
            token_count_estimate: 0,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            display_name: None,
        }
    }

    #[test]
    fn system_key_prefixes_classify_as_system() {
        for key in [
            "daemon:tick",
            "cron:nightly",
            "eval-suite",
            "web:fetch",
            "test-run",
            "diagnostic-probe",
            "cross:sync",
        ] {
            assert!(
                is_system_session(&session("syn", key)),
                "{key} should classify as system"
            );
        }
    }

    #[test]
    fn conversational_keys_are_not_system() {
        for key in ["incident-review", "chat", "attest", "daily-notes"] {
            assert!(
                !is_system_session(&session("syn", key)),
                "{key} should not classify as system"
            );
        }
    }

    #[test]
    fn group_by_nous_preserves_order_and_membership() {
        let groups = group_by_nous(vec![
            session("syn", "a"),
            session("ker", "b"),
            session("syn", "c"),
        ]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "syn");
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[1].0, "ker");
        assert_eq!(groups[1].1.len(), 1);
    }
}
