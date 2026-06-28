//! Two-panel session management: list (left) + detail (right).

pub(crate) mod actions;
pub(crate) mod detail;
pub(crate) mod list;
pub(crate) mod search;

use dioxus::prelude::*;
use skene::api::types::{HistoryMessage, HistoryResponse, PaginatedSessionsResponse, Session};
use skene::id::SessionId;

use crate::api::client::authenticated_client;
use crate::components::resize_handle::{ResizeDir, ResizeHandle, use_resize_state};
use crate::state::agents::AgentStore;
use crate::state::chat::ChatSelection;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::sessions::{
    MessagePreview, SessionDetailStore, SessionListStore, SessionSelectionStore, SessionSort,
    StatusFilter,
};
use crate::state::toasts::{ToastSeverity, ToastStore};
use crate::state::view_preservation::{PreservedViewState, ViewKey, ViewPreservationStore};
use crate::views::sessions::actions::BulkActionBar;
use crate::views::sessions::detail::{SessionDetail, SessionDetailEmpty};
use crate::views::sessions::list::SessionList;
use crate::views::sessions::search::SessionSearchBar;

const SESSIONS_LAYOUT_STYLE: &str = "\
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

const DEFAULT_LIST_WIDTH: f64 = 480.0;
const MIN_LIST_WIDTH: f64 = 280.0;
const MAX_LIST_WIDTH: f64 = 800.0;

fn chat_selection_for_session(session: &Session) -> ChatSelection {
    ChatSelection::for_existing_session(
        session.nous_id.clone(),
        session.id.clone(),
        session.key.clone(),
        session.label().to_string(),
        session.message_count,
    )
}

fn session_detail_from_history(
    session: Session,
    messages: &[HistoryMessage],
) -> SessionDetailStore {
    let mut user_count = 0u32;
    let mut assistant_count = 0u32;
    let mut first_ts: Option<String> = None;
    let mut last_ts: Option<String> = None;
    let mut previews = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "user" => user_count += 1,
            "assistant" => assistant_count += 1,
            _ => {}
        }

        if first_ts.is_none() {
            first_ts = Some(msg.created_at.clone());
        }
        last_ts = Some(msg.created_at.clone());

        let summary = msg
            .content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(120)
            .collect::<String>();

        previews.push(MessagePreview {
            role: msg.role.clone(),
            summary,
            created_at: Some(msg.created_at.clone()),
        });
    }

    SessionDetailStore {
        model: session.model.clone(),
        session: Some(session),
        user_messages: user_count,
        assistant_messages: assistant_count,
        started_at: first_ts,
        ended_at: last_ts,
        message_previews: previews,
        ..SessionDetailStore::default()
    }
}

#[component]
pub(crate) fn Sessions() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut agents: Signal<AgentStore> = use_context();
    let mut chat_selection: Signal<Option<ChatSelection>> = use_context();

    let mut list_store = use_signal(SessionListStore::new);
    // WHY: the sessions endpoint pages by opaque cursor (`after`), not offset;
    // the cursor from the last response is the only way to fetch the next page.
    let mut next_cursor: Signal<Option<String>> = use_signal(|| None);
    let selection_store = use_signal(SessionSelectionStore::new);
    let detail_state =
        use_signal(|| FetchState::<SessionDetailStore>::Loaded(SessionDetailStore::default()));
    let mut selected_session_id: Signal<Option<SessionId>> = use_signal(|| None);

    let resize = use_resize_state(DEFAULT_LIST_WIDTH, MIN_LIST_WIDTH, MAX_LIST_WIDTH);

    // WHY: Restore preserved search state on mount (#2411 context preservation).
    let mut preservation = use_context::<Signal<ViewPreservationStore>>();
    use_hook(|| {
        if let Some(saved) = preservation.write().restore(&ViewKey::Sessions)
            && !saved.input_text.is_empty()
        {
            list_store.write().search_query = saved.input_text;
        }
    });

    use_drop(move || {
        preservation.write().save(
            ViewKey::Sessions,
            PreservedViewState {
                scroll_top: 0.0,
                input_text: list_store.read().search_query.clone(),
                secondary_scroll: 0.0,
            },
        );
    });

    let fetch_sessions = {
        let mut list_store = list_store;
        move || {
            let cfg = config.read().clone();
            let store = list_store.read();
            let search = store.search_query.clone();
            let status = store.status_filter;
            let agent_filter = store.agent_filter.clone();
            let page = store.page;
            drop(store);
            let cursor = if page > 0 {
                next_cursor.read().clone()
            } else {
                None
            };

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');

                let mut url = format!(
                    "{base}/api/v1/sessions?limit={}",
                    SessionListStore::PAGE_SIZE
                );

                if let Some(cursor) = &cursor {
                    let encoded: String = keryx::url::encode_path_segment(cursor);
                    url.push_str(&format!("&after={encoded}"));
                }

                if !search.is_empty() {
                    let encoded: String = keryx::url::encode_path_segment(&search);
                    url.push_str(&format!("&search={encoded}"));
                }

                match status {
                    StatusFilter::Active => url.push_str("&status=active"),
                    StatusFilter::Idle => url.push_str("&status=idle"),
                    StatusFilter::Archived => url.push_str("&status=archived"),
                    StatusFilter::All => {}
                }

                for agent in &agent_filter {
                    let encoded: String = keryx::url::encode_path_segment(agent);
                    url.push_str(&format!("&nous_id={encoded}"));
                }

                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        // WHY: API may return the paginated {items, has_more,
                        // next_cursor} envelope, {sessions: [...]}, or bare [...].
                        let text = match resp.text().await {
                            Ok(t) => t,
                            Err(e) => {
                                tracing::warn!("failed to read sessions response: {e}");
                                return;
                            }
                        };

                        let (sessions, has_more, new_cursor) = if let Ok(envelope) =
                            serde_json::from_str::<PaginatedSessionsResponse>(&text)
                        {
                            // WHY: has_more without a cursor cannot be continued.
                            let more = envelope.has_more && envelope.next_cursor.is_some();
                            (envelope.items, more, envelope.next_cursor)
                        } else if let Ok(list) = serde_json::from_str::<Vec<Session>>(&text) {
                            // NOTE: a bare array carries no cursor, so no
                            // further pages can be requested.
                            (list, false, None)
                        } else {
                            tracing::warn!("failed to parse sessions response");
                            return;
                        };

                        next_cursor.set(new_cursor);

                        let mut store = list_store.write();
                        if page == 0 {
                            store.load(sessions, has_more);
                        } else {
                            store.append(sessions, has_more);
                        }
                        store.sort_sessions();
                    }
                    Ok(resp) => {
                        tracing::warn!(status = %resp.status(), "sessions request failed");
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

    let fetch_detail = {
        let mut detail_state = detail_state;
        move |session_id: SessionId, session: Session| {
            let cfg = config.read().clone();
            detail_state.set(FetchState::Loading);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String = keryx::url::encode_path_segment(session_id.as_ref());
                let url = format!("{base}/api/v1/sessions/{encoded}/history");

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
                        let messages = if let Ok(wrapper) =
                            serde_json::from_str::<HistoryResponse>(&text)
                        {
                            wrapper.messages
                        } else {
                            serde_json::from_str::<Vec<HistoryMessage>>(&text)
                                .unwrap_or_else(|e| {
                                    tracing::warn!(error = %e, "history response is not a message array; using empty history");
                                    Vec::new()
                                })
                        };

                        let detail = session_detail_from_history(session.clone(), &messages);

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

    let archive_session = {
        move |session_id: SessionId| {
            let cfg = config.read().clone();
            let id = session_id.clone();

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String = keryx::url::encode_path_segment(id.as_ref());
                let url = format!("{base}/api/v1/sessions/{encoded}/archive");

                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::info!("archived session {id}");
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        tracing::warn!(%status, "archive failed");
                        if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                            ts.write()
                                .push(ToastSeverity::Error, format!("Archive failed: {status}"));
                        }
                    }
                    Err(e) => {
                        tracing::warn!("archive error: {e}");
                        if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                            ts.write()
                                .push(ToastSeverity::Error, format!("Archive error: {e}"));
                        }
                    }
                }
            });
        }
    };

    let restore_session = {
        move |session_id: SessionId| {
            let cfg = config.read().clone();
            let id = session_id.clone();

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String = keryx::url::encode_path_segment(id.as_ref());
                let url = format!("{base}/api/v1/sessions/{encoded}/unarchive");

                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::info!("restored session {id}");
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        tracing::warn!(%status, "restore failed");
                        if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                            ts.write()
                                .push(ToastSeverity::Error, format!("Restore failed: {status}"));
                        }
                    }
                    Err(e) => {
                        tracing::warn!("restore error: {e}");
                        if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                            ts.write()
                                .push(ToastSeverity::Error, format!("Restore error: {e}"));
                        }
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
            div {
                style: "{HEADER_STYLE}",
                h2 {
                    style: "font-size: var(--text-lg); margin: 0; color: var(--text-primary);",
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
                div {
                    style: "{LIST_PANEL_STYLE} width: {width}px;",
                    role: "region",
                    "aria-label": "Session list",
                    SessionList {
                        list_store,
                        selection_store,
                        on_select_session: {
                            let mut fetch_detail = fetch_detail;
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
                    BulkActionBar {
                        selection_store,
                        on_bulk_archive: move |ids: Vec<SessionId>| {
                            for id in ids {
                                archive_session(id);
                            }
                            list_store.write().page = 0;
                            fetch_sessions();
                        },
                        on_bulk_restore: move |ids: Vec<SessionId>| {
                            for id in ids {
                                restore_session(id);
                            }
                            list_store.write().page = 0;
                            fetch_sessions();
                        },
                    }
                }
                ResizeHandle {
                    dir: ResizeDir::Horizontal,
                    state: resize,
                }
                div {
                    style: "{DETAIL_PANEL_STYLE}",
                    role: "region",
                    "aria-label": "Session detail",
                    if selected_session_id.read().is_some() {
                        SessionDetail {
                            detail_state,
                            on_open_chat: move |id: SessionId| {
                                let selected = match &*detail_state.read() {
                                    FetchState::Loaded(store) => store.session.clone(),
                                    FetchState::Loading | FetchState::Error(_) => None,
                                }
                                .or_else(|| {
                                    list_store
                                        .read()
                                        .sessions
                                        .iter()
                                        .find(|session| session.id == id)
                                        .cloned()
                                });

                                if let Some(session) = selected {
                                    let selection = chat_selection_for_session(&session);
                                    agents.write().set_active(&selection.agent_id);
                                    chat_selection.set(Some(selection));
                                }

                                let nav = navigator();
                                nav.push(crate::app::Route::Chat {});
                            },
                            on_archive: move |id: SessionId| {
                                archive_session(id);
                                list_store.write().page = 0;
                                fetch_sessions();
                            },
                            on_restore: move |id: SessionId| {
                                restore_session(id);
                                list_store.write().page = 0;
                                fetch_sessions();
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

#[cfg(test)]
mod tests {
    use skene::id::{NousId, SessionId};

    use super::*;

    fn session(display_name: Option<&str>) -> Session {
        Session {
            id: SessionId::from("session-id"),
            nous_id: NousId::from("syn"),
            key: "incident-review".to_string(),
            status: "active".to_string(),
            model: Some("mock-model".to_string()),
            message_count: 4,
            token_count_estimate: 128,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:01:00Z".to_string(),
            display_name: display_name.map(str::to_string),
        }
    }

    #[test]
    fn chat_selection_uses_session_owner_key_and_label() {
        let selection = chat_selection_for_session(&session(Some("Incident Review")));

        assert_eq!(selection.agent_id, NousId::from("syn"));
        assert_eq!(selection.session_id.as_deref(), Some("session-id"));
        assert_eq!(selection.session_key, "incident-review");
        assert_eq!(selection.title, "Incident Review");
        assert_eq!(selection.message_count, Some(4));
    }

    #[test]
    fn chat_selection_title_falls_back_to_session_key() {
        let selection = chat_selection_for_session(&session(None));

        assert_eq!(selection.title, "incident-review");
    }

    #[test]
    fn session_detail_projection_uses_pylon_history_without_history_model() {
        let history = r#"{
            "messages": [
                {
                    "id": 1,
                    "seq": 1,
                    "role": "user",
                    "content": "What happened?",
                    "tool_call_id": null,
                    "tool_name": null,
                    "created_at": "2025-01-01T00:00:00Z"
                },
                {
                    "id": 2,
                    "seq": 2,
                    "role": "assistant",
                    "content": "Recovered the transcript.",
                    "tool_call_id": null,
                    "tool_name": null,
                    "created_at": "2025-01-01T00:00:01Z"
                },
                {
                    "id": 3,
                    "seq": 3,
                    "role": "tool",
                    "content": "file contents",
                    "tool_call_id": "toolu_01",
                    "tool_name": "read_file",
                    "created_at": "2025-01-01T00:00:02Z"
                }
            ]
        }"#;
        let response: HistoryResponse = serde_json::from_str(history).unwrap();

        let detail =
            session_detail_from_history(session(Some("Incident Review")), &response.messages);

        assert_eq!(detail.user_messages, 1);
        assert_eq!(detail.assistant_messages, 1);
        assert_eq!(detail.model.as_deref(), Some("mock-model"));
        assert_eq!(detail.started_at.as_deref(), Some("2025-01-01T00:00:00Z"));
        assert_eq!(detail.ended_at.as_deref(), Some("2025-01-01T00:00:02Z"));
        assert_eq!(detail.message_previews.len(), 3);
        assert_eq!(detail.message_previews[2].summary, "file contents");
    }
}
