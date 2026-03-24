//! Entity search bar with text search, type filter, confidence filter, and agent filter.

use dioxus::prelude::*;

use crate::state::memory::{EntityListStore, EntityType};

const SEARCH_BAR_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 8px; \
    padding-bottom: 8px;\
";

const SEARCH_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    flex-wrap: wrap;\
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

const FILTER_SELECT_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 8px 12px; \
    color: #e0e0e0; \
    font-size: 13px; \
    cursor: pointer;\
";

const CHIPS_ROW_STYLE: &str = "\
    display: flex; \
    flex-wrap: wrap; \
    gap: 6px;\
";

const CHIP_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 4px; \
    background: #2a2a4a; \
    color: #e0e0e0; \
    border-radius: 12px; \
    padding: 4px 10px; \
    font-size: 12px;\
";

const CHIP_DISMISS_STYLE: &str = "\
    cursor: pointer; \
    color: #888; \
    font-size: 14px; \
    line-height: 1;\
";

const CLEAR_ALL_STYLE: &str = "\
    color: #7a7aff; \
    font-size: 12px; \
    cursor: pointer; \
    background: none; \
    border: none; \
    padding: 4px 8px;\
";

/// Search and filter bar for entity list.
#[component]
pub(crate) fn EntitySearchBar(
    list_store: Signal<EntityListStore>,
    on_search_change: EventHandler<String>,
    on_filter_change: EventHandler<()>,
    on_clear_all: EventHandler<()>,
) -> Element {
    let store = list_store.read();
    let has_filters = store.has_active_filters();
    let search_query = store.search_query.clone();
    let type_filter = store.type_filter.clone();
    let min_confidence = store.min_confidence;
    let agent_filter = store.agent_filter.clone();
    drop(store);

    rsx! {
        div {
            style: "{SEARCH_BAR_STYLE}",
            // Search and filter controls
            div {
                style: "{SEARCH_ROW_STYLE}",
                // Text search with debounce
                input {
                    style: "{SEARCH_INPUT_STYLE}",
                    r#type: "text",
                    placeholder: "Search entities...",
                    value: "{search_query}",
                    oninput: move |evt: Event<FormData>| {
                        let query = evt.value().clone();
                        list_store.write().search_query = query.clone();

                        // WHY: 300ms debounce avoids firing a search on every keystroke.
                        // Each spawn replaces the previous timer conceptually — the last
                        // one to fire wins since it reads the latest query from the store.
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                            on_search_change.call(query);
                        });
                    },
                }

                // Type filter
                select {
                    style: "{FILTER_SELECT_STYLE}",
                    value: "",
                    onchange: move |evt: Event<FormData>| {
                        let label = evt.value();
                        if label.is_empty() {
                            return;
                        }
                        for et in EntityType::FIXED {
                            if et.label() == label {
                                let mut store = list_store.write();
                                if !store.type_filter.contains(et) {
                                    store.type_filter.push(et.clone());
                                }
                                drop(store);
                                on_filter_change.call(());
                                break;
                            }
                        }
                    },
                    option { value: "", "Filter by type..." }
                    for et in EntityType::FIXED {
                        option {
                            value: "{et.label()}",
                            "{et.label()}"
                        }
                    }
                }

                // Confidence filter
                select {
                    style: "{FILTER_SELECT_STYLE}",
                    value: "{min_confidence}",
                    onchange: move |evt: Event<FormData>| {
                        if let Ok(v) = evt.value().parse::<f64>() {
                            list_store.write().min_confidence = v;
                            on_filter_change.call(());
                        }
                    },
                    option { value: "0", "All confidence" }
                    option { value: "0.4", "> 40%" }
                    option { value: "0.7", "> 70%" }
                    option { value: "0.9", "> 90%" }
                }
            }

            // Active filter chips
            if has_filters {
                div {
                    style: "{CHIPS_ROW_STYLE}",
                    // Search query chip
                    if !search_query.is_empty() {
                        div {
                            style: "{CHIP_STYLE}",
                            span { "Search: {search_query}" }
                            span {
                                style: "{CHIP_DISMISS_STYLE}",
                                onclick: move |_| {
                                    list_store.write().search_query.clear();
                                    on_search_change.call(String::new());
                                },
                                "×"
                            }
                        }
                    }
                    // Type filter chips
                    for et in type_filter.iter() {
                        {
                            let label = et.label().to_string();
                            let color = et.color();
                            let et_clone = et.clone();
                            rsx! {
                                div {
                                    key: "type-{label}",
                                    style: "{CHIP_STYLE} border: 1px solid {color}44;",
                                    span { style: "color: {color};", "{label}" }
                                    span {
                                        style: "{CHIP_DISMISS_STYLE}",
                                        onclick: move |_| {
                                            list_store.write().type_filter.retain(|t| t != &et_clone);
                                            on_filter_change.call(());
                                        },
                                        "×"
                                    }
                                }
                            }
                        }
                    }
                    // Confidence chip
                    if min_confidence > 0.0 {
                        {
                            #[expect(clippy::cast_sign_loss, reason = "min_confidence clamped 0.0–1.0")]
                            #[expect(clippy::cast_possible_truncation, reason = "percentage 0–100")]
                            let pct = (min_confidence * 100.0) as u32;
                            rsx! {
                                div {
                                    style: "{CHIP_STYLE}",
                                    span { "Confidence > {pct}%" }
                                    span {
                                        style: "{CHIP_DISMISS_STYLE}",
                                        onclick: move |_| {
                                            list_store.write().min_confidence = 0.0;
                                            on_filter_change.call(());
                                        },
                                        "×"
                                    }
                                }
                            }
                        }
                    }
                    // Agent filter chips
                    for agent in agent_filter.iter() {
                        {
                            let agent_name = agent.clone();
                            let agent_dismiss = agent.clone();
                            rsx! {
                                div {
                                    key: "agent-{agent_name}",
                                    style: "{CHIP_STYLE}",
                                    span { "Agent: {agent_name}" }
                                    span {
                                        style: "{CHIP_DISMISS_STYLE}",
                                        onclick: move |_| {
                                            let dismiss = agent_dismiss.clone();
                                            list_store.write().agent_filter.retain(|a| a != &dismiss);
                                            on_filter_change.call(());
                                        },
                                        "×"
                                    }
                                }
                            }
                        }
                    }
                    // Clear all
                    button {
                        style: "{CLEAR_ALL_STYLE}",
                        onclick: move |_| on_clear_all.call(()),
                        "Clear all"
                    }
                }
            }
        }
    }
}
