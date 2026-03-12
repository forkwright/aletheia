//! Session management and message streaming handlers.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{info, instrument, warn};
use utoipa::ToSchema;

use aletheia_hermeneus::anthropic::StreamEvent as LlmStreamEvent;
use aletheia_mneme::types::SessionStatus;
use aletheia_nous::pipeline::TurnResult;
use aletheia_nous::stream::TurnStreamEvent;

use crate::error::{
    ApiError, BadRequestSnafu, ErrorResponse, InternalSnafu, NousNotFoundSnafu,
    SessionNotFoundSnafu,
};
use crate::extract::Claims;
use crate::state::AppState;
use crate::stream::{SseEvent, TurnOutcome, UsageData, WebchatEvent};

/// POST /api/v1/sessions — create a new session.
#[utoipa::path(
    post,
    path = "/api/v1/sessions",
    request_body = CreateSessionRequest,
    responses(
        (status = 201, description = "Session created", body = SessionResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Nous not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, _claims, body))]
pub async fn create(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Json(body): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let nous_id = body.nous_id;
    let session_key = body.session_key;

    let config = state.nous_manager.get_config(&nous_id).ok_or_else(|| {
        NousNotFoundSnafu {
            id: nous_id.clone(),
        }
        .build()
    })?;

    let id = ulid::Ulid::new().to_string();
    let model = config.model.clone();

    let state_clone = Arc::clone(&state);
    let id_clone = id.clone();
    let nid = nous_id.clone();
    let skey = session_key.clone();

    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .find_or_create_session(&id_clone, &nid, &skey, Some(&model), None)
            .map_err(ApiError::from)
    })
    .await??;

    info!(session_id = %session.id, nous_id, "session created");

    Ok((
        StatusCode::CREATED,
        Json(SessionResponse::from_mneme(&session)),
    ))
}

/// GET /api/v1/sessions — list sessions, optionally filtered by agent.
#[instrument(skip(state, _claims))]
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<ListSessionsResponse>, ApiError> {
    let nous_id = params.nous_id;

    let state_clone = Arc::clone(&state);
    let sessions = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .list_sessions(nous_id.as_deref())
            .map_err(ApiError::from)
    })
    .await??;

    let items = sessions
        .into_iter()
        .map(|s| SessionListItem {
            id: s.id,
            nous_id: s.nous_id,
            session_key: s.session_key,
            status: s.status.as_str().to_owned(),
            message_count: s.message_count,
            updated_at: s.updated_at,
            display_name: s.display_name,
        })
        .collect();

    Ok(Json(ListSessionsResponse { sessions: items }))
}

/// GET /api/v1/sessions/{id} — get session state.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{id}",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 200, description = "Session details", body = SessionResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, _claims))]
pub async fn get_session(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<SessionResponse>, ApiError> {
    let session = find_session(&state, &id).await?;
    Ok(Json(SessionResponse::from_mneme(&session)))
}

/// DELETE /api/v1/sessions/{id} — close (archive) a session.
#[utoipa::path(
    delete,
    path = "/api/v1/sessions/{id}",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 204, description = "Session closed"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, _claims))]
pub async fn close(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    archive_session_by_id(&state, &id).await
}

/// POST /api/v1/sessions/{id}/archive — archive a session.
///
/// Same behavior as DELETE but via POST, matching the TUI's API contract.
#[instrument(skip(state, _claims))]
pub async fn archive(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    archive_session_by_id(&state, &id).await
}

/// Shared archive logic for both DELETE and POST archive routes.
async fn archive_session_by_id(state: &Arc<AppState>, id: &str) -> Result<StatusCode, ApiError> {
    let _ = find_session(state, id).await?;

    let state_clone = Arc::clone(state);
    let id_clone = id.to_owned();
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .update_session_status(&id_clone, SessionStatus::Archived)
            .map_err(ApiError::from)
    })
    .await??;

    info!(session_id = %id, "session archived");
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/sessions/{id}/unarchive — reactivate an archived session.
#[instrument(skip(state, _claims))]
pub async fn unarchive(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let _ = find_session(&state, &id).await?;

    let state_clone = Arc::clone(&state);
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .update_session_status(&id_clone, SessionStatus::Active)
            .map_err(ApiError::from)
    })
    .await??;

    info!(session_id = %id, "session unarchived");
    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/v1/sessions/{id}/name — rename a session.
#[instrument(skip(state, _claims, body))]
pub async fn rename(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<RenameSessionRequest>,
) -> Result<StatusCode, ApiError> {
    let _ = find_session(&state, &id).await?;

    if body.name.is_empty() {
        return Err(BadRequestSnafu {
            message: "name must not be empty",
        }
        .build());
    }

    let state_clone = Arc::clone(&state);
    let id_clone = id.clone();
    let name = body.name;
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .update_display_name(&id_clone, &name)
            .map_err(ApiError::from)
    })
    .await??;

    info!(session_id = %id, "session renamed");
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/sessions/{id}/history — get conversation history.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{id}/history",
    params(
        ("id" = String, Path, description = "Session ID"),
        ("limit" = Option<u32>, Query, description = "Maximum messages to return"),
        ("before" = Option<i64>, Query, description = "Return messages before this sequence number"),
    ),
    responses(
        (status = 200, description = "Conversation history", body = HistoryResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, _claims))]
pub async fn history(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, ApiError> {
    let _ = find_session(&state, &id).await?;

    let state_clone = Arc::clone(&state);
    let id_clone = id.clone();
    let limit = params.limit;
    let messages = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .get_history(&id_clone, limit.map(i64::from))
            .map_err(ApiError::from)
    })
    .await??;

    let mut items: Vec<HistoryMessage> = messages
        .into_iter()
        .map(|m| HistoryMessage {
            id: m.id,
            seq: m.seq,
            role: m.role.as_str().to_owned(),
            content: m.content,
            tool_call_id: m.tool_call_id,
            tool_name: m.tool_name,
            created_at: m.created_at,
        })
        .collect();

    if let Some(before) = params.before {
        items.retain(|m| m.seq < before);
    }

    Ok(Json(HistoryResponse { messages: items }))
}

/// POST /api/v1/sessions/{id}/messages — send a message and stream the response via SSE.
#[utoipa::path(
    post,
    path = "/api/v1/sessions/{id}/messages",
    params(("id" = String, Path, description = "Session ID")),
    request_body = SendMessageRequest,
    responses(
        (status = 200, description = "SSE event stream", content_type = "text/event-stream"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(session_id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let session = find_session(&state, &session_id).await?;
    let content = body.content;

    if content.is_empty() {
        return Err(BadRequestSnafu {
            message: "content must not be empty",
        }
        .build());
    }

    // Store user message under the pylon session ID (immediate feedback).
    store_message(
        &state,
        &session_id,
        aletheia_mneme::types::Role::User,
        &content,
        0,
    )
    .await?;

    // Resolve the nous actor
    let nous_id = &session.nous_id;
    let handle = state
        .nous_manager
        .get(nous_id)
        .ok_or_else(|| {
            InternalSnafu {
                message: format!("no actor for nous {nous_id}"),
            }
            .build()
        })?
        .clone();

    // Pre-flight: verify provider exists for the model
    if let Some(config) = state.nous_manager.get_config(nous_id) {
        if state
            .provider_registry
            .find_provider(&config.model)
            .is_none()
        {
            return Err(InternalSnafu {
                message: format!("no provider for model {}", config.model),
            }
            .build());
        }
    }

    let session_key = session.session_key.clone();
    let (tx, rx) = mpsc::channel::<SseEvent>(32);
    let state_clone = Arc::clone(&state);
    let sid = session_id.clone();

    tokio::spawn(async move {
        match handle.send_turn(&session_key, &content).await {
            Ok(result) => {
                emit_turn_result_events(&tx, &result).await;

                // Store assistant response under the pylon session ID.
                // NOTE: The finalize stage also persists to its own internal session ID
                // (generated by NousActor). Until session ID unification (WIRE-04+), we
                // need this manual store to keep pylon history consistent.
                let token_estimate = i64::try_from(result.usage.output_tokens).unwrap_or(0);
                let _ = store_message(
                    &state_clone,
                    &sid,
                    aletheia_mneme::types::Role::Assistant,
                    &result.content,
                    token_estimate,
                )
                .await;
            }
            Err(err) => {
                warn!(error = %err, "turn failed");
                let _ = tx
                    .send(SseEvent::Error {
                        code: "turn_failed".to_owned(),
                        message: err.to_string(),
                    })
                    .await;
                // Always send a completion marker so the client knows the
                // stream is finished, even on error paths.
                let _ = tx
                    .send(SseEvent::MessageComplete {
                        stop_reason: "error".to_owned(),
                        usage: UsageData {
                            input_tokens: 0,
                            output_tokens: 0,
                        },
                    })
                    .await;
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|e| {
            warn!(error = %e, "failed to serialize SSE event");
            String::new()
        });
        Ok(Event::default().event(event.event_type()).data(data))
    });

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

/// POST /api/v1/sessions/stream — stream a conversation turn (TUI protocol).
///
/// Accepts the webchat-style `StreamRequest` (agentId, message, sessionKey) and
/// returns SSE events in the `WebchatEvent` format that the TUI expects:
/// `turn_start`, `text_delta`, `thinking_delta`, `tool_start`, `tool_result`,
/// `turn_complete`, `error`.
#[expect(
    clippy::too_many_lines,
    reason = "streaming bridge setup is inherently sequential"
)]
#[instrument(skip(state, _claims, body), fields(agent_id = %body.agent_id))]
pub async fn stream_turn(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Json(body): Json<StreamTurnRequest>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let agent_id = body.agent_id;
    let message = body.message;
    let session_key = body.session_key;

    if message.is_empty() {
        return Err(BadRequestSnafu {
            message: "message must not be empty",
        }
        .build());
    }

    let handle = state
        .nous_manager
        .get(&agent_id)
        .ok_or_else(|| {
            NousNotFoundSnafu {
                id: agent_id.clone(),
            }
            .build()
        })?
        .clone();

    let model = state
        .nous_manager
        .get_config(&agent_id)
        .map(|c| c.model.clone());

    let session_id = resolve_session(&state, &agent_id, &session_key, model.as_deref()).await?;

    store_message(
        &state,
        &session_id,
        aletheia_mneme::types::Role::User,
        &message,
        0,
    )
    .await?;

    let turn_id = ulid::Ulid::new().to_string();
    let (webchat_tx, webchat_rx) = mpsc::channel::<WebchatEvent>(32);
    let (nous_tx, mut nous_rx) = mpsc::channel::<TurnStreamEvent>(64);

    let _ = webchat_tx
        .send(WebchatEvent::TurnStart {
            session_id: session_id.clone(),
            nous_id: agent_id.clone(),
            turn_id,
        })
        .await;

    let sid = session_id;
    let aid = agent_id;

    // Bridge nous stream events to webchat events in real-time.
    // Returns a JoinHandle so the turn task can wait for all deltas to drain
    // before emitting turn_complete (prevents the race where turn_complete
    // arrives at the TUI before the final text_delta events).
    let bridge_tx = webchat_tx.clone();
    let bridge_handle = tokio::spawn(async move {
        while let Some(event) = nous_rx.recv().await {
            let webchat_event = match event {
                TurnStreamEvent::LlmDelta(LlmStreamEvent::TextDelta { text }) => {
                    WebchatEvent::TextDelta { text }
                }
                TurnStreamEvent::LlmDelta(LlmStreamEvent::ThinkingDelta { thinking }) => {
                    WebchatEvent::ThinkingDelta { text: thinking }
                }
                TurnStreamEvent::ToolStart {
                    tool_id,
                    tool_name,
                    input,
                } => WebchatEvent::ToolStart {
                    tool_name,
                    tool_id,
                    input,
                },
                TurnStreamEvent::ToolResult {
                    tool_id,
                    tool_name,
                    result,
                    is_error,
                    duration_ms,
                } => WebchatEvent::ToolResult {
                    tool_name,
                    tool_id,
                    result,
                    is_error,
                    duration_ms,
                },
                _ => continue,
            };
            if bridge_tx.send(webchat_event).await.is_err() {
                break;
            }
        }
    });

    // Run the turn, wait for bridge to drain, then emit completion event.
    tokio::spawn(async move {
        match handle
            .send_turn_streaming(&session_key, &message, nous_tx)
            .await
        {
            Ok(result) => {
                // Wait for the bridge to finish forwarding all buffered deltas
                // before sending turn_complete. This prevents the TUI from
                // seeing turn_complete before the final text_delta events.
                let _ = bridge_handle.await;

                let token_estimate = i64::try_from(result.usage.output_tokens).unwrap_or(0);
                let _ = webchat_tx
                    .send(WebchatEvent::TurnComplete {
                        outcome: TurnOutcome {
                            text: result.content.clone(),
                            nous_id: aid,
                            session_id: sid.clone(),
                            model,
                            tool_calls: result.tool_calls.len(),
                            input_tokens: result.usage.input_tokens,
                            output_tokens: result.usage.output_tokens,
                            cache_read_tokens: result.usage.cache_read_tokens,
                            cache_write_tokens: result.usage.cache_write_tokens,
                        },
                    })
                    .await;
                let _ = store_message(
                    &state,
                    &sid,
                    aletheia_mneme::types::Role::Assistant,
                    &result.content,
                    token_estimate,
                )
                .await;
            }
            Err(err) => {
                warn!(error = %err, "streaming turn failed");
                let _ = bridge_handle.await;
                let _ = webchat_tx
                    .send(WebchatEvent::Error {
                        message: err.to_string(),
                    })
                    .await;
                // Always send a completion marker so the TUI knows the stream
                // is finished, even on error paths.
                let _ = webchat_tx
                    .send(WebchatEvent::TurnComplete {
                        outcome: TurnOutcome {
                            text: String::new(),
                            nous_id: aid,
                            session_id: sid,
                            model,
                            tool_calls: 0,
                            input_tokens: 0,
                            output_tokens: 0,
                            cache_read_tokens: 0,
                            cache_write_tokens: 0,
                        },
                    })
                    .await;
            }
        }
    });

    let stream = ReceiverStream::new(webchat_rx).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|e| {
            warn!(error = %e, "failed to serialize SSE event");
            String::new()
        });
        Ok(Event::default().event(event.event_type()).data(data))
    });

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("heartbeat"),
    ))
}

/// GET /api/v1/events — global SSE event channel.
///
/// Provides system-wide events for the TUI dashboard: turn lifecycle,
/// tool calls, status changes, and session events. Currently emits
/// `init` (with empty active turns) and periodic `ping` heartbeats.
pub async fn events(
    _claims: Claims,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<Event>(8);

    // Emit init event with empty active turns.
    let init_data = serde_json::json!({"activeTurns": [], "pendingDeliveries": 0}).to_string();
    let _ = tx
        .send(Event::default().event("init").data(init_data))
        .await;

    // Ping every 15 seconds to keep the connection alive.
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            if tx
                .send(Event::default().event("ping").data("{}"))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(Ok);

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("heartbeat"),
    )
}

/// Emit turn result as individual SSE events to a single client channel.
///
/// Each SSE endpoint serves exactly one client — there is no multi-subscriber
/// broadcast. Serialization happens once at the stream boundary (`ReceiverStream::map`).
async fn emit_turn_result_events(tx: &mpsc::Sender<SseEvent>, result: &TurnResult) {
    if !result.content.is_empty() {
        let _ = tx
            .send(SseEvent::TextDelta {
                text: result.content.clone(),
            })
            .await;
    }

    for tc in &result.tool_calls {
        let _ = tx
            .send(SseEvent::ToolUse {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: tc.input.clone(),
            })
            .await;
        if let Some(ref result_content) = tc.result {
            let _ = tx
                .send(SseEvent::ToolResult {
                    tool_use_id: tc.id.clone(),
                    content: result_content.clone(),
                    is_error: tc.is_error,
                })
                .await;
        }
    }

    let _ = tx
        .send(SseEvent::MessageComplete {
            stop_reason: result.stop_reason.clone(),
            usage: UsageData {
                input_tokens: result.usage.input_tokens,
                output_tokens: result.usage.output_tokens,
            },
        })
        .await;
}

/// Resolve or create a session for the given agent and session key.
async fn resolve_session(
    state: &Arc<AppState>,
    agent_id: &str,
    session_key: &str,
    model: Option<&str>,
) -> Result<String, ApiError> {
    let id = ulid::Ulid::new().to_string();
    let state_clone = Arc::clone(state);
    let id_clone = id.clone();
    let aid = agent_id.to_owned();
    let skey = session_key.to_owned();
    let model_owned = model.map(ToOwned::to_owned);

    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .find_or_create_session(&id_clone, &aid, &skey, model_owned.as_deref(), None)
            .map_err(ApiError::from)
    })
    .await??;

    Ok(session.id)
}

async fn store_message(
    state: &Arc<AppState>,
    session_id: &str,
    role: aletheia_mneme::types::Role,
    content: &str,
    token_estimate: i64,
) -> Result<i64, ApiError> {
    let state_clone = Arc::clone(state);
    let sid = session_id.to_owned();
    let content = content.to_owned();
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .append_message(&sid, role, &content, None, None, token_estimate)
            .map_err(ApiError::from)
    })
    .await?
}

async fn find_session(
    state: &Arc<AppState>,
    id: &str,
) -> Result<aletheia_mneme::types::Session, ApiError> {
    let state_clone = Arc::clone(state);
    let id_owned = id.to_owned();
    let id_for_error = id.to_owned();
    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store.find_session_by_id(&id_owned).map_err(ApiError::from)
    })
    .await??;

    session.ok_or_else(|| SessionNotFoundSnafu { id: id_for_error }.build())
}

/// Body for `POST /api/v1/sessions`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSessionRequest {
    /// Target nous agent to bind the session to.
    pub nous_id: String,
    /// Client-chosen key for session deduplication.
    pub session_key: String,
}

/// Body for `PUT /api/v1/sessions/{id}/name`.
#[derive(Debug, Deserialize)]
pub struct RenameSessionRequest {
    pub name: String,
}

/// Body for `POST /api/v1/sessions/{id}/messages`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SendMessageRequest {
    /// User message text.
    pub content: String,
}

/// Body for `POST /api/v1/sessions/stream` (TUI streaming protocol).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamTurnRequest {
    /// Target agent ID.
    #[serde(alias = "agentId")]
    pub agent_id: String,
    /// User message text.
    pub message: String,
    /// Session key for deduplication (defaults to "main").
    #[serde(alias = "sessionKey", default = "default_session_key")]
    pub session_key: String,
}

fn default_session_key() -> String {
    "main".to_owned()
}

/// Query parameters for `GET /api/v1/sessions`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsParams {
    /// Filter sessions by agent ID.
    pub nous_id: Option<String>,
}

/// Query parameters for `GET /api/v1/sessions/{id}/history`.
#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    /// Maximum number of messages to return.
    pub limit: Option<u32>,
    /// Return messages with `seq` strictly less than this value.
    pub before: Option<i64>,
}

/// Response for `GET /api/v1/sessions` (list).
#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionListItem>,
}

/// Session summary for list endpoints.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListItem {
    pub id: String,
    pub nous_id: String,
    pub session_key: String,
    pub status: String,
    pub message_count: i64,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Session metadata returned by create and get endpoints.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionResponse {
    /// Session identifier.
    pub id: String,
    /// Nous agent owning this session.
    pub nous_id: String,
    /// Client-chosen deduplication key.
    pub session_key: String,
    /// Lifecycle status (e.g. `"active"`, `"archived"`).
    pub status: String,
    /// LLM model used for this session, if set.
    pub model: Option<String>,
    /// Total messages stored in this session.
    pub message_count: i64,
    /// Estimated total tokens across all messages.
    pub token_count_estimate: i64,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-updated timestamp.
    pub updated_at: String,
}

impl SessionResponse {
    fn from_mneme(s: &aletheia_mneme::types::Session) -> Self {
        Self {
            id: s.id.clone(),
            nous_id: s.nous_id.clone(),
            session_key: s.session_key.clone(),
            status: s.status.as_str().to_owned(),
            model: s.model.clone(),
            message_count: s.message_count,
            token_count_estimate: s.token_count_estimate,
            created_at: s.created_at.clone(),
            updated_at: s.updated_at.clone(),
        }
    }
}

/// Response for `GET /api/v1/sessions/{id}/history`.
#[derive(Debug, Serialize, ToSchema)]
pub struct HistoryResponse {
    /// Conversation messages in chronological order.
    pub messages: Vec<HistoryMessage>,
}

/// A single message in the conversation history.
#[derive(Debug, Serialize, ToSchema)]
pub struct HistoryMessage {
    /// Database row ID.
    pub id: i64,
    /// Sequence number within the session.
    pub seq: i64,
    /// Message role (`"user"`, `"assistant"`, `"tool"`).
    pub role: String,
    /// Message text content.
    pub content: String,
    /// Tool call ID if this is a tool result message.
    pub tool_call_id: Option<String>,
    /// Tool name if this is a tool result message.
    pub tool_name: Option<String>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_response_serializes_all_fields() {
        let resp = SessionResponse {
            id: "01ABCDEF".to_owned(),
            nous_id: "alice".to_owned(),
            session_key: "main".to_owned(),
            status: "active".to_owned(),
            model: Some("anthropic/claude-opus-4-6".to_owned()),
            message_count: 3,
            token_count_estimate: 150,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T01:00:00Z".to_owned(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["id"], "01ABCDEF");
        assert_eq!(json["nous_id"], "alice");
        assert_eq!(json["message_count"], 3);
        assert_eq!(json["model"], "anthropic/claude-opus-4-6");
    }

    #[test]
    fn session_response_model_none_serializes_null() {
        let resp = SessionResponse {
            id: "01XYZ".to_owned(),
            nous_id: "bob".to_owned(),
            session_key: "main".to_owned(),
            status: "active".to_owned(),
            model: None,
            message_count: 0,
            token_count_estimate: 0,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["model"].is_null());
    }

    #[test]
    fn session_list_item_uses_camel_case() {
        let item = SessionListItem {
            id: "ses-1".to_owned(),
            nous_id: "alice".to_owned(),
            session_key: "debug".to_owned(),
            status: "active".to_owned(),
            message_count: 10,
            updated_at: "2026-03-01T00:00:00Z".to_owned(),
            display_name: None,
        };
        let json = serde_json::to_value(&item).unwrap();
        assert!(
            json.get("nousId").is_some(),
            "nous_id should be camelCase nousId"
        );
        assert!(
            json.get("sessionKey").is_some(),
            "session_key should be camelCase sessionKey"
        );
        assert!(
            json.get("messageCount").is_some(),
            "message_count should be camelCase messageCount"
        );
        assert!(
            json.get("updatedAt").is_some(),
            "updated_at should be camelCase updatedAt"
        );
        // display_name=None should be omitted (skip_serializing_if)
        assert!(
            json.get("displayName").is_none(),
            "None display_name should be omitted"
        );
    }

    #[test]
    fn session_list_item_includes_display_name_when_set() {
        let item = SessionListItem {
            id: "ses-2".to_owned(),
            nous_id: "alice".to_owned(),
            session_key: "main".to_owned(),
            status: "active".to_owned(),
            message_count: 0,
            updated_at: "2026-03-01T00:00:00Z".to_owned(),
            display_name: Some("My Session".to_owned()),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["displayName"], "My Session");
    }

    #[test]
    fn create_session_request_deserializes() {
        let json = r#"{"nous_id": "alice", "session_key": "work"}"#;
        let req: CreateSessionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.nous_id, "alice");
        assert_eq!(req.session_key, "work");
    }

    #[test]
    fn stream_turn_request_defaults_session_key_to_main() {
        let json = r#"{"agentId": "alice", "message": "hello"}"#;
        let req: StreamTurnRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.agent_id, "alice");
        assert_eq!(req.message, "hello");
        assert_eq!(req.session_key, "main");
    }

    #[test]
    fn stream_turn_request_accepts_explicit_session_key() {
        let json = r#"{"agentId": "alice", "message": "hi", "sessionKey": "debug"}"#;
        let req: StreamTurnRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.session_key, "debug");
    }

    #[test]
    fn history_response_serializes_messages_in_order() {
        let resp = HistoryResponse {
            messages: vec![
                HistoryMessage {
                    id: 1,
                    seq: 0,
                    role: "user".to_owned(),
                    content: "Hello".to_owned(),
                    tool_call_id: None,
                    tool_name: None,
                    created_at: "2026-01-01T00:00:00Z".to_owned(),
                },
                HistoryMessage {
                    id: 2,
                    seq: 1,
                    role: "assistant".to_owned(),
                    content: "Hi there".to_owned(),
                    tool_call_id: None,
                    tool_name: None,
                    created_at: "2026-01-01T00:00:01Z".to_owned(),
                },
            ],
        };
        let json = serde_json::to_value(&resp).unwrap();
        let msgs = json["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[0]["seq"], 0);
        assert_eq!(msgs[1]["seq"], 1);
    }

    #[test]
    fn history_before_filter_retains_earlier_messages() {
        // Verify the before-filter logic: messages with seq < before are kept
        let messages = [
            HistoryMessage {
                id: 1,
                seq: 0,
                role: "user".to_owned(),
                content: "first".to_owned(),
                tool_call_id: None,
                tool_name: None,
                created_at: "2026-01-01T00:00:00Z".to_owned(),
            },
            HistoryMessage {
                id: 2,
                seq: 5,
                role: "assistant".to_owned(),
                content: "fifth".to_owned(),
                tool_call_id: None,
                tool_name: None,
                created_at: "2026-01-01T00:00:05Z".to_owned(),
            },
        ];
        let before: i64 = 5;
        let filtered: Vec<_> = messages.iter().filter(|m| m.seq < before).collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].seq, 0);
    }

    #[test]
    fn session_not_found_error_is_404() {
        use crate::error::{ApiError, SessionNotFoundSnafu};
        use axum::response::IntoResponse;
        let err: ApiError = SessionNotFoundSnafu {
            id: "missing-session".to_owned(),
        }
        .build();
        let response = err.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn list_sessions_response_serializes() {
        let resp = ListSessionsResponse {
            sessions: vec![SessionListItem {
                id: "ses-1".to_owned(),
                nous_id: "alice".to_owned(),
                session_key: "main".to_owned(),
                status: "active".to_owned(),
                message_count: 2,
                updated_at: "2026-01-01T00:00:00Z".to_owned(),
                display_name: Some("Project Alpha".to_owned()),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(json["sessions"][0]["displayName"], "Project Alpha");
    }
}
