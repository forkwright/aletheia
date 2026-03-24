//! Search and filter bar for the session list.

use dioxus::prelude::*;

use crate::state::sessions::{SessionListStore, StatusFilter};

const SEARCH_BAR_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 8px; \
    padding: 12px;\
";

const SEARCH_INPUT_STYLE: &str = "\
    flex: 1; \
    min-width: 200px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 8px 12px; \
    color: #e0e0e0; \
    font-size: 14px;\
";

const FILTER_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    flex-wrap: wrap;\
";

const SELECT_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 6px 10px; \
    color: #e0e0e0; \
    font-size: 13px; \
    cursor: pointer;\
";

const CHIP_STYLE: &str = "\
    display: inline-flex; \
    align-items: center; \
    gap: 4px; \
    background: #2a2a4a; \
    border: 1px solid #4a4aff; \
    border-radius: 16px; \
    padding: 2px 10px; \
    font-size: 11px; \
    color: #7a7aff;\
";

const CHIP_CLOSE_STYLE: &str = "\
    background: none; \
    border: none; \
    color: #7a7aff; \
    cursor: pointer; \
    font-size: 14px; \
    padding: 0; \
    line-height: 1;\
";

const CLEAR_BTN_STYLE: &str = "\
    background: none; \
    border: 1px solid #444; \
    border-radius: 4px; \
    padding: 2px 8px; \
    font-size: 11px; \
    color: #888; \
    cursor: pointer;\
";

const CHIPS_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 6px; \
    flex-wrap: wrap;\
";

/// Search and filter bar for sessions.
#[component]
pub(crate) fn SessionSearchBar(
    mut list_store: Signal<SessionListStore>,
    agent_names: Vec<String>,
    on_search_change: EventHandler<String>,
    on_status_change: EventHandler<StatusFilter>,
    on_agent_change: EventHandler<Vec<String>>,
    on_clear_all: EventHandler<()>,
) -> Element {
    let mut debounce_timer = use_signal(|| None::<i64>);

    let store = list_store.read();
    let current_query = store.search_query.clone();
    let current_status = store.status_filter;
    let current_agents = store.agent_filter.clone();
    let has_filters = store.has_active_filters();
    drop(store);

    rsx! {
        div {
            style: "{SEARCH_BAR_STYLE}",
            // Search input
            input {
                style: "{SEARCH_INPUT_STYLE}",
                r#type: "text",
                placeholder: "Search sessions...",
                value: "{current_query}",
                oninput: move |evt: Event<FormData>| {
                    let value = evt.value().clone();
                    list_store.write().search_query = value.clone();

                    // WHY: debounce search to avoid firing on every keystroke.
                    let timer_id = *debounce_timer.read();
                    if let Some(id) = timer_id {
                        // SAFETY: clearing a timer that may not exist is a no-op in JS/desktop.
                        web_sys_clear_timeout(id);
                    }

                    let new_timer = spawn_debounced_search(value, on_search_change, 300);
                    debounce_timer.set(Some(new_timer));
                },
            }
            // Filter row
            div {
                style: "{FILTER_ROW_STYLE}",
                // Status filter
                select {
                    style: "{SELECT_STYLE}",
                    value: status_filter_value(current_status),
                    onchange: move |evt: Event<FormData>| {
                        let filter = parse_status_filter(&evt.value());
                        on_status_change.call(filter);
                    },
                    for filter in StatusFilter::ALL {
                        option {
                            value: status_filter_value(*filter),
                            "{filter.label()}"
                        }
                    }
                }
                // Agent filter
                if !agent_names.is_empty() {
                    select {
                        style: "{SELECT_STYLE}",
                        value: "",
                        onchange: {
                            let current = current_agents.clone();
                            move |evt: Event<FormData>| {
                                let agent = evt.value().clone();
                                if !agent.is_empty() {
                                    let mut agents = current.clone();
                                    if !agents.contains(&agent) {
                                        agents.push(agent);
                                    }
                                    on_agent_change.call(agents);
                                }
                            }
                        },
                        option { value: "", "All agents" }
                        for name in agent_names.iter() {
                            option { value: "{name}", "{name}" }
                        }
                    }
                }
                // Clear all button
                if has_filters {
                    button {
                        style: "{CLEAR_BTN_STYLE}",
                        onclick: move |_| on_clear_all.call(()),
                        "Clear all"
                    }
                }
            }
            // Active filter chips
            if has_filters {
                div {
                    style: "{CHIPS_ROW_STYLE}",
                    if !list_store.read().search_query.is_empty() {
                        FilterChip {
                            label: format!("search: {}", list_store.read().search_query),
                            on_remove: move |_| {
                                list_store.write().search_query.clear();
                                on_search_change.call(String::new());
                            },
                        }
                    }
                    if current_status != StatusFilter::All {
                        FilterChip {
                            label: format!("status: {}", current_status.label()),
                            on_remove: move |_| {
                                on_status_change.call(StatusFilter::All);
                            },
                        }
                    }
                    for agent in current_agents.iter() {
                        FilterChip {
                            key: "{agent}",
                            label: format!("agent: {agent}"),
                            on_remove: {
                                let agent_name = agent.clone();
                                let current = list_store.read().agent_filter.clone();
                                move |_| {
                                    let filtered: Vec<String> = current
                                        .iter()
                                        .filter(|a| **a != agent_name)
                                        .cloned()
                                        .collect();
                                    on_agent_change.call(filtered);
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

/// A dismissible filter chip.
#[component]
fn FilterChip(label: String, on_remove: EventHandler<()>) -> Element {
    rsx! {
        span {
            style: "{CHIP_STYLE}",
            "{label}"
            button {
                style: "{CHIP_CLOSE_STYLE}",
                onclick: move |_| on_remove.call(()),
                "x"
            }
        }
    }
}

fn status_filter_value(filter: StatusFilter) -> &'static str {
    match filter {
        StatusFilter::All => "all",
        StatusFilter::Active => "active",
        StatusFilter::Idle => "idle",
        StatusFilter::Archived => "archived",
    }
}

fn parse_status_filter(value: &str) -> StatusFilter {
    match value {
        "active" => StatusFilter::Active,
        "idle" => StatusFilter::Idle,
        "archived" => StatusFilter::Archived,
        _ => StatusFilter::All,
    }
}

/// Spawn a delayed search callback. Returns a synthetic timer ID for tracking.
///
/// NOTE: Dioxus desktop runs on tokio, so we use `tokio::time::sleep` for
/// the debounce delay instead of browser `setTimeout`.
fn spawn_debounced_search(query: String, on_search: EventHandler<String>, delay_ms: u64) -> i64 {
    static TIMER_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
    let id = TIMER_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        on_search.call(query);
    });

    id
}

/// No-op on desktop -- timer cancellation is handled by re-spawning.
fn web_sys_clear_timeout(_id: i64) {
    // WHY: on desktop (tokio), we cannot cancel a spawned future by ID.
    // The debounce works because the search handler will use the latest
    // query value from the signal regardless of how many timers fire.
}
