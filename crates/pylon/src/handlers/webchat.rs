//! Webchat compatibility endpoints for the Svelte web UI.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{instrument, warn};

use aletheia_hermeneus::anthropic::StreamEvent as LlmStreamEvent;
use aletheia_nous::stream::TurnStreamEvent;

use crate::error::{ApiError, BadRequestSnafu, InternalSnafu, NousNotFoundSnafu};
use crate::extract::OptionalClaims;
use crate::state::AppState;
use crate::stream::{TurnOutcome, WebchatEvent};

// --- POST /api/sessions/stream ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamRequest {
    pub agent_id: String,
    pub message: String,
    #[serde(default = "default_session_key")]
    pub session_key: String,
}

fn default_session_key() -> String {
    "main".to_owned()
}

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
        let store = state_clone.session_store.lock().map_err(|_poison| {
            InternalSnafu {
                message: "session store lock poisoned",
            }
            .build()
        })?;
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
        let store = state_clone.session_store.lock().map_err(|_poison| {
            InternalSnafu {
                message: "session store lock poisoned",
            }
            .build()
        })?;
        store
            .append_message(&sid, role, &content, None, None, token_estimate)
            .map_err(ApiError::from)
    })
    .await?
}

#[expect(
    clippy::too_many_lines,
    reason = "streaming bridge setup is inherently sequential"
)]
#[instrument(skip(state, _claims, body), fields(agent_id = %body.agent_id))]
pub async fn stream(
    State(state): State<Arc<AppState>>,
    _claims: OptionalClaims,
    Json(body): Json<StreamRequest>,
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
    // Each SSE endpoint serves a single client — no multi-subscriber broadcast.
    // Serialization happens once at the stream boundary (ReceiverStream::map).
    let bridge_tx = webchat_tx.clone();
    tokio::spawn(async move {
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

    // Run the turn and emit completion event
    tokio::spawn(async move {
        match handle
            .send_turn_streaming(&session_key, &message, nous_tx)
            .await
        {
            Ok(result) => {
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
                warn!(error = %err, "turn failed");
                let _ = webchat_tx
                    .send(WebchatEvent::Error {
                        message: err.to_string(),
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

// --- GET /api/agents ---

#[derive(Debug, Serialize)]
pub struct AgentsListResponse {
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub workspace: String,
    pub model: String,
}

pub async fn agents_list(
    State(state): State<Arc<AppState>>,
    _claims: OptionalClaims,
) -> Json<AgentsListResponse> {
    let config = state.config.read().await;
    let defaults = &config.agents.defaults;

    let agents = config
        .agents
        .list
        .iter()
        .map(|a| {
            let model = a
                .model
                .as_ref()
                .map_or_else(|| defaults.model.primary.clone(), |m| m.primary.clone());
            AgentInfo {
                id: a.id.clone(),
                name: a.name.clone().unwrap_or_else(|| a.id.clone()),
                workspace: a.workspace.clone(),
                model,
            }
        })
        .collect();

    Json(AgentsListResponse { agents })
}

// --- GET /api/agents/{id}/identity ---

#[derive(Debug, Serialize)]
pub struct AgentIdentityResponse {
    pub id: String,
    pub name: String,
    pub emoji: Option<String>,
}

#[instrument(skip(state, _claims))]
pub async fn agent_identity(
    State(state): State<Arc<AppState>>,
    _claims: OptionalClaims,
    Path(id): Path<String>,
) -> Result<Json<AgentIdentityResponse>, ApiError> {
    let config = state.config.read().await;
    let agent = config
        .agents
        .list
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| NousNotFoundSnafu { id: id.clone() }.build())?;

    let fallback_name = agent.name.clone().unwrap_or_else(|| id.clone());
    let workspace = std::path::Path::new(&agent.workspace);
    let identity_path = workspace.join("IDENTITY.md");

    let (name, emoji) = match tokio::fs::read_to_string(&identity_path).await {
        Ok(content) => parse_identity(&content, &fallback_name),
        Err(_) => (fallback_name, None),
    };

    Ok(Json(AgentIdentityResponse { id, name, emoji }))
}

fn parse_identity(content: &str, fallback_name: &str) -> (String, Option<String>) {
    let mut name = fallback_name.to_owned();
    let mut emoji = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            let val = val.trim();
            if !val.is_empty() {
                val.clone_into(&mut name);
            }
        } else if let Some(val) = line.strip_prefix("emoji:") {
            let val = val.trim();
            if !val.is_empty() {
                emoji = Some(val.to_owned());
            }
        }
    }

    (name, emoji)
}

// --- GET /api/branding ---

#[derive(Debug, Serialize)]
pub struct BrandingResponse {
    pub name: &'static str,
}

pub async fn branding(_claims: OptionalClaims) -> Json<BrandingResponse> {
    Json(BrandingResponse { name: "Aletheia" })
}

// --- GET /api/auth/mode ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthModeResponse {
    pub mode: &'static str,
    pub session_auth: bool,
}

pub async fn auth_mode(_claims: OptionalClaims) -> Json<AuthModeResponse> {
    Json(AuthModeResponse {
        mode: "token",
        session_auth: false,
    })
}

// --- GET /api/sessions ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsListQuery {
    pub nous_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionsListResponse {
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub nous_id: String,
    pub session_key: String,
    pub status: String,
    pub message_count: i64,
    pub updated_at: String,
}

#[instrument(skip(state, _claims))]
pub async fn sessions_list(
    State(state): State<Arc<AppState>>,
    _claims: OptionalClaims,
    Query(params): Query<SessionsListQuery>,
) -> Result<Json<SessionsListResponse>, ApiError> {
    let nous_id = params.nous_id;

    let state_clone = Arc::clone(&state);
    let sessions = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.lock().map_err(|_poison| {
            InternalSnafu {
                message: "session store lock poisoned",
            }
            .build()
        })?;
        store
            .list_sessions(nous_id.as_deref())
            .map_err(ApiError::from)
    })
    .await??;

    let items = sessions
        .into_iter()
        .map(|s| SessionInfo {
            id: s.id,
            nous_id: s.nous_id,
            session_key: s.session_key,
            status: s.status.as_str().to_owned(),
            message_count: s.message_count,
            updated_at: s.updated_at,
        })
        .collect();

    Ok(Json(SessionsListResponse { sessions: items }))
}

// --- GET /api/events ---

pub async fn events_sse(
    _claims: OptionalClaims,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<Event>(8);

    // Emit init event
    let init_data = serde_json::json!({"activeTurns": {}, "pendingDeliveries": 0}).to_string();
    let _ = tx
        .send(Event::default().event("init").data(init_data))
        .await;

    // Ping every 15 seconds
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_identity_extracts_name_and_emoji() {
        let content = "name: Syn\nemoji: x\nother: ignored\n";
        let (name, emoji) = parse_identity(content, "fallback");
        assert_eq!(name, "Syn");
        assert_eq!(emoji.as_deref(), Some("x"));
    }

    #[test]
    fn parse_identity_uses_fallback_when_empty() {
        let content = "emoji: x\n";
        let (name, _emoji) = parse_identity(content, "fallback");
        assert_eq!(name, "fallback");
    }

    #[test]
    fn default_session_key_is_main() {
        assert_eq!(default_session_key(), "main");
    }

    #[test]
    fn stream_request_deserializes_with_defaults() {
        let json = r#"{"agentId": "syn", "message": "hello"}"#;
        let req: StreamRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.agent_id, "syn");
        assert_eq!(req.message, "hello");
        assert_eq!(req.session_key, "main");
    }

    #[test]
    fn stream_request_deserializes_with_session_key() {
        let json = r#"{"agentId": "syn", "message": "hello", "sessionKey": "debug"}"#;
        let req: StreamRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.session_key, "debug");
    }

    #[test]
    fn branding_response_shape() {
        let resp = BrandingResponse { name: "Aletheia" };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["name"], "Aletheia");
    }

    #[test]
    fn auth_mode_response_shape() {
        let resp = AuthModeResponse {
            mode: "token",
            session_auth: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["mode"], "token");
        assert_eq!(json["sessionAuth"], false);
    }

    #[test]
    fn agents_list_response_serializes() {
        let resp = AgentsListResponse {
            agents: vec![AgentInfo {
                id: "syn".to_owned(),
                name: "Syn".to_owned(),
                workspace: "/tmp/syn".to_owned(),
                model: "anthropic/claude-opus-4-6".to_owned(),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["agents"][0]["id"], "syn");
        assert_eq!(json["agents"][0]["model"], "anthropic/claude-opus-4-6");
    }

    #[test]
    fn sessions_list_response_uses_camel_case() {
        let resp = SessionInfo {
            id: "abc".to_owned(),
            nous_id: "syn".to_owned(),
            session_key: "main".to_owned(),
            status: "active".to_owned(),
            message_count: 5,
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("nousId").is_some());
        assert!(json.get("sessionKey").is_some());
        assert!(json.get("messageCount").is_some());
        assert!(json.get("updatedAt").is_some());
    }
}
