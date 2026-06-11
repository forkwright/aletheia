//! Memory explorer: list-detail entity browser with search, filtering, and actions.

pub(crate) mod actions;
pub(crate) mod detail;
pub(crate) mod facts;
pub(crate) mod list;
pub(crate) mod search;

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::agents::AgentStore;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::memory::{
    Entity, EntityDetailStore, EntityListStore, EntityMemory, EntityNavigationHistory,
    EntityProperty, EntitySort, EntityType, Relationship,
};
use crate::state::view_preservation::{PreservedViewState, ViewKey, ViewPreservationStore};
use crate::views::memory::detail::EntityDetail;
use crate::views::memory::facts::{FACTS_FETCH_LIMIT, FactsPanel, FactsSnapshot};
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

const NOUS_PILL_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    flex-wrap: wrap; \
    padding-bottom: var(--space-2);\
";

const NOUS_PILL_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) var(--space-3); \
    border-radius: var(--radius-full); \
    border: 1px solid var(--border); \
    background: var(--bg-surface); \
    color: var(--text-secondary); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const NOUS_PILL_ACTIVE_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) var(--space-3); \
    border-radius: var(--radius-full); \
    border: 1px solid var(--border-selected); \
    background: var(--border); \
    color: var(--text-primary); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const DEFAULT_LIST_WIDTH: f64 = 480.0;
const MIN_LIST_WIDTH: f64 = 280.0;
const MAX_LIST_WIDTH: f64 = 800.0;

/// Top-level memory explorer component.
#[component]
pub(crate) fn Memory() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let agent_store = use_context::<Signal<AgentStore>>();

    let mut list_store = use_signal(EntityListStore::new);
    let mut detail_state =
        use_signal(|| FetchState::<EntityDetailStore>::Loaded(EntityDetailStore::default()));
    let mut nav_history = use_signal(EntityNavigationHistory::new);
    let mut selected_entity_id: Signal<Option<String>> = use_signal(|| None);
    let mut facts_state = use_signal(FactsSnapshot::default);
    let mut nous_scope: Signal<Option<String>> = use_signal(|| None);

    let mut list_width = use_signal(|| DEFAULT_LIST_WIDTH);
    let mut is_resizing = use_signal(|| false);
    let mut resize_start_x = use_signal(|| 0.0f64);
    let mut resize_start_width = use_signal(|| 0.0f64);

    // WHY: Restore preserved scroll and search state on mount (#2411).
    let mut preservation = use_context::<Signal<ViewPreservationStore>>();
    use_hook(|| {
        if let Some(saved) = preservation.write().restore(&ViewKey::Memory) {
            // Restore search query from the preserved input text.
            if !saved.input_text.is_empty() {
                list_store.write().search_query = saved.input_text;
            }
        }
    });

    // Save state on unmount.
    use_drop(move || {
        preservation.write().save(
            ViewKey::Memory,
            PreservedViewState {
                scroll_top: 0.0,
                input_text: list_store.read().search_query.clone(),
                secondary_scroll: 0.0,
            },
        );
    });

    let fetch_entities = {
        move || {
            let cfg = config.read().clone();
            let scope = nous_scope.read().clone();
            // WHY: peek, not read — the fetch writes list_store on completion,
            // and a tracked read here would re-trigger the mount effect forever.
            let store = list_store.peek();
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

                if let Some(ref nous) = scope {
                    let encoded: String =
                        form_urlencoded::byte_serialize(nous.as_bytes()).collect();
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

                        // WHY: deserialize through EntityWire — the server sends
                        // entity_type as a free-form string ("person"), which the
                        // EntityType enum's derived Deserialize rejects, killing
                        // the whole response parse and blanking the explorer.
                        let wires: Vec<EntityWire> = if let Ok(wrapper) =
                            serde_json::from_str::<EntitiesResponse>(&text)
                        {
                            wrapper.entities
                        } else if let Ok(list) = serde_json::from_str::<Vec<EntityWire>>(&text) {
                            list
                        } else {
                            tracing::warn!("failed to parse entities response");
                            return;
                        };
                        let entities: Vec<Entity> = wires.into_iter().map(Entity::from).collect();

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
                        tracing::warn!(status = %resp.status(), "entities request failed");
                    }
                    Err(e) => {
                        tracing::warn!("entities connection error: {e}");
                    }
                }
            });
        }
    };

    let fetch_facts = {
        move || {
            let cfg = config.read().clone();
            let scope = nous_scope.read().clone();

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');

                let mut url = format!(
                    "{base}/api/v1/knowledge/facts?limit={FACTS_FETCH_LIMIT}&sort=recency&order=desc"
                );
                if let Some(ref nous) = scope {
                    let encoded: String =
                        form_urlencoded::byte_serialize(nous.as_bytes()).collect();
                    url.push_str(&format!("&nous_id={encoded}"));
                }

                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        match resp.json::<FactsSnapshot>().await {
                            Ok(mut snapshot) => {
                                snapshot.loaded = true;
                                facts_state.set(snapshot);
                            }
                            Err(e) => tracing::warn!("failed to parse facts response: {e}"),
                        }
                    }
                    Ok(resp) => {
                        tracing::warn!(status = %resp.status(), "facts request failed");
                    }
                    Err(e) => {
                        tracing::warn!("facts connection error: {e}");
                    }
                }
            });
        }
    };

    use_effect(move || {
        fetch_entities();
        fetch_facts();
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

                // WHY: response bodies stream in after send() resolves; they
                // must be awaited fully — a single poll yields no data.
                let mut entity: Option<Entity> = match entity_res {
                    Ok(r) if r.status().is_success() => {
                        r.json::<EntityWire>().await.ok().map(Entity::from)
                    }
                    _ => None,
                };
                if entity.is_none() {
                    // WHY: detail endpoint may be unavailable (knowledge store
                    // disabled); fall back to the already-fetched list row.
                    entity = list_store
                        .peek()
                        .entities
                        .iter()
                        .find(|e| e.id == id)
                        .cloned();
                }

                let relationships: Vec<Relationship> = match rels_res {
                    Ok(r) if r.status().is_success() => match r.text().await {
                        Ok(text) => serde_json::from_str::<RelationshipsResponse>(&text)
                            .map(|w| w.relationships)
                            .or_else(|_| serde_json::from_str::<Vec<Relationship>>(&text))
                            .unwrap_or_default(),
                        Err(_) => Vec::new(),
                    },
                    _ => Vec::new(),
                };

                let memories: Vec<EntityMemory> = match mems_res {
                    Ok(r) if r.status().is_success() => {
                        r.json::<Vec<EntityMemory>>().await.unwrap_or_default()
                    }
                    _ => Vec::new(),
                };

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

    // Nous scope pills: (id, label) per known agent, mirroring the sidebar roster.
    let nous_pills: Vec<(String, String)> = agent_store
        .read()
        .all()
        .iter()
        .map(|rec| {
            let emoji = rec.agent.emoji.clone().unwrap_or_default();
            let label = if emoji.is_empty() {
                rec.display_name().to_string()
            } else {
                format!("{emoji} {}", rec.display_name())
            };
            (rec.agent.id.to_string(), label)
        })
        .collect();
    let active_scope = nous_scope.read().clone();
    let scope_label: Option<String> = active_scope.as_deref().and_then(|scope_id| {
        nous_pills
            .iter()
            .find(|(id, _)| id == scope_id)
            .map(|(_, label)| label.clone())
    });

    // WHY: an entity-empty store with no filters is the correct-empty case
    // (facts exist but the graph has not linked them yet) — show facts, not
    // a blank pane. Filtered-empty keeps the normal list's "No entities found".
    let show_facts_fallback =
        list_store.read().entities.is_empty() && !list_store.read().has_active_filters();

    rsx! {
        div {
            style: "{MEMORY_LAYOUT_STYLE}",
            // Header
            div {
                style: "{HEADER_STYLE}",
                h2 {
                    style: "font-size: var(--text-lg); margin: 0; color: var(--text-primary);",
                    "Memory Explorer"
                }
                div {
                    style: "display: flex; gap: var(--space-2); align-items: center;",
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
                            fetch_facts();
                        },
                        "Refresh"
                    }
                }
            }

            // Nous scope pills
            if !nous_pills.is_empty() {
                div {
                    style: "{NOUS_PILL_ROW_STYLE}",
                    button {
                        style: if active_scope.is_none() { "{NOUS_PILL_ACTIVE_STYLE}" } else { "{NOUS_PILL_STYLE}" },
                        onclick: move |_| {
                            if nous_scope.peek().is_some() {
                                list_store.write().page = 0;
                                nous_scope.set(None);
                            }
                        },
                        "All"
                    }
                    for (id , label) in nous_pills.iter() {
                        {
                            let is_active = active_scope.as_deref() == Some(id.as_str());
                            let id_for_click = id.clone();
                            rsx! {
                                button {
                                    key: "nous-{id}",
                                    style: if is_active { "{NOUS_PILL_ACTIVE_STYLE}" } else { "{NOUS_PILL_STYLE}" },
                                    onclick: move |_| {
                                        let next = if nous_scope.peek().as_deref() == Some(id_for_click.as_str()) {
                                            None
                                        } else {
                                            Some(id_for_click.clone())
                                        };
                                        list_store.write().page = 0;
                                        nous_scope.set(next);
                                    },
                                    "{label}"
                                }
                            }
                        }
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
                                style: "color: var(--text-primary);",
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

            // Facts fallback (correct-empty graph) or the two-panel explorer
            if show_facts_fallback {
                FactsPanel {
                    snapshot: facts_state.read().clone(),
                    scope_label,
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
                                    fetch_facts();
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

/// Response wrapper for the entity list endpoint.
#[derive(Debug, serde::Deserialize)]
struct EntitiesResponse {
    entities: Vec<EntityWire>,
}

/// Response wrapper for the entity relationships endpoint.
#[derive(Debug, serde::Deserialize)]
struct RelationshipsResponse {
    relationships: Vec<Relationship>,
}

/// Wire shape for one entity row; `entity_type` arrives as a free-form string.
#[derive(Debug, serde::Deserialize)]
struct EntityWire {
    // kanon:ignore RUST/primitive-for-domain-id — mirrors server-side string IDs from the knowledge API
    id: String,
    name: String,
    #[serde(default)]
    entity_type: String,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    page_rank: f64,
    #[serde(default)]
    memory_count: u32,
    #[serde(default)]
    relationship_count: u32,
    #[serde(default)]
    properties: Vec<EntityProperty>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    created_by: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    flagged: bool,
}

impl From<EntityWire> for Entity {
    fn from(wire: EntityWire) -> Self {
        Self {
            id: wire.id,
            name: wire.name,
            entity_type: parse_entity_type(&wire.entity_type),
            confidence: wire.confidence,
            page_rank: wire.page_rank,
            memory_count: wire.memory_count,
            relationship_count: wire.relationship_count,
            properties: wire.properties,
            updated_at: wire.updated_at,
            created_by: wire.created_by,
            created_at: wire.created_at,
            flagged: wire.flagged,
        }
    }
}

/// Map a server entity-type string onto the fixed display set, case-insensitively.
fn parse_entity_type(raw: &str) -> EntityType {
    if raw.is_empty() {
        return EntityType::Other("Unknown".to_string());
    }
    EntityType::FIXED
        .iter()
        .find(|et| et.label().eq_ignore_ascii_case(raw))
        .cloned()
        .unwrap_or_else(|| EntityType::Other(raw.to_string()))
}
