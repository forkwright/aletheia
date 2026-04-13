//! Entity detail view: properties, relationships, memories, and metadata.

use dioxus::prelude::*;

use crate::components::confidence_bar::ConfidenceBar;
use crate::state::fetch::FetchState;
use crate::state::memory::{
    EntityDetailStore, EntityListStore, confidence_color, format_confidence, format_page_rank,
};
use crate::state::sessions::format_relative_time;
use crate::views::memory::actions::{DeleteDialog, FlagDialog, MergeDialog};

const DETAIL_CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow-y: auto; \
    padding: var(--space-4);\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-3); \
    margin-bottom: var(--space-4); \
    flex-wrap: wrap;\
";

const ENTITY_NAME_STYLE: &str = "\
    font-size: var(--text-xl); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary);\
";

const TYPE_BADGE_STYLE: &str = "\
    font-size: var(--text-xs); \
    padding: var(--space-1) var(--space-3); \
    border-radius: var(--radius-lg); \
    font-weight: var(--weight-medium);\
";

const SCORE_BOX_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    font-size: var(--text-sm); \
    color: var(--text-secondary);\
";

const SECTION_STYLE: &str = "\
    margin-bottom: var(--space-5);\
";

const SECTION_HEADER_STYLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary); \
    margin-bottom: var(--space-2); \
    display: flex; \
    align-items: center; \
    justify-content: space-between;\
";

const CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3);\
";

const PROPERTY_ROW_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    padding: var(--space-2) 0; \
    border-bottom: 1px solid var(--border); \
    font-size: var(--text-sm);\
";

const PROPERTY_KEY_STYLE: &str = "color: var(--text-secondary); font-weight: var(--weight-medium);";
const PROPERTY_VALUE_STYLE: &str = "color: var(--text-primary);";

const REL_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2); \
    border-radius: var(--radius-md); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick); \
    font-size: var(--text-sm);\
";

const REL_TYPE_STYLE: &str = "\
    color: var(--text-secondary); \
    font-size: var(--text-xs); \
    background: var(--border); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-sm);\
";

const REL_ENTITY_STYLE: &str = "\
    color: #7a7aff; \
    font-weight: var(--weight-medium); \
    flex: 1;\
";

const MEMORY_CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    margin-bottom: var(--space-2);\
";

const MEMORY_CONTENT_STYLE: &str = "\
    color: var(--text-primary); \
    font-size: var(--text-sm); \
    line-height: var(--leading-normal); \
    white-space: pre-wrap; \
    overflow: hidden;\
";

const MEMORY_META_STYLE: &str = "\
    display: flex; \
    gap: var(--space-3); \
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    margin-top: var(--space-2);\
";

const EXPAND_BTN_STYLE: &str = "\
    color: #7a7aff; \
    font-size: var(--text-xs); \
    cursor: pointer; \
    background: none; \
    border: none; \
    padding: var(--space-1) 0; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const META_GRID_STYLE: &str = "\
    display: grid; \
    grid-template-columns: 1fr 1fr; \
    gap: var(--space-2);\
";

const META_ITEM_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary);\
";

const META_VALUE_STYLE: &str = "color: var(--text-primary); font-weight: var(--weight-medium);";

const ACTION_BAR_STYLE: &str = "\
    display: flex; \
    gap: var(--space-2); \
    margin-left: auto;\
";

const ACTION_BTN_STYLE: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const ACTION_BTN_DANGER_STYLE: &str = "\
    background: #4a1a1a; \
    color: var(--status-error); \
    border: 1px solid var(--status-error)44; \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const LOADING_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    height: 100%; \
    color: var(--text-secondary); \
    font-size: var(--text-base);\
";

const ERROR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    height: 100%; \
    color: var(--status-error); \
    font-size: var(--text-base);\
";

const CONTENT_PREVIEW_MAX_LINES: usize = 4;

/// Entity detail panel with properties, relationships, memories, and actions.
#[component]
pub(crate) fn EntityDetail(
    detail_state: Signal<FetchState<EntityDetailStore>>,
    list_store: Signal<EntityListStore>,
    on_navigate_entity: EventHandler<String>,
    on_entity_changed: EventHandler<()>,
) -> Element {
    let mut show_merge = use_signal(|| false);
    let mut show_flag = use_signal(|| false);
    let mut show_delete = use_signal(|| false);
    let expanded_memories = use_signal(Vec::<String>::new);

    match &*detail_state.read() {
        FetchState::Loading => rsx! {
            div { style: "{LOADING_STYLE}", "Loading entity..." }
        },
        FetchState::Error(err) => rsx! {
            div { style: "{ERROR_STYLE}", "Error: {err}" }
        },
        FetchState::Loaded(detail) => {
            let Some(ref entity) = detail.entity else {
                return rsx! {
                    div { style: "{LOADING_STYLE}", "No entity data" }
                };
            };

            let entity_id = entity.id.clone();
            let entity_name = entity.name.clone();
            let type_label = entity.entity_type.label().to_string();
            let type_color = entity.entity_type.color();
            let confidence = entity.confidence;
            let page_rank = entity.page_rank;
            let properties = entity.properties.clone();
            let flagged = entity.flagged;
            let created_by = entity.created_by.clone();
            let created_at = entity.created_at.clone();
            let updated_at = entity.updated_at.clone();
            let relationships = detail.relationships.clone();
            let memories = detail.memories.clone();
            let rel_count = relationships.len();
            let mem_count = memories.len();

            rsx! {
                div {
                    style: "{DETAIL_CONTAINER_STYLE}",
                    // Header
                    div {
                        style: "{HEADER_STYLE}",
                        span { style: "{ENTITY_NAME_STYLE}",
                            "{entity_name}"
                            if flagged {
                                span { style: "color: var(--status-error); margin-left: var(--space-2);", "⚑" }
                            }
                        }
                        span {
                            style: "{TYPE_BADGE_STYLE} background: {type_color}22; color: {type_color};",
                            "{type_label}"
                        }
                        div {
                            style: "{SCORE_BOX_STYLE}",
                            span { "Confidence:" }
                            ConfidenceBar { value: confidence, width: "100px" }
                        }
                        div {
                            style: "{SCORE_BOX_STYLE}",
                            span { "PageRank:" }
                            span { style: "color: var(--text-primary); font-weight: var(--weight-semibold);",
                                "{format_page_rank(page_rank)}"
                            }
                        }
                        // Action buttons
                        div {
                            style: "{ACTION_BAR_STYLE}",
                            button {
                                style: "{ACTION_BTN_STYLE}",
                                onclick: move |_| show_merge.set(true),
                                "Merge"
                            }
                            button {
                                style: "{ACTION_BTN_STYLE}",
                                onclick: move |_| show_flag.set(true),
                                "Flag"
                            }
                            button {
                                style: "{ACTION_BTN_DANGER_STYLE}",
                                onclick: move |_| show_delete.set(true),
                                "Delete"
                            }
                        }
                    }

                    // Properties
                    if !properties.is_empty() {
                        div {
                            style: "{SECTION_STYLE}",
                            div { style: "{SECTION_HEADER_STYLE}", "Properties" }
                            div {
                                style: "{CARD_STYLE}",
                                for prop in properties.iter() {
                                    div {
                                        key: "prop-{prop.key}",
                                        style: "{PROPERTY_ROW_STYLE}",
                                        span { style: "{PROPERTY_KEY_STYLE}", "{prop.key}" }
                                        span { style: "{PROPERTY_VALUE_STYLE}", "{prop.value}" }
                                    }
                                }
                            }
                        }
                    }

                    // Relationships
                    div {
                        style: "{SECTION_STYLE}",
                        div {
                            style: "{SECTION_HEADER_STYLE}",
                            span { "Relationships ({rel_count})" }
                        }
                        if relationships.is_empty() {
                            div {
                                style: "{CARD_STYLE} color: var(--text-muted); font-size: var(--text-sm);",
                                "No relationships"
                            }
                        } else {
                            div {
                                style: "{CARD_STYLE}",
                                for rel in relationships.iter() {
                                    {
                                        let direction = rel.direction.arrow();
                                        let rel_type = rel.relationship_type.clone();
                                        let entity_name = rel.entity_name.clone();
                                        let entity_id = rel.entity_id.clone();
                                        let conf = rel.confidence;
                                        let conf_color = confidence_color(conf);
                                        let rel_id = rel.id.clone();

                                        rsx! {
                                            div {
                                                key: "rel-{rel_id}",
                                                style: "{REL_ROW_STYLE}",
                                                onclick: {
                                                    let id = entity_id.clone();
                                                    move |_| on_navigate_entity.call(id.clone())
                                                },
                                                span { style: "color: var(--text-muted); font-size: var(--text-base);", "{direction}" }
                                                span { style: "{REL_TYPE_STYLE}", "{rel_type}" }
                                                span { style: "{REL_ENTITY_STYLE}", "{entity_name}" }
                                                span {
                                                    style: "font-size: var(--text-xs); color: {conf_color};",
                                                    "{format_confidence(conf)}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Memories
                    div {
                        style: "{SECTION_STYLE}",
                        div {
                            style: "{SECTION_HEADER_STYLE}",
                            span { "Memories ({mem_count})" }
                        }
                        if memories.is_empty() {
                            div {
                                style: "{CARD_STYLE} color: var(--text-muted); font-size: var(--text-sm);",
                                "No memories"
                            }
                        } else {
                            for mem in memories.iter() {
                                {
                                    let is_expanded = expanded_memories.read().contains(&mem.id);
                                    let lines: Vec<&str> = mem.content.lines().collect();
                                    let needs_expand = lines.len() > CONTENT_PREVIEW_MAX_LINES;
                                    let display_content = if is_expanded || !needs_expand {
                                        mem.content.clone()
                                    } else {
                                        lines.get(..CONTENT_PREVIEW_MAX_LINES).unwrap_or(&lines).join("\n")
                                    };
                                    let mem_id = mem.id.clone();
                                    let agent = mem.agent.clone();
                                    let session = mem.session.clone();
                                    let created_at = mem.created_at.clone();
                                    let conf = mem.confidence;
                                    let conf_color = confidence_color(conf);

                                    rsx! {
                                        div {
                                            key: "mem-{mem_id}",
                                            style: "{MEMORY_CARD_STYLE}",
                                            div {
                                                style: "{MEMORY_CONTENT_STYLE}",
                                                "{display_content}"
                                            }
                                            if needs_expand {
                                                button {
                                                    style: "{EXPAND_BTN_STYLE}",
                                                    onclick: {
                                                        let id = mem_id.clone();
                                                        let mut expanded = expanded_memories;
                                                        move |_| {
                                                            let id = id.clone();
                                                            let mut exp = expanded.write();
                                                            if let Some(pos) = exp.iter().position(|x| x == &id) {
                                                                exp.remove(pos);
                                                            } else {
                                                                exp.push(id);
                                                            }
                                                        }
                                                    },
                                                    if is_expanded { "Show less" } else { "Show more" }
                                                }
                                            }
                                            div {
                                                style: "{MEMORY_META_STYLE}",
                                                span {
                                                    style: "color: {conf_color};",
                                                    "{format_confidence(conf)}"
                                                }
                                                if let Some(ref a) = agent {
                                                    span { "agent: {a}" }
                                                }
                                                if let Some(ref s) = session {
                                                    span { "session: {s}" }
                                                }
                                                if let Some(ref ts) = created_at {
                                                    span { "{format_relative_time(ts)}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Metadata
                    div {
                        style: "{SECTION_STYLE}",
                        div { style: "{SECTION_HEADER_STYLE}", "Metadata" }
                        div {
                            style: "{CARD_STYLE}",
                            div {
                                style: "{META_GRID_STYLE}",
                                div {
                                    style: "{META_ITEM_STYLE}",
                                    "ID: "
                                    span { style: "{META_VALUE_STYLE}", "{entity_id}" }
                                }
                                if let Some(ref agent) = created_by {
                                    div {
                                        style: "{META_ITEM_STYLE}",
                                        "Created by: "
                                        span { style: "{META_VALUE_STYLE}", "{agent}" }
                                    }
                                }
                                if let Some(ref ts) = created_at {
                                    div {
                                        style: "{META_ITEM_STYLE}",
                                        "Created: "
                                        span { style: "{META_VALUE_STYLE}", "{format_relative_time(ts)}" }
                                    }
                                }
                                if let Some(ref ts) = updated_at {
                                    div {
                                        style: "{META_ITEM_STYLE}",
                                        "Updated: "
                                        span { style: "{META_VALUE_STYLE}", "{format_relative_time(ts)}" }
                                    }
                                }
                            }
                        }
                    }

                    // Action dialogs
                    if *show_merge.read() {
                        MergeDialog {
                            entity_id: entity_id.clone(),
                            entity_name: entity_name.clone(),
                            list_store,
                            on_close: move |_| show_merge.set(false),
                            on_merged: move |_| {
                                show_merge.set(false);
                                on_entity_changed.call(());
                            },
                        }
                    }
                    if *show_flag.read() {
                        FlagDialog {
                            entity_id: entity_id.clone(),
                            entity_name: entity_name.clone(),
                            on_close: move |_| show_flag.set(false),
                            on_flagged: move |_| {
                                show_flag.set(false);
                                on_entity_changed.call(());
                            },
                        }
                    }
                    if *show_delete.read() {
                        DeleteDialog {
                            entity_id: entity_id.clone(),
                            entity_name: entity_name.clone(),
                            relationship_count: rel_count,
                            memory_count: mem_count,
                            on_close: move |_| show_delete.set(false),
                            on_deleted: move |_| {
                                show_delete.set(false);
                                on_entity_changed.call(());
                            },
                        }
                    }
                }
            }
        }
    }
}
