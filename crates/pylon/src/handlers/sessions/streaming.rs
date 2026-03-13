//! SSE streaming handlers for session message delivery.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{Instrument, instrument, warn};

use aletheia_hermeneus::anthropic::StreamEvent as LlmStreamEvent;
use aletheia_nous::pipeline::TurnResult;
use aletheia_nous::stream::TurnStreamEvent;

use crate::error::{ApiError, BadRequestSnafu, ConflictSnafu, InternalSnafu, NousNotFoundSnafu};
use crate::extract::Claims;
use crate::idempotency::{LookupResult, MAX_KEY_LENGTH};
use crate::middleware::RequestId;
use crate::state::AppState;
use crate::stream::{SseEvent, TurnOutcome, UsageData, WebchatEvent};

use super::types::{SendMessageRequest, StreamTurnRequest};
use super::{find_session, resolve_session};

/// POST /api/v1/sessions/{id}/messages — send a message and stream the response via SSE.
#[utoipa::path(
    post,
    path = "/api/v1/sessions/{id}/messages",
    params(
        ("id" = String, Path, description = "Session ID"),
        ("Idempotency-Key" = Option<String>, Header, description = "Optional idempotency key (max 64 chars). Duplicate keys with a completed request return the cached response; duplicate keys with an in-flight request return 409 Conflict."),
    ),
    request_body = SendMessageRequest,
    responses(
        (status = 200, description = "SSE event stream", content_type = "text/event-stream"),
        (status = 400, description = "Bad request", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Session not found", body = crate::error::ErrorResponse),
        (status = 409, description = "Idempotency conflict — request still in flight", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[expect(
    clippy::too_many_lines,
    reason = "handler includes preflight checks, idempotency guard, and spawned turn task"
)]
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    headers: axum::http::HeaderMap,
    axum::extract::Extension(request_id): axum::extract::Extension<RequestId>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    // --- Idempotency key extraction ---
    let idempotency_key = extract_idempotency_key(&headers)?;

    if let Some(ref key) = idempotency_key {
        match state.idempotency_cache.check_or_insert(key) {
            LookupResult::Miss => { /* proceed normally */ }
            LookupResult::Hit { .. } => {
                // Return a minimal SSE stream indicating the request was already processed.
                tracing::info!(idempotency_key = %key, "idempotency cache hit — returning cached completion");
                let (tx, rx) = mpsc::channel::<SseEvent>(1);
                let _ = tx
                    .send(SseEvent::MessageComplete {
                        stop_reason: "idempotency_replay".to_owned(),
                        usage: UsageData {
                            input_tokens: 0,
                            output_tokens: 0,
                        },
                    })
                    .await;
                drop(tx);
                let stream = ReceiverStream::new(rx).map(sse_event_to_axum);
                return Ok(Sse::new(stream).keep_alive(
                    KeepAlive::new()
                        .interval(Duration::from_secs(15))
                        .text("ping"),
                ));
            }
            LookupResult::Conflict => {
                return Err(ConflictSnafu {
                    message: "Request with this idempotency key is still in progress",
                }
                .build());
            }
        }
    }

    let session = find_session(&state, &session_id).await?;
    let content = body.content;

    if content.is_empty() {
        return Err(BadRequestSnafu {
            message: "content must not be empty",
        }
        .build());
    }

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
    if let Some(config) = state.nous_manager.get_config(nous_id)
        && state
            .provider_registry
            .find_provider(&config.model)
            .is_none()
    {
        return Err(InternalSnafu {
            message: format!("no provider for model {}", config.model),
        }
        .build());
    }

    let session_key = session.session_key.clone();
    let (tx, rx) = mpsc::channel::<SseEvent>(32);
    let sid = session_id.clone();

    let idem_key = idempotency_key.clone();
    let idem_cache = Arc::clone(&state.idempotency_cache);

    let turn_span = tracing::info_span!(
        "send_turn",
        session.id = %session_id,
        session.key = %session_key,
        nous.id = %session.nous_id,
        request_id = %request_id.0,
        idempotency_key = idempotency_key.as_deref().unwrap_or(""),
    );
    tokio::spawn(
        async move {
            match handle
                .send_turn_with_session_id(
                    &session_key,
                    Some(sid.clone()),
                    &content,
                    aletheia_nous::handle::DEFAULT_SEND_TIMEOUT,
                )
                .await
            {
                Ok(result) => {
                    emit_turn_result_events(&tx, &result).await;

                    // Mark idempotency entry as completed so retries get a cache hit.
                    if let Some(ref key) = idem_key {
                        idem_cache.complete(key, axum::http::StatusCode::OK, String::new());
                    }
                }
                Err(err) => {
                    // Log full error internally; the active span carries request_id and
                    // session/nous context. Never forward internal details to the client. (#844)
                    tracing::error!(error = %err, "turn failed");

                    // Remove idempotency entry on error so the client can retry.
                    if let Some(ref key) = idem_key {
                        idem_cache.remove(key);
                    }

                    let _ = tx
                        .send(SseEvent::Error {
                            code: "turn_failed".to_owned(),
                            message: "An internal error occurred".to_owned(),
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
        }
        .instrument(turn_span),
    );

    let stream = ReceiverStream::new(rx).map(sse_event_to_axum);

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
#[utoipa::path(
    post,
    path = "/api/v1/sessions/stream",
    request_body(
        content = serde_json::Value,
        description = "Stream turn request: `{agentId, message, sessionKey?}`",
        content_type = "application/json"
    ),
    responses(
        (status = 200, description = "SSE event stream (WebchatEvent format)", content_type = "text/event-stream"),
        (status = 400, description = "Bad request", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Nous not found", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[expect(
    clippy::too_many_lines,
    reason = "streaming bridge setup is inherently sequential"
)]
#[instrument(skip(state, _claims, body), fields(agent_id = %body.agent_id))]
pub async fn stream_turn(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    axum::extract::Extension(request_id): axum::extract::Extension<RequestId>,
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

    let turn_span = tracing::info_span!(
        "stream_turn",
        session.id = %sid,
        session.key = %session_key,
        nous.id = %aid,
        request_id = %request_id.0,
    );

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
    tokio::spawn(
        async move {
            match handle
                .send_turn_streaming_with_session_id(
                    &session_key,
                    Some(sid.clone()),
                    &message,
                    nous_tx,
                    aletheia_nous::handle::DEFAULT_SEND_TIMEOUT,
                )
                .await
            {
                Ok(result) => {
                    // Wait for the bridge to finish forwarding all buffered deltas
                    // before sending turn_complete. This prevents the TUI from
                    // seeing turn_complete before the final text_delta events.
                    let _ = bridge_handle.await;

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
                }
                Err(err) => {
                    // Log full error internally; span carries session/nous context. (#844)
                    tracing::error!(error = %err, "streaming turn failed");
                    let _ = bridge_handle.await;
                    let _ = webchat_tx
                        .send(WebchatEvent::Error {
                            message: "An internal error occurred".to_owned(),
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
        }
        .instrument(turn_span),
    );

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
#[utoipa::path(
    get,
    path = "/api/v1/events",
    responses(
        (status = 200, description = "SSE event stream: `init`, `ping` events", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
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

/// Convert an [`SseEvent`] into an Axum SSE [`Event`].
///
/// Used as a named function (not closure) so that both the idempotency-replay
/// path and the normal streaming path produce the same `impl Stream` type.
#[expect(
    clippy::unnecessary_wraps,
    reason = "Result<_, Infallible> required by Stream<Item = Result<Event, Infallible>>"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "owned value received from Stream::map"
)]
fn sse_event_to_axum(event: SseEvent) -> Result<Event, Infallible> {
    let data = serde_json::to_string(&event).unwrap_or_else(|e| {
        warn!(error = %e, "failed to serialize SSE event");
        String::new()
    });
    Ok(Event::default().event(event.event_type()).data(data))
}

/// Extract and validate the optional `Idempotency-Key` header.
fn extract_idempotency_key(headers: &axum::http::HeaderMap) -> Result<Option<String>, ApiError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let key = value.to_str().map_err(|_non_ascii| {
        BadRequestSnafu {
            message: "Idempotency-Key header must be valid ASCII",
        }
        .build()
    })?;
    if key.is_empty() {
        return Err(BadRequestSnafu {
            message: "Idempotency-Key must not be empty",
        }
        .build());
    }
    if key.len() > MAX_KEY_LENGTH {
        return Err(BadRequestSnafu {
            message: format!("Idempotency-Key must be at most {MAX_KEY_LENGTH} characters"),
        }
        .build());
    }
    Ok(Some(key.to_owned()))
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
