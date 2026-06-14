//! Memory explorer: a readable list of what the agent remembers (facts) as the
//! default surface, with the entity knowledge graph demoted to an opt-in lens.

pub(crate) mod actions;
pub(crate) mod curation;
pub(crate) mod detail;
pub(crate) mod fact_filters;
pub(crate) mod fact_list;
pub(crate) mod health_strip;
pub(crate) mod list;
pub(crate) mod search;

use dioxus::prelude::*;

use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::memory::{
    Entity, EntityDetailStore, EntityListStore, EntityMemory, EntityNavigationHistory, EntitySort,
    Fact, FactListData, FactListError, FactListErrorKind, FactListState, FactListStore, FactSort,
    Relationship,
};
use skene::ApiClient;
use skene::api::types as skene_types;
use crate::state::view_preservation::{PreservedViewState, ViewKey, ViewPreservationStore};
use crate::views::memory::detail::EntityDetail;
use crate::views::memory::fact_filters::FactFilters;
use crate::views::memory::fact_list::FactList;
use crate::views::memory::health_strip::HealthStrip;
use crate::views::memory::list::EntityList;
use crate::views::memory::search::EntitySearchBar;

const MEMORY_LAYOUT_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: var(--space-3);\
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
    flex-shrink: 0;\
";

const DETAIL_PANEL_STYLE: &str = "\
    flex: 1; \
    overflow: hidden;\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding-bottom: var(--space-2);\
";

const REFRESH_BTN: &str = "\
    background: var(--bg-surface); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const TABS_STYLE: &str = "\
    display: flex; \
    gap: var(--space-1); \
    background: var(--bg-surface-dim); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: 2px;\
";

const TAB_BTN_STYLE: &str = "\
    background: none; \
    border: none; \
    border-radius: var(--radius-sm); \
    color: var(--text-secondary); \
    font-size: var(--text-sm); \
    padding: var(--space-1) var(--space-3); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick);\
";

const TAB_BTN_ACTIVE_STYLE: &str = "\
    background: var(--bg-surface); \
    border: none; \
    border-radius: var(--radius-sm); \
    color: var(--text-primary); \
    font-size: var(--text-sm); \
    font-weight: var(--weight-medium); \
    padding: var(--space-1) var(--space-3); \
    cursor: pointer;\
";

const BREADCRUMB_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-1); \
    font-size: var(--text-sm); \
    color: var(--text-muted); \
    padding: var(--space-1) 0; \
    flex-wrap: wrap;\
";

const BREADCRUMB_LINK_STYLE: &str = "\
    color: var(--accent); \
    cursor: pointer; \
    text-decoration: none; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const NAV_BTN_STYLE: &str = "\
    background: var(--bg-surface); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const NAV_BTN_DISABLED_STYLE: &str = "\
    background: var(--bg-surface-dim); \
    color: var(--text-muted); \
    border: 1px solid var(--border-separator); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-sm); \
    cursor: default;\
";

const EMPTY_DETAIL_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    height: 100%; \
    color: var(--text-muted); \
    font-size: var(--text-base);\
";

const GRAPH_EMPTY_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    gap: var(--space-2); \
    flex: 1; \
    color: var(--text-muted); \
    font-size: var(--text-base); \
    text-align: center; \
    padding: var(--space-6);\
";

/// Build an [`ApiClient`] from the desktop connection config.
///
/// WHY: centralises the narrow boundary where raw config becomes a typed
/// first-party client; call sites handle the error uniformly.
pub(crate) fn memory_client(cfg: &ConnectionConfig) -> Option<ApiClient> {
    ApiClient::new(&cfg.server_url, cfg.auth_token.clone()).ok()
}
const DEFAULT_LIST_WIDTH: f64 = 480.0;
const MIN_LIST_WIDTH: f64 = 280.0;
const MAX_LIST_WIDTH: f64 = 800.0;

/// Active surface within the memory view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryTab {
    /// Facts list — the default front door.
    Facts,
    /// Entity knowledge graph — opt-in advanced lens.
    Graph,
}

/// Top-level memory explorer component.
#[component]
pub(crate) fn Memory() -> Element {
    let config: Signal<ConnectionConfig> = use_context();

    let mut tab = use_signal(|| MemoryTab::Facts);

    // ── Fact surface state ──
    let mut fact_store = use_signal(FactListStore::default);

    // ── Entity (graph) surface state ──
    let mut list_store = use_signal(EntityListStore::new);
    let mut detail_state =
        use_signal(|| FetchState::<EntityDetailStore>::Loaded(EntityDetailStore::default()));
    let mut nav_history = use_signal(EntityNavigationHistory::new);
    let mut selected_entity_id: Signal<Option<String>> = use_signal(|| None);

    let mut list_width = use_signal(|| DEFAULT_LIST_WIDTH);
    let mut is_resizing = use_signal(|| false);
    let mut resize_start_x = use_signal(|| 0.0f64);
    let mut resize_start_width = use_signal(|| 0.0f64);

    // WHY: Restore preserved search state on mount (#2411).
    let mut preservation = use_context::<Signal<ViewPreservationStore>>();
    use_hook(|| {
        if let Some(saved) = preservation.write().restore(&ViewKey::Memory)
            && !saved.input_text.is_empty()
        {
            fact_store.write().search_query = saved.input_text;
        }
    });

    use_drop(move || {
        preservation.write().save(
            ViewKey::Memory,
            PreservedViewState {
                scroll_top: 0.0,
                input_text: fact_store.read().search_query.clone(),
                secondary_scroll: 0.0,
            },
        );
    });

    let fetch_facts = move || {
        let cfg = config.read().clone();
        let store = fact_store.read();
        let search = store.search_query.clone();
        let sort = store.sort;
        let type_filter = store.type_filter.clone();
        let tier_filter = store.tier_filter.clone();
        drop(store);

        spawn(async move {
            fact_store.write().set_loading();

            let Some(client) = memory_client(&cfg) else {
                fact_store.write().set_error(FactListError {
                    kind: FactListErrorKind::Connection,
                    message: "could not build API client".to_string(),
                });
                return;
            };

            let store = fact_store.read();
            let include_forgotten = store.include_forgotten();
            drop(store);

            let query = skene_types::FactsQuery {
                sort: sort.wire().to_string(),
                order: "desc".to_string(),
                limit: FactListStore::FETCH_LIMIT as u32,
                filter: Some(search.clone()).filter(|s| !s.is_empty()),
                fact_type: type_filter.first().map(|ft| ft.wire().to_string()),
                tier: tier_filter.first().map(|t| t.wire().to_string()),
                include_forgotten,
                ..Default::default()
            };

            match client.knowledge_facts(&query).await {
                Ok(resp) => {
                    // WHY: apply any additional client-side type/tier narrowing
                    // beyond the single server hint so multi-select reads true.
                    let filtered: Vec<Fact> = resp
                        .facts
                        .into_iter()
                        .map(Fact::from)
                        .filter(|f| type_filter.is_empty() || type_filter.contains(&f.fact_type))
                        .filter(|f| tier_filter.is_empty() || tier_filter.contains(&f.tier))
                        .collect();
                    let active_count = filtered.iter().filter(|f| !f.is_forgotten).count();
                    let total_count = if resp.total == 0 {
                        filtered.len()
                    } else {
                        resp.total
                    };
                    fact_store.write().load(FactListData {
                        facts: filtered,
                        active_count,
                        total_count,
                        legacy_array: false,
                    });
                }
                Err(skene::api::ApiError::Auth { .. }) => {
                    fact_store.write().set_error(FactListError {
                        kind: FactListErrorKind::Non2xx(401),
                        message: "authentication failed".to_string(),
                    });
                }
                Err(skene::api::ApiError::Server { status, message, .. }) => {
                    tracing::warn!(status = %status, "facts request failed");
                    fact_store.write().set_error(FactListError {
                        kind: FactListErrorKind::Non2xx(status),
                        message,
                    });
                }
                Err(e) => {
                    tracing::warn!("facts connection error: {e}");
                    fact_store.write().set_error(FactListError {
                        kind: FactListErrorKind::Connection,
                        message: format!("could not reach server: {e}"),
                    });
                }
            }
        });
    };

    let fetch_entities = move || {
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
            let Some(client) = memory_client(&cfg) else {
                tracing::warn!("entities fetch failed: could not build API client");
                return;
            };

            let (sort_param, order) = match sort {
                EntitySort::PageRank => ("page_rank", "desc"),
                EntitySort::Confidence => ("confidence", "desc"),
                EntitySort::MemoryCount => ("memory_count", "desc"),
                EntitySort::LastUpdated => ("updated_at", "desc"),
                EntitySort::Alphabetical => ("name", "asc"),
            };

            let query = skene_types::EntitiesQuery {
                limit: EntityListStore::PAGE_SIZE as u32,
                offset: (page * EntityListStore::PAGE_SIZE) as u32,
                q: Some(search.clone()).filter(|s| !s.is_empty()),
                sort: sort_param.to_string(),
                order: order.to_string(),
                entity_type: type_filter.iter().map(|t| t.label().to_string()).collect(),
                min_confidence: (min_confidence > 0.0).then_some(min_confidence),
                agent: agent_filter.clone(),
            };

            match client.knowledge_entities(&query).await {
                Ok(resp) => {
                    let entities: Vec<Entity> =
                        resp.entities.into_iter().map(Entity::from).collect();
                    let has_more = entities.len() >= EntityListStore::PAGE_SIZE;

                    let mut store = list_store.write();
                    if page == 0 {
                        store.load(entities, has_more);
                    } else {
                        store.append(entities, has_more);
                    }
                    store.sort_entities();
                }
                Err(e) => {
                    tracing::warn!("entities request failed: {e}");
                }
            }
        });
    };

    // WHY: the graph is an opt-in lens; fetch its entities lazily the first
    // time its tab is shown, then never again on tab switches.
    let mut graph_loaded = use_signal(|| false);

    // WHY: load the active surface. Reads only `tab` + `graph_loaded` in the
    // effect body, so a fetch's own store mutation never re-triggers it.
    use_effect(move || {
        if *tab.read() == MemoryTab::Facts {
            fetch_facts();
        } else if !*graph_loaded.read() {
            graph_loaded.set(true);
            fetch_entities();
        }
    });

    let fetch_detail = move |entity_id: String| {
        let cfg = config.read().clone();
        let id = entity_id.clone();
        detail_state.set(FetchState::Loading);

        spawn(async move {
            let Some(client) = memory_client(&cfg) else {
                detail_state.set(FetchState::Loaded(EntityDetailStore::default()));
                return;
            };

            let entity_fut = client.knowledge_entity(&id);
            let rels_fut = client.knowledge_entity_relationships(&id);
            let mems_fut = client.knowledge_entity_memories(&id);

            let (entity_res, rels_res, mems_res) = tokio::join!(entity_fut, rels_fut, mems_fut);

            let entity: Option<Entity> = entity_res.ok().map(Entity::from);

            let relationships: Vec<Relationship> = rels_res
                .ok()
                .map(|r| r.relationships.into_iter().map(Relationship::from).collect())
                .unwrap_or_default();

            let memories: Vec<EntityMemory> = mems_res
                .ok()
                .map(|m| m.into_iter().map(EntityMemory::from).collect())
                .unwrap_or_default();

            let detail = EntityDetailStore {
                entity,
                relationships,
                memories,
            };

            detail_state.set(FetchState::Loaded(detail));
        });
    };

    let navigate_to_entity = {
        let mut fetch_detail = fetch_detail;
        move |entity_id: String| {
            nav_history.write().push(entity_id.clone());
            selected_entity_id.set(Some(entity_id.clone()));
            fetch_detail(entity_id);
        }
    };

    let active_tab = *tab.read();
    let width = *list_width.read();
    let can_back = nav_history.read().can_go_back();
    let can_forward = nav_history.read().can_go_forward();
    let breadcrumbs: Vec<String> = nav_history.read().breadcrumbs().to_vec();
    let fact_count = fact_store.read().total_count;
    let (health, health_is_subset) = {
        let store = fact_store.read();
        let subset = store.facts.len() != store.total_count && store.total_count > 0;
        (store.health(), subset)
    };

    rsx! {
        div {
            style: "{MEMORY_LAYOUT_STYLE}",
            div {
                style: "{HEADER_STYLE}",
                div {
                    style: "display: flex; align-items: center; gap: var(--space-3);",
                    h2 {
                        style: "font-size: var(--text-lg); margin: 0; color: var(--text-primary);",
                        "Memory"
                    }
                    div {
                        style: "{TABS_STYLE}",
                        button {
                            style: if active_tab == MemoryTab::Facts { "{TAB_BTN_ACTIVE_STYLE}" } else { "{TAB_BTN_STYLE}" },
                            onclick: move |_| tab.set(MemoryTab::Facts),
                            "Facts"
                        }
                        button {
                            style: if active_tab == MemoryTab::Graph { "{TAB_BTN_ACTIVE_STYLE}" } else { "{TAB_BTN_STYLE}" },
                            onclick: move |_| tab.set(MemoryTab::Graph),
                            "Graph / advanced"
                        }
                    }
                }
                div {
                    style: "display: flex; gap: var(--space-2); align-items: center;",
                    if active_tab == MemoryTab::Graph {
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
                    }
                    button {
                        style: "{REFRESH_BTN}",
                        onclick: move |_| {
                            if active_tab == MemoryTab::Facts {
                                fetch_facts();
                            } else {
                                list_store.write().page = 0;
                                fetch_entities();
                            }
                        },
                        "Refresh"
                    }
                }
            }

            if active_tab == MemoryTab::Facts {
                // ── Facts surface: health strip + filters + list ──
                HealthStrip { health, is_subset: health_is_subset }
                FactFilters {
                    list_store: fact_store,
                    on_search_change: move |_query: String| {
                        fetch_facts();
                    },
                    on_filter_change: move |_| {
                        fetch_facts();
                    },
                    on_clear_all: move |_| {
                        fact_store.write().clear_filters();
                        fetch_facts();
                    },
                }
                FactList {
                    list_store: fact_store,
                    on_sort_change: move |sort: FactSort| {
                        fact_store.write().sort = sort;
                        fetch_facts();
                    },
                    on_mutated: move |_| {
                        fetch_facts();
                    },
                    on_retry: move |_| {
                        fetch_facts();
                    },
                }
            } else {
                // ── Graph surface (opt-in advanced lens) ──
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
                                    style: "color: var(--text-primary);",
                                    "{crumb}"
                                }
                            }
                        }
                    }
                }

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

                if list_store.read().entities.is_empty() {
                    div {
                        style: "{GRAPH_EMPTY_STYLE}",
                        div { style: "font-weight: var(--weight-medium); color: var(--text-secondary);", "No entity graph yet" }
                        div { "{fact_count} facts are in your memory list — switch to the Facts tab to browse them." }
                    }
                } else {
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
                        div {
                            style: "{RESIZE_HANDLE_STYLE}",
                            onmousedown: move |evt: Event<MouseData>| {
                                is_resizing.set(true);
                                resize_start_x.set(evt.client_coordinates().x);
                                resize_start_width.set(*list_width.read());
                            },
                        }
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
    }
}
