//! Entity list component with sort controls and pagination.

use dioxus::prelude::*;

use crate::components::confidence_bar::ConfidenceBar;
use crate::state::memory::{EntityListStore, EntitySort, format_page_rank};
use crate::state::sessions::format_relative_time;

const LIST_CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow: hidden;\
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

const SORT_SELECT_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    color: var(--text-primary); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const SCROLL_AREA_STYLE: &str = "\
    flex: 1; \
    overflow-y: auto; \
    padding: var(--space-1);\
";

const ROW_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-1); \
    padding: var(--space-3) var(--space-3); \
    border-radius: var(--radius-md); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const ROW_SELECTED_BG: &str = "background: var(--border);";

const ROW_HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    gap: var(--space-2);\
";

const ENTITY_NAME_STYLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary); \
    overflow: hidden; \
    text-overflow: ellipsis; \
    white-space: nowrap; \
    flex: 1;\
";

const TYPE_BADGE_STYLE: &str = "\
    font-size: var(--text-xs); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-lg); \
    font-weight: var(--weight-medium); \
    white-space: nowrap;\
";

const STATS_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-3); \
    font-size: var(--text-xs); \
    color: var(--text-secondary);\
";

const FLAG_ICON_STYLE: &str = "\
    color: var(--status-error); \
    font-size: var(--text-xs); \
    margin-left: var(--space-1);\
";

const LOAD_MORE_STYLE: &str = "\
    padding: var(--space-3); \
    text-align: center; \
    color: #7a7aff; \
    cursor: pointer; \
    font-size: var(--text-sm); \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const EMPTY_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: var(--text-muted); \
    font-size: var(--text-base);\
";

const COUNT_STYLE: &str = "\
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    color: var(--text-muted);\
";

/// Entity list panel with sort dropdown, rows, and pagination.
#[component]
pub(crate) fn EntityList(
    list_store: Signal<EntityListStore>,
    selected_id: Option<String>,
    on_select_entity: EventHandler<String>,
    on_sort_change: EventHandler<EntitySort>,
    on_load_more: EventHandler<()>,
) -> Element {
    let store = list_store.read();
    let sort = store.sort;
    let entities = &store.entities;
    let has_more = store.has_more;
    let count = entities.len();

    rsx! {
        div {
            style: "{LIST_CONTAINER_STYLE}",
            // Sort bar
            div {
                style: "{SORT_BAR_STYLE}",
                span { "Sort:" }
                select {
                    style: "{SORT_SELECT_STYLE}",
                    value: "{sort.label()}",
                    onchange: move |evt: Event<FormData>| {
                        let label = evt.value();
                        for s in EntitySort::ALL {
                            if s.label() == label {
                                on_sort_change.call(*s);
                                break;
                            }
                        }
                    },
                    for s in EntitySort::ALL {
                        option {
                            value: "{s.label()}",
                            selected: *s == sort,
                            "{s.label()}"
                        }
                    }
                }
                span {
                    style: "margin-left: auto;",
                    "{count} entities"
                }
            }

            // Scrollable list
            if entities.is_empty() {
                div {
                    style: "{EMPTY_STYLE}",
                    "No entities found"
                }
            } else {
                div {
                    style: "{SCROLL_AREA_STYLE}",
                    for entity in entities.iter() {
                        {
                            let is_selected = selected_id.as_deref() == Some(&entity.id);
                            let bg = if is_selected { ROW_SELECTED_BG } else { "" };
                            let entity_id = entity.id.clone();
                            let type_color = entity.entity_type.color();
                            let type_label = entity.entity_type.label().to_string();
                            let name = entity.name.clone();
                            let confidence = entity.confidence;
                            let page_rank = entity.page_rank;
                            let mem_count = entity.memory_count;
                            let rel_count = entity.relationship_count;
                            let updated = entity.updated_at.clone();
                            let flagged = entity.flagged;

                            rsx! {
                                div {
                                    key: "{entity_id}",
                                    style: "{ROW_STYLE} {bg}",
                                    onmouseenter: move |evt: Event<MouseData>| {
                                        // NOTE: hover effect handled via CSS transition.
                                        let _ = evt;
                                    },
                                    onclick: {
                                        let id = entity_id.clone();
                                        move |_| on_select_entity.call(id.clone())
                                    },
                                    // Row header: name + type badge
                                    div {
                                        style: "{ROW_HEADER_STYLE}",
                                        span {
                                            style: "{ENTITY_NAME_STYLE}",
                                            "{name}"
                                            if flagged {
                                                span { style: "{FLAG_ICON_STYLE}", "⚑" }
                                            }
                                        }
                                        span {
                                            style: "{TYPE_BADGE_STYLE} background: {type_color}22; color: {type_color};",
                                            "{type_label}"
                                        }
                                    }
                                    // Confidence bar
                                    ConfidenceBar { value: confidence, width: "120px" }
                                    // Stats row
                                    div {
                                        style: "{STATS_ROW_STYLE}",
                                        span { "PR: {format_page_rank(page_rank)}" }
                                        span { "{mem_count} memories" }
                                        span { "{rel_count} rels" }
                                        if let Some(ref ts) = updated {
                                            span { "{format_relative_time(ts)}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Pagination
                    if has_more {
                        div {
                            style: "{LOAD_MORE_STYLE}",
                            onclick: move |_| on_load_more.call(()),
                            "Load more..."
                        }
                    }
                    // Count footer
                    div { style: "{COUNT_STYLE}", "{count} entities loaded" }
                }
            }
        }
    }
}
