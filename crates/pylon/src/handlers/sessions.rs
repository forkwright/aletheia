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

use aletheia_mneme::types::SessionStatus;
use aletheia_nous::pipeline::TurnResult;

use crate::error::{
    ApiError, BadRequestSnafu, ErrorResponse, InternalSnafu, NousNotFoundSnafu, SessionNotFoundSnafu,
};
use crate::extract::Claims;
use crate::state::AppState;
use crate::stream::{SseEvent, UsageData};

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
        let store = state_clone.session_store.lock().expect("store lock");
        store.find_or_create_session(&id_clone, &nid, &skey, Some(&model), None)
    })
    .await??;

    info!(session_id = %session.id, nous_id, "session created");

    Ok((
        StatusCode::CREATED,
        Json(SessionResponse::from_mneme(&session)),
    ))
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
    let _ = find_session(&state, &id).await?;

    let state_clone = Arc::clone(&state);
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.lock().expect("store lock");
        store.update_session_status(&id_clone, SessionStatus::Archived)
    })
    .await??;

    info!(session_id = %id, "session closed");
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
        let store = state_clone.session_store.lock().expect("store lock");
        store.get_history(&id_clone, limit.map(i64::from))
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
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_default();
        Ok(Event::default().event(event.event_type()).data(data))
    });

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

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
        let store = state_clone.session_store.lock().expect("store lock");
        store.append_message(&sid, role, &content, None, None, token_estimate)
    })
    .await?
    .map_err(ApiError::from)
}

async fn find_session(
    state: &Arc<AppState>,
    id: &str,
) -> Result<aletheia_mneme::types::Session, ApiError> {
    let state_clone = Arc::clone(state);
    let id_owned = id.to_owned();
    let id_for_error = id.to_owned();
    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.lock().expect("store lock");
        store.find_session_by_id(&id_owned)
    })
    .await??;

    session.ok_or_else(|| SessionNotFoundSnafu { id: id_for_error }.build())
}

// --- Request/Response types ---

/// Body for `POST /api/sessions`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSessionRequest {
    /// Target nous agent to bind the session to.
    pub nous_id: String,
    /// Client-chosen key for session deduplication.
    pub session_key: String,
}

/// Body for `POST /api/sessions/{id}/messages`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SendMessageRequest {
    /// User message text.
    pub content: String,
}

/// Query parameters for `GET /api/sessions/{id}/history`.
#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    /// Maximum number of messages to return.
    pub limit: Option<u32>,
    /// Return messages with `seq` strictly less than this value.
    pub before: Option<i64>,
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

/// Response for `GET /api/sessions/{id}/history`.
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
