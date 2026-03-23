//! Memory explorer: master-detail entity browser with search, filtering, and actions.

pub(crate) mod actions;
pub(crate) mod detail;
pub(crate) mod list;
pub(crate) mod search;

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::memory::{
    Entity, EntityDetailStore, EntityListStore, EntityMemory, EntityNavigationHistory, EntitySort,
    Relationship,
};
use crate::views::memory::detail::EntityDetail;
use crate::views::memory::list::EntityList;
use crate::views::memory::search::EntitySearchBar;

const MEMORY_LAYOUT_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: 12px;\
";

const PANELS_STYLE: &str = "\
    display: flex; \
    flex: 1; \
    overflow: hidden; \
    gap: 0;\
";

const LIST_PANEL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    overflow: hidden; \
    flex-shrink: 0;\
";

const RESIZE_HANDLE_STYLE: &str = "\
    width: 4px; \
    cursor: col-resize; \
    background: transparent; \
    flex-shrink: 0; \
    transition: background 0.15s;\
";

const DETAIL_PANEL_STYLE: &str = "\
    flex: 1; \
    overflow: hidden;\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding-bottom: 8px;\
";

const REFRESH_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const BREADCRUMB_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 4px; \
    font-size: 12px; \
    color: #888; \
    padding: 4px 0; \
    flex-wrap: wrap;\
";

const BREADCRUMB_LINK_STYLE: &str = "\
    color: #7a7aff; \
    cursor: pointer; \
    text-decoration: none;\
";

const NAV_BTN_STYLE: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 4px; \
    padding: 2px 8px; \
    font-size: 12px; \
    cursor: pointer;\
";

const NAV_BTN_DISABLED_STYLE: &str = "\
    background: #1a1a2e; \
    color: #555; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 2px 8px; \
    font-size: 12px; \
    cursor: default;\
";

const EMPTY_DETAIL_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    height: 100%; \
    color: #555; \
    font-size: 14px;\
";

const DEFAULT_LIST_WIDTH: f64 = 480.0;
const MIN_LIST_WIDTH: f64 = 280.0;
const MAX_LIST_WIDTH: f64 = 800.0;

/// Top-level memory explorer component.
#[component]
pub(crate) fn Memory() -> Element {
    let config: Signal<ConnectionConfig> = use_context();

    let mut list_store = use_signal(EntityListStore::new);
    let mut detail_state =
        use_signal(|| FetchState::<EntityDetailStore>::Loaded(EntityDetailStore::default()));
    let mut nav_history = use_signal(EntityNavigationHistory::new);
    let mut selected_entity_id: Signal<Option<String>> = use_signal(|| None);

    let mut list_width = use_signal(|| DEFAULT_LIST_WIDTH);
    let mut is_resizing = use_signal(|| false);
    let mut resize_start_x = use_signal(|| 0.0f64);
    let mut resize_start_width = use_signal(|| 0.0f64);

    let fetch_entities = {
        move || {
            let cfg = config.read().clone();
            let store = list_store.read();
            let search = store.search_query.clone();
            let sort = store.sort;
            let type_filter = store.type_filter.clone();
            let min_confidence = store.min_confidence;
            let agent_filter = store.agent_filter.clone();
            let page = store.page;
            drop(store);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');

                let mut url = format!(
                    "{base}/api/v1/knowledge/entities?limit={}",
                    EntityListStore::PAGE_SIZE
                );

                if page > 0 {
                    url.push_str(&format!("&offset={}", page * EntityListStore::PAGE_SIZE));
                }

                if !search.is_empty() {
                    let encoded: String =
                        form_urlencoded::byte_serialize(search.as_bytes()).collect();
                    url.push_str(&format!("&q={encoded}"));
                }

                let sort_param = match sort {
                    EntitySort::PageRank => "page_rank",
                    EntitySort::Confidence => "confidence",
                    EntitySort::MemoryCount => "memory_count",
                    EntitySort::LastUpdated => "updated_at",
                    EntitySort::Alphabetical => "name",
                };
                url.push_str(&format!("&sort={sort_param}&order=desc"));

                if sort == EntitySort::Alphabetical {
                    // NOTE: alphabetical sorts ascending.
                    url.truncate(url.len() - 4);
                    url.push_str("asc");
                }

                for et in &type_filter {
                    let encoded: String =
                        form_urlencoded::byte_serialize(et.label().as_bytes()).collect();
                    url.push_str(&format!("&entity_type={encoded}"));
                }

                if min_confidence > 0.0 {
                    url.push_str(&format!("&min_confidence={min_confidence}"));
                }

                for agent in &agent_filter {
                    let encoded: String =
                        form_urlencoded::byte_serialize(agent.as_bytes()).collect();
                    url.push_str(&format!("&agent={encoded}"));
                }

                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let text = match resp.text().await {
                            Ok(t) => t,
                            Err(e) => {
                                tracing::warn!("failed to read entities response: {e}");
                                return;
                            }
                        };

                        let entities: Vec<Entity> = if let Ok(list) =
                            serde_json::from_str::<Vec<Entity>>(&text)
                        {
                            list
                        } else if let Ok(wrapper) = serde_json::from_str::<EntitiesResponse>(&text)
                        {
                            wrapper.entities
                        } else {
                            tracing::warn!("failed to parse entities response");
                            return;
                        };

                        let has_more = entities.len() >= EntityListStore::PAGE_SIZE;

                        let mut store = list_store.write();
                        if page == 0 {
                            store.load(entities, has_more);
                        } else {
                            store.append(entities, has_more);
                        }
                        store.sort_entities();
                    }
                    Ok(resp) => {
                        tracing::warn!("entities request failed: {}", resp.status());
                    }
                    Err(e) => {
                        tracing::warn!("entities connection error: {e}");
                    }
                }
            });
        }
    };

    use_effect(move || {
        fetch_entities();
    });

    let fetch_detail = {
        move |entity_id: String| {
            let cfg = config.read().clone();
            let id = entity_id.clone();
            detail_state.set(FetchState::Loading);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String = form_urlencoded::byte_serialize(id.as_bytes()).collect();

                // Fetch entity detail, relationships, and memories in parallel.
                let entity_url = format!("{base}/api/v1/knowledge/entities/{encoded}");
                let rels_url = format!("{base}/api/v1/knowledge/entities/{encoded}/relationships");
                let mems_url = format!("{base}/api/v1/knowledge/entities/{encoded}/memories");

                let entity_fut = client.get(&entity_url).send();
                let rels_fut = client.get(&rels_url).send();
                let mems_fut = client.get(&mems_url).send();

                let (entity_res, rels_res, mems_res) = tokio::join!(entity_fut, rels_fut, mems_fut);

                let entity: Option<Entity> = entity_res
                    .ok()
                    .filter(|r| r.status().is_success())
                    .and_then(|r| futures_util::FutureExt::now_or_never(r.json::<Entity>()))
                    .and_then(|r| r.ok());

                // WHY: If the entity fetch failed, still try to show what we can.
                // Fall back to finding the entity in the list store if the detail
                // endpoint returned an error.

                let relationships: Vec<Relationship> = rels_res
                    .ok()
                    .filter(|r| r.status().is_success())
                    .and_then(|r| {
                        futures_util::FutureExt::now_or_never(r.json::<Vec<Relationship>>())
                    })
                    .and_then(|r| r.ok())
                    .unwrap_or_default();

                let memories: Vec<EntityMemory> = mems_res
                    .ok()
                    .filter(|r| r.status().is_success())
                    .and_then(|r| {
                        futures_util::FutureExt::now_or_never(r.json::<Vec<EntityMemory>>())
                    })
                    .and_then(|r| r.ok())
                    .unwrap_or_default();

                let detail = EntityDetailStore {
                    entity,
                    relationships,
                    memories,
                };

                detail_state.set(FetchState::Loaded(detail));
            });
        }
    };

    let navigate_to_entity = {
        let mut fetch_detail = fetch_detail;
        move |entity_id: String| {
            nav_history.write().push(entity_id.clone());
            selected_entity_id.set(Some(entity_id.clone()));
            fetch_detail(entity_id);
        }
    };

    let width = *list_width.read();
    let can_back = nav_history.read().can_go_back();
    let can_forward = nav_history.read().can_go_forward();
    let breadcrumbs: Vec<String> = nav_history.read().breadcrumbs().to_vec();

    rsx! {
        div {
            style: "{MEMORY_LAYOUT_STYLE}",
            // Header
            div {
                style: "{HEADER_STYLE}",
                h2 {
                    style: "font-size: 18px; margin: 0; color: #e0e0e0;",
                    "Memory Explorer"
                }
                div {
                    style: "display: flex; gap: 8px; align-items: center;",
                    // Back/forward navigation
                    button {
                        style: if can_back { "{NAV_BTN_STYLE}" } else { "{NAV_BTN_DISABLED_STYLE}" },
                        disabled: !can_back,
                        onclick: {
                            let mut fetch_detail = fetch_detail;
                            move |_| {
                                let id = nav_history.write().back().map(String::from);
                                if let Some(id) = id {
                                    selected_entity_id.set(Some(id.clone()));
                                    fetch_detail(id);
                                }
                            }
                        },
                        "←"
                    }
                    button {
                        style: if can_forward { "{NAV_BTN_STYLE}" } else { "{NAV_BTN_DISABLED_STYLE}" },
                        disabled: !can_forward,
                        onclick: {
                            let mut fetch_detail = fetch_detail;
                            move |_| {
                                let id = nav_history.write().forward().map(String::from);
                                if let Some(id) = id {
                                    selected_entity_id.set(Some(id.clone()));
                                    fetch_detail(id);
                                }
                            }
                        },
                        "→"
                    }
                    button {
                        style: "{REFRESH_BTN}",
                        onclick: move |_| {
                            list_store.write().page = 0;
                            fetch_entities();
                        },
                        "Refresh"
                    }
                }
            }

            // Breadcrumbs
            if breadcrumbs.len() > 1 {
                div {
                    style: "{BREADCRUMB_STYLE}",
                    span { "Memory" }
                    for (i, crumb) in breadcrumbs.iter().enumerate() {
                        span { " › " }
                        if i < breadcrumbs.len() - 1 {
                            span {
                                style: "{BREADCRUMB_LINK_STYLE}",
                                onclick: {
                                    let entity_id = crumb.clone();
                                    let mut fetch_detail = fetch_detail;
                                    move |_| {
                                        let entity_id = entity_id.clone();
                                        selected_entity_id.set(Some(entity_id.clone()));
                                        fetch_detail(entity_id);
                                    }
                                },
                                "{crumb}"
                            }
                        } else {
                            span {
                                style: "color: #e0e0e0;",
                                "{crumb}"
                            }
                        }
                    }
                }
            }

            // Search bar
            EntitySearchBar {
                list_store,
                on_search_change: move |_query: String| {
                    list_store.write().page = 0;
                    fetch_entities();
                },
                on_filter_change: move |_| {
                    list_store.write().page = 0;
                    fetch_entities();
                },
                on_clear_all: move |_| {
                    list_store.write().clear_filters();
                    fetch_entities();
                },
            }

            // Two-panel layout
            div {
                style: "{PANELS_STYLE}",
                onmousemove: move |evt: Event<MouseData>| {
                    if *is_resizing.read() {
                        let delta = evt.client_coordinates().x - *resize_start_x.read();
                        let new_width = (*resize_start_width.read() + delta)
                            .clamp(MIN_LIST_WIDTH, MAX_LIST_WIDTH);
                        list_width.set(new_width);
                    }
                },
                onmouseup: move |_| {
                    is_resizing.set(false);
                },
                // List panel
                div {
                    style: "{LIST_PANEL_STYLE} width: {width}px;",
                    EntityList {
                        list_store,
                        selected_id: selected_entity_id.read().clone(),
                        on_select_entity: {
                            let mut navigate = navigate_to_entity;
                            move |id: String| {
                                navigate(id);
                            }
                        },
                        on_sort_change: move |sort: EntitySort| {
                            let mut store = list_store.write();
                            store.sort = sort;
                            store.sort_entities();
                        },
                        on_load_more: move |_| {
                            list_store.write().page += 1;
                            fetch_entities();
                        },
                    }
                }
                // Resize handle
                div {
                    style: "{RESIZE_HANDLE_STYLE}",
                    onmousedown: move |evt: Event<MouseData>| {
                        is_resizing.set(true);
                        resize_start_x.set(evt.client_coordinates().x);
                        resize_start_width.set(*list_width.read());
                    },
                }
                // Detail panel
                div {
                    style: "{DETAIL_PANEL_STYLE}",
                    if selected_entity_id.read().is_some() {
                        EntityDetail {
                            detail_state,
                            list_store,
                            on_navigate_entity: {
                                let mut navigate = navigate_to_entity;
                                move |id: String| {
                                    navigate(id);
                                }
                            },
                            on_entity_changed: move |_| {
                                // WHY: Refresh list after merge/delete/flag to reflect changes.
                                list_store.write().page = 0;
                                fetch_entities();
                            },
                        }
                    } else {
                        div {
                            style: "{EMPTY_DETAIL_STYLE}",
                            "Select an entity to view details"
                        }
                    }
                }
            }
        }
    }
}

/// Response wrapper for entity list endpoint.
#[derive(Debug, serde::Deserialize)]
struct EntitiesResponse {
    entities: Vec<Entity>,
}
