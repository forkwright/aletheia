//! Two-panel session management: list (left) + detail (right).

pub(crate) mod actions;
pub(crate) mod detail;
pub(crate) mod list;
pub(crate) mod search;

use dioxus::prelude::*;
use theatron_core::api::types::{HistoryResponse, Session, SessionsResponse};
use theatron_core::id::SessionId;

use crate::api::client::authenticated_client;
use crate::components::resize_handle::{use_resize_state, ResizeDir, ResizeHandle};
use crate::state::agents::AgentStore;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::sessions::{
    MessagePreview, SessionDetailStore, SessionListStore, SessionSelectionStore, SessionSort,
    StatusFilter,
};
use crate::views::sessions::actions::BulkActionBar;
use crate::views::sessions::detail::{SessionDetail, SessionDetailEmpty};
use crate::views::sessions::list::SessionList;
use crate::views::sessions::search::SessionSearchBar;

const SESSIONS_LAYOUT_STYLE: &str = "\
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

const DEFAULT_LIST_WIDTH: f64 = 480.0;
const MIN_LIST_WIDTH: f64 = 280.0;
const MAX_LIST_WIDTH: f64 = 800.0;

#[component]
pub(crate) fn Sessions() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let agents: Signal<AgentStore> = use_context();

    let mut list_store = use_signal(SessionListStore::new);
    let selection_store = use_signal(SessionSelectionStore::new);
    let detail_state =
        use_signal(|| FetchState::<SessionDetailStore>::Loaded(SessionDetailStore::default()));
    let mut selected_session_id: Signal<Option<SessionId>> = use_signal(|| None);

    let resize = use_resize_state(DEFAULT_LIST_WIDTH, MIN_LIST_WIDTH, MAX_LIST_WIDTH);

    // Fetch sessions on mount.
    let fetch_sessions = {
        let config = config;
        let mut list_store = list_store;
        move || {
            let cfg = config.read().clone();
            let store = list_store.read();
            let search = store.search_query.clone();
            let status = store.status_filter;
            let agent_filter = store.agent_filter.clone();
            let page = store.page;
            drop(store);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');

                let mut url = format!(
                    "{base}/api/v1/sessions?limit={}",
                    SessionListStore::PAGE_SIZE
                );

                if page > 0 {
                    url.push_str(&format!("&offset={}", page * SessionListStore::PAGE_SIZE));
                }

                if !search.is_empty() {
                    let encoded: String =
                        form_urlencoded::byte_serialize(search.as_bytes()).collect();
                    url.push_str(&format!("&search={encoded}"));
                }

                match status {
                    StatusFilter::Active => url.push_str("&status=active"),
                    StatusFilter::Idle => url.push_str("&status=idle"),
                    StatusFilter::Archived => url.push_str("&status=archived"),
                    StatusFilter::All => {}
                }

                for agent in &agent_filter {
                    let encoded: String =
                        form_urlencoded::byte_serialize(agent.as_bytes()).collect();
                    url.push_str(&format!("&nous_id={encoded}"));
                }

                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        // WHY: API may return {sessions: [...]} or bare [...].
                        let text = match resp.text().await {
                            Ok(t) => t,
                            Err(e) => {
                                tracing::warn!("failed to read sessions response: {e}");
                                return;
                            }
                        };

                        let sessions: Vec<Session> =
                            if let Ok(wrapper) = serde_json::from_str::<SessionsResponse>(&text) {
                                wrapper.sessions
                            } else if let Ok(list) = serde_json::from_str::<Vec<Session>>(&text) {
                                list
                            } else {
                                tracing::warn!("failed to parse sessions response");
                                return;
                            };

                        let has_more = sessions.len() >= SessionListStore::PAGE_SIZE;

                        let mut store = list_store.write();
                        if page == 0 {
                            store.load(sessions, has_more);
                        } else {
                            store.append(sessions, has_more);
                        }
                        store.sort_sessions();
                    }
                    Ok(resp) => {
                        tracing::warn!("sessions request failed: {}", resp.status());
                    }
                    Err(e) => {
                        tracing::warn!("sessions connection error: {e}");
                    }
                }
            });
        }
    };

    use_effect(move || {
        fetch_sessions();
    });

    // Fetch detail for a selected session.
    let fetch_detail = {
        let config = config;
        let mut detail_state = detail_state;
        move |session_id: SessionId, session: Session| {
            let cfg = config.read().clone();
            detail_state.set(FetchState::Loading);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String =
                    form_urlencoded::byte_serialize(session_id.as_ref().as_bytes()).collect();
                let url = format!("{base}/api/v1/sessions/{encoded}/history");

                let mut detail = SessionDetailStore {
                    session: Some(session.clone()),
                    ..SessionDetailStore::default()
                };

                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let text = match resp.text().await {
                            Ok(t) => t,
                            Err(e) => {
                                detail_state.set(FetchState::Error(format!("read history: {e}")));
                                return;
                            }
                        };

                        // WHY: history response may be {messages: [...]} or bare [...].
                        let messages =
                            if let Ok(wrapper) = serde_json::from_str::<HistoryResponse>(&text) {
                                wrapper.messages
                            } else if let Ok(list) = serde_json::from_str::<
                                Vec<theatron_core::api::types::HistoryMessage>,
                            >(&text)
                            {
                                list
                            } else {
                                Vec::new()
                            };

                        let mut user_count = 0u32;
                        let mut assistant_count = 0u32;
                        let mut first_ts: Option<String> = None;
                        let mut last_ts: Option<String> = None;
                        let mut model: Option<String> = None;
                        let mut previews = Vec::new();

                        for msg in &messages {
                            match msg.role.as_str() {
                                "user" => user_count += 1,
                                "assistant" => {
                                    assistant_count += 1;
                                    if model.is_none() {
                                        model = msg.model.clone();
                                    }
                                }
                                _ => {}
                            }

                            if let Some(ref ts) = msg.created_at {
                                if first_ts.is_none() {
                                    first_ts = Some(ts.clone());
                                }
                                last_ts = Some(ts.clone());
                            }

                            let summary = msg
                                .content
                                .as_ref()
                                .and_then(|c| {
                                    c.as_str().map(|s| {
                                        s.lines()
                                            .next()
                                            .unwrap_or("")
                                            .chars()
                                            .take(120)
                                            .collect::<String>()
                                    })
                                })
                                .unwrap_or_default();

                            previews.push(MessagePreview {
                                role: msg.role.clone(),
                                summary,
                                created_at: msg.created_at.clone(),
                            });
                        }

                        detail.user_messages = user_count;
                        detail.assistant_messages = assistant_count;
                        detail.model = model;
                        detail.started_at = first_ts;
                        detail.ended_at = last_ts;
                        detail.message_previews = previews;

                        detail_state.set(FetchState::Loaded(detail));
                    }
                    Ok(resp) => {
                        detail_state.set(FetchState::Error(format!(
                            "server returned {}",
                            resp.status()
                        )));
                    }
                    Err(e) => {
                        detail_state.set(FetchState::Error(format!("connection error: {e}")));
                    }
                }
            });
        }
    };

    // Archive a session.
    let archive_session = {
        let config = config;
        move |session_id: SessionId| {
            let cfg = config.read().clone();
            let id = session_id.clone();

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String =
                    form_urlencoded::byte_serialize(id.as_ref().as_bytes()).collect();
                let url = format!("{base}/api/v1/sessions/{encoded}/archive");

                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::info!("archived session {id}");
                    }
                    Ok(resp) => {
                        tracing::warn!("archive failed: {}", resp.status());
                    }
                    Err(e) => {
                        tracing::warn!("archive error: {e}");
                    }
                }
            });
        }
    };

    // Restore (unarchive) a session.
    let restore_session = {
        let config = config;
        move |session_id: SessionId| {
            let cfg = config.read().clone();
            let id = session_id.clone();

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String =
                    form_urlencoded::byte_serialize(id.as_ref().as_bytes()).collect();
                let url = format!("{base}/api/v1/sessions/{encoded}/unarchive");

                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::info!("restored session {id}");
                    }
                    Ok(resp) => {
                        tracing::warn!("restore failed: {}", resp.status());
                    }
                    Err(e) => {
                        tracing::warn!("restore error: {e}");
                    }
                }
            });
        }
    };

    let agent_names: Vec<String> = agents
        .read()
        .all()
        .iter()
        .map(|r| r.agent.id.to_string())
        .collect();

    let width = *resize.size.read();

    rsx! {
        div {
            style: "{SESSIONS_LAYOUT_STYLE}",
            // Header
            div {
                style: "{HEADER_STYLE}",
                h2 {
                    style: "font-size: 18px; margin: 0; color: var(--text-primary, #e0e0e0);",
                    "Sessions"
                }
                button {
                    style: "{REFRESH_BTN}",
                    "aria-label": "Refresh sessions",
                    onclick: move |_| {
                        list_store.write().page = 0;
                        fetch_sessions();
                    },
                    "Refresh"
                }
            }
            // Search bar
            SessionSearchBar {
                list_store,
                agent_names,
                on_search_change: move |_query: String| {
                    list_store.write().page = 0;
                    fetch_sessions();
                },
                on_status_change: move |filter: StatusFilter| {
                    let mut store = list_store.write();
                    store.status_filter = filter;
                    store.page = 0;
                    drop(store);
                    fetch_sessions();
                },
                on_agent_change: move |agents: Vec<String>| {
                    let mut store = list_store.write();
                    store.agent_filter = agents;
                    store.page = 0;
                    drop(store);
                    fetch_sessions();
                },
                on_clear_all: move |_| {
                    list_store.write().clear_filters();
                    fetch_sessions();
                },
            }
            // Two-panel layout
            div {
                style: "{PANELS_STYLE}",
                role: "region",
                "aria-label": "Sessions workspace",
                onmousemove: move |evt: Event<MouseData>| {
                    let c = evt.client_coordinates();
                    resize.on_move(c.x, c.y, ResizeDir::Horizontal);
                },
                onmouseup: move |_| {
                    resize.on_up();
                },
                // List panel
                div {
                    style: "{LIST_PANEL_STYLE} width: {width}px;",
                    role: "region",
                    "aria-label": "Session list",
                    SessionList {
                        list_store,
                        selection_store,
                        on_select_session: {
                            let mut fetch_detail = fetch_detail.clone();
                            move |id: SessionId| {
                                selected_session_id.set(Some(id.clone()));
                                let session = list_store.read().sessions
                                    .iter()
                                    .find(|s| s.id == id)
                                    .cloned();
                                if let Some(session) = session {
                                    fetch_detail(id, session);
                                }
                            }
                        },
                        on_sort_change: move |sort: SessionSort| {
                            let mut store = list_store.write();
                            store.sort = sort;
                            store.sort_sessions();
                        },
                        on_load_more: move |_| {
                            list_store.write().page += 1;
                            fetch_sessions();
                        },
                    }
                    // Bulk action bar
                    BulkActionBar {
                        selection_store,
                        on_bulk_archive: {
                            let archive_session = archive_session.clone();
                            move |ids: Vec<SessionId>| {
                                for id in ids {
                                    archive_session(id);
                                }
                                list_store.write().page = 0;
                                fetch_sessions();
                            }
                        },
                        on_bulk_restore: {
                            let restore_session = restore_session.clone();
                            move |ids: Vec<SessionId>| {
                                for id in ids {
                                    restore_session(id);
                                }
                                list_store.write().page = 0;
                                fetch_sessions();
                            }
                        },
                    }
                }
                // Resize handle — replaces inline div
                ResizeHandle {
                    dir: ResizeDir::Horizontal,
                    state: resize,
                }
                // Detail panel
                div {
                    style: "{DETAIL_PANEL_STYLE}",
                    role: "region",
                    "aria-label": "Session detail",
                    if selected_session_id.read().is_some() {
                        SessionDetail {
                            detail_state,
                            on_open_chat: move |_id: SessionId| {
                                // TODO(#108): set active agent and session in chat state before navigating.
                                let nav = navigator();
                                nav.push(crate::app::Route::Chat {});
                            },
                            on_archive: {
                                let archive_session = archive_session.clone();
                                move |id: SessionId| {
                                    archive_session(id);
                                    list_store.write().page = 0;
                                    fetch_sessions();
                                }
                            },
                            on_restore: {
                                let restore_session = restore_session.clone();
                                move |id: SessionId| {
                                    restore_session(id);
                                    list_store.write().page = 0;
                                    fetch_sessions();
                                }
                            },
                        }
                    } else {
                        SessionDetailEmpty {}
                    }
                }
            }
        }
    }
}
