//! Session management and message streaming handlers.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::{info, instrument, warn};

use aletheia_hermeneus::types::{
    CompletionRequest, Content, ContentBlock, Message as LlmMessage, Role as LlmRole,
};
use aletheia_mneme::types::SessionStatus;

use crate::error::{ApiError, BadRequestSnafu, InternalSnafu, SessionNotFoundSnafu};
use crate::state::AppState;
use crate::stream::{SseEvent, UsageData};

/// POST /api/sessions — create a new session.
#[instrument(skip(state, body))]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let nous_id = body.nous_id;
    let session_key = body.session_key;

    let manager = &state.session_manager;
    let session_state = manager.create_session(&ulid::Ulid::new().to_string(), &session_key);

    let state_clone = Arc::clone(&state);
    let id = session_state.id.clone();
    let nid = session_state.nous_id.clone();
    let skey = session_key.clone();
    let model = session_state.model.clone();

    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.lock().expect("store lock");
        store.find_or_create_session(&id, &nid, &skey, Some(&model), None)
    })
    .await??;

    info!(session_id = %session.id, nous_id, "session created");

    Ok((StatusCode::CREATED, Json(SessionResponse::from_mneme(&session))))
}

/// GET /api/sessions/{id} — get session state.
#[instrument(skip(state))]
pub async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionResponse>, ApiError> {
    let session = find_session(&state, &id).await?;
    Ok(Json(SessionResponse::from_mneme(&session)))
}

/// DELETE /api/sessions/{id} — close (archive) a session.
#[instrument(skip(state))]
pub async fn close(
    State(state): State<Arc<AppState>>,
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
#[instrument(skip(state))]
pub async fn history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, ApiError> {
    let _ = find_session(&state, &id).await?;

    let state_clone = Arc::clone(&state);
    let id_clone = id.clone();
    let limit = params.limit;
    let messages = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.lock().expect("store lock");
        store.get_history(&id_clone, limit.map(|l| l as usize))
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
    store_message(&state, &session_id, aletheia_mneme::types::Role::User, &content, 0).await?;

    let model = session
        .model
        .clone()
        .unwrap_or_else(|| "claude-opus-4-20250514".to_owned());

    let request = build_completion_request(&state, &session_id, &model).await?;

    if state.provider_registry.find_provider(&model).is_none() {
        return Err(InternalSnafu {
            message: format!("no provider for model {model}"),
        }
        .build());
    }

    let (tx, rx) = mpsc::channel::<SseEvent>(32);
    let state_clone = Arc::clone(&state);
    let sid = session_id.clone();

    tokio::task::spawn_blocking(move || {
        run_completion(&state_clone, &model, &request, &tx, &sid);
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

async fn build_completion_request(
    state: &Arc<AppState>,
    session_id: &str,
    model: &str,
) -> Result<CompletionRequest, ApiError> {
    let state_clone = Arc::clone(state);
    let sid = session_id.to_owned();
    let history = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.lock().expect("store lock");
        store.get_history(&sid, Some(50))
    })
    .await??;

    let messages: Vec<LlmMessage> = history
        .iter()
        .filter_map(|m| {
            let role = match m.role {
                aletheia_mneme::types::Role::User => LlmRole::User,
                aletheia_mneme::types::Role::Assistant => LlmRole::Assistant,
                _ => return None,
            };
            Some(LlmMessage {
                role,
                content: Content::Text(m.content.clone()),
            })
        })
        .collect();

    Ok(CompletionRequest {
        model: model.to_owned(),
        system: None,
        messages,
        max_tokens: 4096,
        tools: state.tool_registry.to_hermeneus_tools(),
        temperature: None,
        thinking: None,
        stop_sequences: vec![],
    })
}

fn run_completion(
    state: &AppState,
    model: &str,
    request: &CompletionRequest,
    tx: &mpsc::Sender<SseEvent>,
    session_id: &str,
) {
    let Some(provider) = state.provider_registry.find_provider(model) else {
        let _ = tx.blocking_send(SseEvent::Error {
            code: "no_provider".to_owned(),
            message: format!("no provider for model {model}"),
        });
        return;
    };

    match provider.complete(request) {
        Ok(response) => {
            emit_response_events(tx, &response);

            let full_text: String = response
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");

            if let Ok(store) = state.session_store.lock() {
                let _ = store.append_message(
                    session_id,
                    aletheia_mneme::types::Role::Assistant,
                    &full_text,
                    None,
                    None,
                    i64::try_from(response.usage.output_tokens).unwrap_or(0),
                );
            }
        }
        Err(err) => {
            warn!(error = %err, "completion failed");
            let _ = tx.blocking_send(SseEvent::Error {
                code: "completion_failed".to_owned(),
                message: err.to_string(),
            });
        }
    }
}

fn emit_response_events(
    tx: &mpsc::Sender<SseEvent>,
    response: &aletheia_hermeneus::types::CompletionResponse,
) {
    for block in &response.content {
        let event = match block {
            ContentBlock::Text { text } => SseEvent::TextDelta {
                text: text.clone(),
            },
            ContentBlock::Thinking { thinking } => SseEvent::ThinkingDelta {
                thinking: thinking.clone(),
            },
            ContentBlock::ToolUse { id, name, input } => SseEvent::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            },
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => SseEvent::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: content.clone(),
                is_error: is_error.unwrap_or(false),
            },
            _ => continue,
        };
        let _ = tx.blocking_send(event);
    }

    let _ = tx.blocking_send(SseEvent::MessageComplete {
        stop_reason: format!("{:?}", response.stop_reason),
        usage: UsageData {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
        },
    });
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

    session.ok_or_else(|| {
        SessionNotFoundSnafu {
            id: id_for_error,
        }
        .build()
    })
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
