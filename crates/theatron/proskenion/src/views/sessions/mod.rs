//! Two-panel session management: list (left) + detail (right).

pub(crate) mod actions;
pub(crate) mod detail;
pub(crate) mod list;
pub(crate) mod search;

use dioxus::prelude::*;
use skene::api::error::{format_http_error_body, parse_pylon_error_body};
use skene::api::types::{
    HistoryMessage, HistoryResponse, PaginatedSessionsResponse, Session, SessionsResponse,
};
use skene::id::SessionId;

use crate::api::client::authenticated_client;
use crate::components::resize_handle::{ResizeDir, ResizeHandle, use_resize_state};
use crate::state::agents::AgentStore;
use crate::state::chat::ChatSelection;
use crate::state::connection::ConnectionConfig;
use crate::state::sessions::{
    MessagePreview, SessionDetailStore, SessionListStore, SessionLoadFailure, SessionLoadState,
    SessionSelectionStore, SessionSort, StatusFilter,
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

#[derive(Debug)]
struct ParsedSessionsPage {
    sessions: Vec<Session>,
    has_more: bool,
    next_cursor: Option<String>,
    total_count: Option<usize>,
}

fn response_request_id(headers: &reqwest::header::HeaderMap) -> Option<String> {
    ["x-request-id", "request-id"].iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
    })
}

fn make_failure(
    path: String,
    status: Option<u16>,
    request_id: Option<String>,
    message: String,
) -> SessionLoadFailure {
    SessionLoadFailure {
        path,
        status,
        request_id,
        message,
    }
}

fn http_failure(
    path: String,
    status: reqwest::StatusCode,
    request_id: Option<String>,
    body: &str,
) -> SessionLoadFailure {
    let request_id =
        request_id.or_else(|| parse_pylon_error_body(body).and_then(|error| error.request_id));
    let reason = status.canonical_reason().unwrap_or("HTTP error");
    let message = if body.trim().is_empty() {
        format!("server returned {} {reason}", status.as_u16())
    } else {
        format_http_error_body(status.as_u16(), reason, body)
    };

    make_failure(path, Some(status.as_u16()), request_id, message)
}

fn parse_sessions_response(text: &str) -> Result<ParsedSessionsPage, String> {
    match serde_json::from_str::<PaginatedSessionsResponse>(text) {
        Ok(envelope) => {
            let total_count = envelope.total.and_then(|total| usize::try_from(total).ok());
            // WHY: has_more without a cursor cannot be continued.
            let has_more = envelope.has_more && envelope.next_cursor.is_some();
            Ok(ParsedSessionsPage {
                sessions: envelope.items,
                has_more,
                next_cursor: envelope.next_cursor,
                total_count,
            })
        }
        Err(envelope_err) => {
            if let Ok(wrapper) = serde_json::from_str::<SessionsResponse>(text) {
                return Ok(ParsedSessionsPage {
                    sessions: wrapper.sessions,
                    has_more: false,
                    next_cursor: None,
                    total_count: None,
                });
            }

            match serde_json::from_str::<Vec<Session>>(text) {
                Ok(list) => Ok(ParsedSessionsPage {
                    sessions: list,
                    has_more: false,
                    next_cursor: None,
                    total_count: None,
                }),
                Err(list_err) => Err(format!(
                    "expected paginated sessions envelope, sessions wrapper, or array: {envelope_err}; {list_err}"
                )),
            }
        }
    }
}

fn parse_history_response(text: &str) -> Result<Vec<HistoryMessage>, String> {
    match serde_json::from_str::<HistoryResponse>(text) {
        Ok(wrapper) => Ok(wrapper.messages),
        Err(wrapper_err) => match serde_json::from_str::<Vec<HistoryMessage>>(text) {
            Ok(messages) => Ok(messages),
            Err(list_err) => Err(format!(
                "expected history wrapper or message array: {wrapper_err}; {list_err}"
            )),
        },
    }
}

fn detail_store_from_history(
    session: Session,
    messages: Vec<HistoryMessage>,
) -> SessionDetailStore {
    let mut detail = SessionDetailStore {
        session: Some(session),
        ..SessionDetailStore::default()
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
            _ => {} // NOTE: non-assistant messages have no model to extract
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
            .unwrap_or_else(String::new);

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

    detail
}

fn detail_state_from_history(
    session: Session,
    messages: Vec<HistoryMessage>,
) -> SessionLoadState<SessionDetailStore> {
    let is_empty = messages.is_empty();
    let detail = detail_store_from_history(session, messages);
    if is_empty {
        SessionLoadState::Empty(detail)
    } else {
        SessionLoadState::Loaded(detail)
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
        use_signal(|| SessionLoadState::<SessionDetailStore>::Empty(SessionDetailStore::default()));
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

    let mut fetch_sessions = {
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

            list_store.write().mark_loading();

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');

                let mut path = format!("/api/v1/sessions?limit={}", SessionListStore::PAGE_SIZE);

                if let Some(cursor) = &cursor {
                    let encoded: String = keryx::url::encode_path_segment(cursor);
                    path.push_str(&format!("&after={encoded}"));
                }

                if !search.is_empty() {
                    let encoded: String = keryx::url::encode_path_segment(&search);
                    path.push_str(&format!("&search={encoded}"));
                }

                if let Some(status) = status.query_value() {
                    path.push_str("&status=");
                    path.push_str(status);
                }

                for agent in &agent_filter {
                    let encoded: String = keryx::url::encode_path_segment(agent);
                    path.push_str(&format!("&nous_id={encoded}"));
                }

                let url = format!("{base}{path}");

                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let status = resp.status().as_u16();
                        let request_id = response_request_id(resp.headers());

                        let text = match resp.text().await {
                            Ok(t) => t,
                            Err(e) => {
                                let failure = make_failure(
                                    path.clone(),
                                    Some(status),
                                    request_id,
                                    format!("failed to read sessions response: {e}"),
                                );
                                tracing::warn!(
                                    path = %failure.path,
                                    status = failure.status.unwrap_or_default(),
                                    request_id = failure.request_id.as_deref().unwrap_or(""),
                                    error = %e,
                                    "sessions response transport error"
                                );
                                list_store
                                    .write()
                                    .mark_failed(SessionLoadState::TransportError(failure));
                                return;
                            }
                        };

                        let parsed = match parse_sessions_response(&text) {
                            Ok(parsed) => parsed,
                            Err(e) => {
                                let failure = make_failure(
                                    path.clone(),
                                    Some(status),
                                    request_id,
                                    format!("failed to parse sessions response: {e}"),
                                );
                                tracing::warn!(
                                    path = %failure.path,
                                    status = failure.status.unwrap_or_default(),
                                    request_id = failure.request_id.as_deref().unwrap_or(""),
                                    error = %e,
                                    "sessions response contract error"
                                );
                                list_store
                                    .write()
                                    .mark_failed(SessionLoadState::ContractError(failure));
                                return;
                            }
                        };

                        next_cursor.set(parsed.next_cursor);

                        let mut store = list_store.write();
                        if page == 0 {
                            store.load(parsed.sessions, parsed.has_more);
                        } else {
                            store.append(parsed.sessions, parsed.has_more);
                        }
                        store.total_count = parsed.total_count;
                        store.sort_sessions();
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let request_id = response_request_id(resp.headers());
                        let body = match resp.text().await {
                            Ok(text) => text,
                            Err(e) => {
                                tracing::warn!(
                                    path = %path,
                                    status = status.as_u16(),
                                    request_id = request_id.as_deref().unwrap_or(""),
                                    error = %e,
                                    "failed to read sessions error response"
                                );
                                String::new()
                            }
                        };
                        let failure = http_failure(path.clone(), status, request_id, &body);
                        tracing::warn!(
                            path = %failure.path,
                            status = failure.status.unwrap_or_default(),
                            request_id = failure.request_id.as_deref().unwrap_or(""),
                            error = %failure.message,
                            "sessions request failed"
                        );
                        list_store
                            .write()
                            .mark_failed(SessionLoadState::HttpError(failure));
                    }
                    Err(e) => {
                        let failure = make_failure(
                            path,
                            None,
                            None,
                            format!("sessions connection error: {e}"),
                        );
                        tracing::warn!(
                            path = %failure.path,
                            error = %e,
                            "sessions request transport error"
                        );
                        list_store
                            .write()
                            .mark_failed(SessionLoadState::TransportError(failure));
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
            detail_state.set(SessionLoadState::Loading);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let base = cfg.server_url.trim_end_matches('/');
                let encoded: String = keryx::url::encode_path_segment(session_id.as_ref());
                let path = format!("/api/v1/sessions/{encoded}/history");
                let url = format!("{base}{path}");

                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let status = resp.status().as_u16();
                        let request_id = response_request_id(resp.headers());

                        let text = match resp.text().await {
                            Ok(t) => t,
                            Err(e) => {
                                let failure = make_failure(
                                    path.clone(),
                                    Some(status),
                                    request_id,
                                    format!("failed to read session history: {e}"),
                                );
                                tracing::warn!(
                                    path = %failure.path,
                                    status = failure.status.unwrap_or_default(),
                                    request_id = failure.request_id.as_deref().unwrap_or(""),
                                    error = %e,
                                    "session history transport error"
                                );
                                detail_state.set(SessionLoadState::TransportError(failure));
                                return;
                            }
                        };

                        let messages = match parse_history_response(&text) {
                            Ok(messages) => messages,
                            Err(e) => {
                                let failure = make_failure(
                                    path.clone(),
                                    Some(status),
                                    request_id,
                                    format!("failed to parse session history: {e}"),
                                );
                                tracing::warn!(
                                    path = %failure.path,
                                    status = failure.status.unwrap_or_default(),
                                    request_id = failure.request_id.as_deref().unwrap_or(""),
                                    error = %e,
                                    "session history contract error"
                                );
                                detail_state.set(SessionLoadState::ContractError(failure));
                                return;
                            }
                        };

                        detail_state.set(detail_state_from_history(session, messages));
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let request_id = response_request_id(resp.headers());
                        let body = match resp.text().await {
                            Ok(text) => text,
                            Err(e) => {
                                tracing::warn!(
                                    path = %path,
                                    status = status.as_u16(),
                                    request_id = request_id.as_deref().unwrap_or(""),
                                    error = %e,
                                    "failed to read session history error response"
                                );
                                String::new()
                            }
                        };
                        let failure = http_failure(path.clone(), status, request_id, &body);
                        tracing::warn!(
                            path = %failure.path,
                            status = failure.status.unwrap_or_default(),
                            request_id = failure.request_id.as_deref().unwrap_or(""),
                            error = %failure.message,
                            "session history request failed"
                        );
                        detail_state.set(SessionLoadState::HttpError(failure));
                    }
                    Err(e) => {
                        let failure = make_failure(
                            path,
                            None,
                            None,
                            format!("session history connection error: {e}"),
                        );
                        tracing::warn!(
                            path = %failure.path,
                            error = %e,
                            "session history transport error"
                        );
                        detail_state.set(SessionLoadState::TransportError(failure));
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
                        on_retry: move |_| {
                            list_store.write().page = 0;
                            fetch_sessions();
                        },
                    }
                    BulkActionBar {
                        list_store,
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
                                    SessionLoadState::Loaded(store)
                                    | SessionLoadState::Empty(store) => store.session.clone(),
                                    SessionLoadState::Loading
                                    | SessionLoadState::TransportError(_)
                                    | SessionLoadState::HttpError(_)
                                    | SessionLoadState::ContractError(_) => None,
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
                            on_retry: {
                                let mut fetch_detail = fetch_detail;
                                move |_| {
                                    let Some(id) = selected_session_id.read().clone() else {
                                        return;
                                    };
                                    let session = list_store
                                        .read()
                                        .sessions
                                        .iter()
                                        .find(|session| session.id == id)
                                        .cloned();
                                    if let Some(session) = session {
                                        fetch_detail(id, session);
                                    }
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

#[cfg(test)]
mod tests {
    use skene::id::{NousId, SessionId};

    use super::*;

    fn session(display_name: Option<&str>) -> Session {
        Session {
            id: SessionId::from("session-id"),
            nous_id: NousId::from("syn"),
            key: "incident-review".to_string(),
            status: Some("active".to_string()),
            message_count: 4,
            session_type: None,
            updated_at: None,
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
    fn list_http_500_preserves_status_path_and_request_id() {
        let failure = http_failure(
            "/api/v1/sessions?limit=50".to_string(),
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            None,
            r#"{"error":{"code":"session_store_failed","message":"session store unavailable","request_id":"req-500"}}"#,
        );
        let state: SessionLoadState<()> = SessionLoadState::HttpError(failure);

        let SessionLoadState::HttpError(failure) = state else {
            panic!("500 response should become an HTTP error state");
        };
        assert_eq!(failure.path, "/api/v1/sessions?limit=50");
        assert_eq!(failure.status, Some(500));
        assert_eq!(failure.request_id.as_deref(), Some("req-500"));
        assert!(failure.message.contains("session store unavailable"));
    }

    #[test]
    fn invalid_session_list_json_becomes_contract_error() {
        let error = match parse_sessions_response(r#"{"items":"not a list"}"#) {
            Ok(_) => panic!("invalid sessions response should not parse"),
            Err(error) => error,
        };
        let state: SessionLoadState<()> = SessionLoadState::ContractError(make_failure(
            "/api/v1/sessions?limit=50".to_string(),
            Some(200),
            Some("req-contract".to_string()),
            format!("failed to parse sessions response: {error}"),
        ));

        let SessionLoadState::ContractError(failure) = state else {
            panic!("invalid sessions response should become a contract error");
        };
        assert_eq!(failure.status, Some(200));
        assert_eq!(failure.request_id.as_deref(), Some("req-contract"));
        assert!(
            failure
                .message
                .contains("failed to parse sessions response")
        );
    }

    #[test]
    fn malformed_history_does_not_become_empty_detail() {
        let error = match parse_history_response(r#"{"messages":[{"role":5}]}"#) {
            Ok(_) => panic!("malformed history response should not parse"),
            Err(error) => error,
        };
        let state: SessionLoadState<SessionDetailStore> =
            SessionLoadState::ContractError(make_failure(
                "/api/v1/sessions/session-id/history".to_string(),
                Some(200),
                Some("req-history".to_string()),
                format!("failed to parse session history: {error}"),
            ));

        let SessionLoadState::ContractError(failure) = state else {
            panic!("malformed history should become a contract error");
        };
        assert_eq!(failure.path, "/api/v1/sessions/session-id/history");
        assert_eq!(failure.request_id.as_deref(), Some("req-history"));
        assert!(failure.message.contains("failed to parse session history"));
    }

    #[test]
    fn legitimate_empty_sessions_response_sets_empty_state() {
        let parsed = match parse_sessions_response(r#"{"items":[],"has_more":false,"total":0}"#) {
            Ok(parsed) => parsed,
            Err(error) => panic!("empty sessions response should parse: {error}"),
        };
        let mut store = SessionListStore::new();
        store.load(parsed.sessions, parsed.has_more);
        store.total_count = parsed.total_count;

        assert!(store.sessions.is_empty());
        assert_eq!(store.total_count, Some(0));
        assert!(matches!(store.load_state, SessionLoadState::Empty(())));
    }
}
