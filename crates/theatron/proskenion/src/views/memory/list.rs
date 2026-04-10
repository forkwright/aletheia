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
    gap: 8px; \
    padding: 8px 12px; \
    border-bottom: 1px solid #2a2a3a; \
    font-size: 12px; \
    color: #888;\
";

const SORT_SELECT_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 4px 8px; \
    color: #e0e0e0; \
    font-size: 12px; \
    cursor: pointer;\
";

const SCROLL_AREA_STYLE: &str = "\
    flex: 1; \
    overflow-y: auto; \
    padding: 4px;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 4px; \
    padding: 10px 12px; \
    border-radius: 6px; \
    cursor: pointer; \
    transition: background 0.1s;\
";

const ROW_SELECTED_BG: &str = "background: #2a2a4a;";

const ROW_HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    gap: 8px;\
";

const ENTITY_NAME_STYLE: &str = "\
    font-size: 14px; \
    font-weight: 600; \
    color: #e0e0e0; \
    overflow: hidden; \
    text-overflow: ellipsis; \
    white-space: nowrap; \
    flex: 1;\
";

const TYPE_BADGE_STYLE: &str = "\
    font-size: 11px; \
    padding: 2px 8px; \
    border-radius: 10px; \
    font-weight: 500; \
    white-space: nowrap;\
";

const STATS_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    font-size: 11px; \
    color: #888;\
";

const FLAG_ICON_STYLE: &str = "\
    color: #ef4444; \
    font-size: 12px; \
    margin-left: 4px;\
";

const LOAD_MORE_STYLE: &str = "\
    padding: 12px; \
    text-align: center; \
    color: #7a7aff; \
    cursor: pointer; \
    font-size: 13px;\
";

const EMPTY_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: #555; \
    font-size: 14px;\
";

const COUNT_STYLE: &str = "\
    padding: 4px 12px; \
    font-size: 11px; \
    color: #666;\
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
