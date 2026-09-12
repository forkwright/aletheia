//! Two-panel session management: list (left) + detail (right).

pub(crate) mod actions;
pub(crate) mod detail;
pub(crate) mod list;
pub(crate) mod search;

use dioxus::prelude::*;
use skene::api::error::ApiError;
use skene::api::types::{HistoryMessage, Session};
use skene::id::ApiSessionId;

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

/// Classify an [`ApiError`] from a sessions-view request into the load state
/// the UI renders.
///
/// WHY(#7198): `skene::api::client::ApiClient::sessions_paginated` and
/// `::history` decode their response with `reqwest::Response::json`, which
/// folds a dropped connection and an undecodable body into the same
/// `ApiError::Http` variant (unlike `health_details`, which hand-parses its
/// body and can report `ApiError::BadResponse` distinctly). Proskenion has
/// no way to re-derive that distinction from outside skene, so it is not
/// invented here: `Http`/`Timeout`/`InvalidToken` (nothing usable was ever
/// received) become `TransportError`, everything else (a response came
/// back, it just was not success) becomes `HttpError`. `operation` labels
/// which request failed for the `path` field of [`SessionLoadFailure`],
/// which now names the operation rather than a literal request path --
/// `ApiError`'s `Display` does not carry the wire path either.
fn session_load_state_for_error<T>(err: ApiError, operation: &'static str) -> SessionLoadState<T> {
    let message = err.to_string();
    match err {
        ApiError::Http { .. } | ApiError::Timeout { .. } | ApiError::InvalidToken => {
            SessionLoadState::TransportError(SessionLoadFailure {
                path: operation.to_string(),
                status: None,
                request_id: None,
                message,
            })
        }
        ApiError::Server { status, .. } => SessionLoadState::HttpError(SessionLoadFailure {
            path: operation.to_string(),
            status: Some(status),
            request_id: None,
            message,
        }),
        // WHY: RateLimited, BadResponse, BodyTooLarge, Auth, and any future
        // `ApiError` variant (the enum is `#[non_exhaustive]`) all mean a
        // response came back and was rejected for a reason with no status
        // code this view can extract; grouped as `HttpError` with `status`
        // left unset rather than invented.
        _ => SessionLoadState::HttpError(SessionLoadFailure {
            path: operation.to_string(),
            status: None,
            request_id: None,
            message,
        }),
    }
}

fn detail_store_from_history(
    session: Session,
    messages: Vec<HistoryMessage>,
) -> SessionDetailStore {
    // WHY(#4911): pylon's history wire format carries no per-message model
    // (it never did -- scanning `msg.model` was a silent no-op against real
    // data). The model is a session-level fact; take it from `session`
    // before it moves into `detail.session` below.
    let model = session.model.clone();
    let mut detail = SessionDetailStore {
        session: Some(session),
        ..SessionDetailStore::default()
    };

    let mut user_count = 0u32;
    let mut assistant_count = 0u32;
    let mut first_ts: Option<String> = None;
    let mut last_ts: Option<String> = None;
    let mut previews = Vec::new();

    for msg in &messages {
        match msg.role.as_str() {
            "user" => user_count += 1,
            "assistant" => assistant_count += 1,
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
    let mut selected_session_id: Signal<Option<ApiSessionId>> = use_signal(|| None);

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
                let client = match skene::api::client::ApiClient::new(
                    &cfg.server_url,
                    cfg.auth_token.clone(),
                ) {
                    Ok(client) => client,
                    Err(err) => {
                        list_store.write().mark_failed(session_load_state_for_error(
                            err,
                            "build sessions client",
                        ));
                        return;
                    }
                };

                // WHY(#7198): pylon's `nous_id` filter is a single scalar
                // (`crates/pylon/src/handlers/sessions/mod.rs`), never a
                // multi-value one -- the prior hand-rolled request sent one
                // `&nous_id=` per selected agent, which pylon's own `Option
                // <String>` extractor cannot honor as a set. The first
                // selection is the only one that was ever actually
                // reachable server-side; this keeps that behavior explicit
                // instead of an accident of query-string repetition.
                let params = skene::api::types::ListSessionsRequest {
                    nous_id: agent_filter.first().cloned(),
                    search: (!search.is_empty()).then(|| search.clone()),
                    status: match status {
                        StatusFilter::All => None,
                        StatusFilter::Active => Some(skene::api::types::SessionLifecycle::Active),
                        StatusFilter::Archived => {
                            Some(skene::api::types::SessionLifecycle::Archived)
                        }
                        StatusFilter::Distilled => {
                            Some(skene::api::types::SessionLifecycle::Distilled)
                        }
                    },
                    limit: Some(u32::try_from(SessionListStore::PAGE_SIZE).unwrap_or(u32::MAX)),
                    after: cursor,
                };

                match client.sessions_paginated(&params).await {
                    Ok(envelope) => {
                        let total_count =
                            envelope.total.and_then(|total| usize::try_from(total).ok());
                        // WHY: has_more without a cursor cannot be continued.
                        let has_more = envelope.has_more && envelope.next_cursor.is_some();
                        next_cursor.set(envelope.next_cursor);

                        let mut store = list_store.write();
                        if page == 0 {
                            store.load(envelope.items, has_more);
                        } else {
                            store.append(envelope.items, has_more);
                        }
                        store.total_count = total_count;
                        store.sort_sessions();
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "sessions request failed");
                        list_store
                            .write()
                            .mark_failed(session_load_state_for_error(err, "load sessions"));
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
        move |session_id: ApiSessionId, session: Session| {
            let cfg = config.read().clone();
            detail_state.set(SessionLoadState::Loading);

            spawn(async move {
                let client = match skene::api::client::ApiClient::new(
                    &cfg.server_url,
                    cfg.auth_token.clone(),
                ) {
                    Ok(client) => client,
                    Err(err) => {
                        detail_state.set(session_load_state_for_error(
                            err,
                            "build session history client",
                        ));
                        return;
                    }
                };

                match client.history(session_id.as_ref(), None, None).await {
                    Ok(messages) => {
                        detail_state.set(detail_state_from_history(session, messages));
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "session history request failed");
                        detail_state.set(session_load_state_for_error(err, "load session history"));
                    }
                }
            });
        }
    };

    let archive_session = {
        move |session_id: ApiSessionId| {
            let cfg = config.read().clone();
            let id = session_id.clone();

            spawn(async move {
                let client = match skene::api::client::ApiClient::new(
                    &cfg.server_url,
                    cfg.auth_token.clone(),
                ) {
                    Ok(client) => client,
                    Err(err) => {
                        if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                            ts.write().push(ToastSeverity::Error, err.to_string());
                        }
                        return;
                    }
                };

                match client.archive_session(id.as_ref()).await {
                    Ok(()) => {
                        tracing::info!("archived session {id}");
                    }
                    Err(err) => {
                        tracing::warn!("archive failed: {err}");
                        if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                            ts.write()
                                .push(ToastSeverity::Error, format!("Archive failed: {err}"));
                        }
                    }
                }
            });
        }
    };

    let restore_session = {
        move |session_id: ApiSessionId| {
            let cfg = config.read().clone();
            let id = session_id.clone();

            spawn(async move {
                let client = match skene::api::client::ApiClient::new(
                    &cfg.server_url,
                    cfg.auth_token.clone(),
                ) {
                    Ok(client) => client,
                    Err(err) => {
                        if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                            ts.write().push(ToastSeverity::Error, err.to_string());
                        }
                        return;
                    }
                };

                match client.unarchive_session(id.as_ref()).await {
                    Ok(()) => {
                        tracing::info!("restored session {id}");
                    }
                    Err(err) => {
                        tracing::warn!("restore failed: {err}");
                        if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                            ts.write()
                                .push(ToastSeverity::Error, format!("Restore failed: {err}"));
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
                            move |id: ApiSessionId| {
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
                        on_bulk_archive: move |ids: Vec<ApiSessionId>| {
                            for id in ids {
                                archive_session(id);
                            }
                            list_store.write().page = 0;
                            fetch_sessions();
                        },
                        on_bulk_restore: move |ids: Vec<ApiSessionId>| {
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
                            on_open_chat: move |id: ApiSessionId| {
                                let selected = match &*detail_state.read() {
                                    SessionLoadState::Loaded(store)
                                    | SessionLoadState::Empty(store) => store.session.clone(),
                                    SessionLoadState::Loading
                                    | SessionLoadState::TransportError(_)
                                    | SessionLoadState::HttpError(_) => None,
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
                            on_archive: move |id: ApiSessionId| {
                                archive_session(id);
                                list_store.write().page = 0;
                                fetch_sessions();
                            },
                            on_restore: move |id: ApiSessionId| {
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
    use skene::id::{ApiNousId, ApiSessionId};

    use super::*;

    fn session(display_name: Option<&str>) -> Session {
        Session {
            id: ApiSessionId::from("session-id"),
            nous_id: ApiNousId::from("syn"),
            key: "incident-review".to_string(),
            status: Some("active".to_string()),
            model: None,
            message_count: 4,
            session_type: None,
            updated_at: None,
            display_name: display_name.map(str::to_string),
            active_turn_id: None,
        }
    }

    // WHY(#4911): detail.model must come from the session, not from
    // scanning history messages -- the wire format never carried a
    // per-message model, so that scan was always a silent no-op.
    #[test]
    fn detail_store_model_comes_from_session_not_messages() {
        let mut s = session(Some("Incident Review"));
        s.model = Some("claude-opus-4-6".to_string());
        let messages = vec![HistoryMessage {
            id: None,
            seq: None,
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String("hi".to_string())),
            created_at: None,
            tool_call_id: None,
            tool_name: None,
            token_estimate: 0,
            is_distilled: false,
        }];

        let detail = detail_store_from_history(s, messages);

        assert_eq!(detail.model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn chat_selection_uses_session_owner_key_and_label() {
        let selection = chat_selection_for_session(&session(Some("Incident Review")));

        assert_eq!(selection.agent_id, ApiNousId::from("syn"));
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

    fn install_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    /// Spawn a one-shot raw-TCP HTTP server that replies with a fixed
    /// status/body to the single request it receives. Mirrors
    /// `crate::api::client::tests::spawn_auth_required_roster`.
    async fn spawn_http_response(
        status_line: &'static str,
        body: &'static str,
    ) -> std::io::Result<(String, tokio::task::JoinHandle<std::io::Result<()>>)> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut buf = [0_u8; 4096];
            let _ = stream.read(&mut buf).await?;
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await?;
            Ok(())
        });
        Ok((format!("http://{addr}"), handle))
    }

    #[tokio::test]
    async fn session_list_server_error_becomes_http_error_state()
    -> Result<(), Box<dyn std::error::Error>> {
        install_crypto();
        let (server_url, server) = spawn_http_response(
            "500 Internal Server Error",
            r#"{"error":{"code":"session_store_failed","message":"session store unavailable","request_id":"req-500"}}"#,
        )
        .await?;
        let client = skene::api::client::ApiClient::new(&server_url, None)?;

        let err = client
            .sessions_paginated(&skene::api::types::ListSessionsRequest::default())
            .await
            .expect_err("500 response should fail");

        let state: SessionLoadState<()> = session_load_state_for_error(err, "load sessions");
        let SessionLoadState::HttpError(failure) = state else {
            panic!("500 response should become an HTTP error state");
        };
        assert_eq!(failure.status, Some(500));
        assert!(failure.message.contains("session store unavailable"));

        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn malformed_history_body_becomes_transport_error_state()
    -> Result<(), Box<dyn std::error::Error>> {
        // WHY(#7198): `skene::api::client::ApiClient::history` decodes via
        // `reqwest::Response::json`, which reports a body that fails to
        // deserialize as the same `ApiError::Http` variant as a dropped
        // connection -- see `session_load_state_for_error`'s doc comment.
        // This pins that behavior so a future skene change that starts
        // distinguishing the two (`ApiError::BadResponse`) is caught here
        // rather than silently changing which UI state operators see.
        install_crypto();
        let (server_url, server) =
            spawn_http_response("200 OK", r#"{"messages":[{"role":5}]}"#).await?;
        let client = skene::api::client::ApiClient::new(&server_url, None)?;

        let err = client
            .history("session-id", None, None)
            .await
            .expect_err("malformed history body should fail to decode");
        assert!(matches!(err, ApiError::Http { .. }));

        let state: SessionLoadState<SessionDetailStore> =
            session_load_state_for_error(err, "load session history");
        let SessionLoadState::TransportError(failure) = state else {
            panic!("malformed history body should become a transport error state");
        };
        assert!(!failure.message.is_empty());

        server.await??;
        Ok(())
    }

    #[test]
    fn legitimate_empty_sessions_response_sets_empty_state() {
        let envelope = skene::api::types::PaginatedSessionsResponse {
            items: vec![],
            has_more: false,
            next_cursor: None,
            total: Some(0),
        };
        let mut store = SessionListStore::new();
        store.load(envelope.items, envelope.has_more);
        store.total_count = envelope.total.and_then(|total| usize::try_from(total).ok());

        assert!(store.sessions.is_empty());
        assert_eq!(store.total_count, Some(0));
        assert!(matches!(store.load_state, SessionLoadState::Empty(())));
    }
}
