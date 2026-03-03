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

use aletheia_mneme::types::SessionStatus;
use aletheia_nous::pipeline::TurnResult;

use crate::error::{
    ApiError, BadRequestSnafu, InternalSnafu, NousNotFoundSnafu, SessionNotFoundSnafu,
};
use crate::extract::Claims;
use crate::state::AppState;
use crate::stream::{SseEvent, UsageData};

/// POST /api/sessions — create a new session.
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

/// GET /api/sessions/{id} — get session state.
#[instrument(skip(state, _claims))]
pub async fn get_session(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<SessionResponse>, ApiError> {
    let session = find_session(&state, &id).await?;
    Ok(Json(SessionResponse::from_mneme(&session)))
}

/// DELETE /api/sessions/{id} — close (archive) a session.
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

/// GET /api/sessions/{id}/history — get conversation history.
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

/// POST /api/sessions/{id}/messages — send a message and stream the response via SSE.
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

    // Store the user message
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

                // TODO: Remove after finalize stage handles persistence
                // Store assistant response
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

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub nous_id: String,
    pub session_key: String,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    pub limit: Option<u32>,
    pub before: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub id: String,
    pub nous_id: String,
    pub session_key: String,
    pub status: String,
    pub model: Option<String>,
    pub message_count: i64,
    pub token_count_estimate: i64,
    pub created_at: String,
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

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub messages: Vec<HistoryMessage>,
}

#[derive(Debug, Serialize)]
pub struct HistoryMessage {
    pub id: i64,
    pub seq: i64,
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub created_at: String,
}
