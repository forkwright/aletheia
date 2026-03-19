//! SSE streaming handlers for session message delivery.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::{IntervalStream, ReceiverStream};
use tracing::{Instrument, instrument, warn};

use aletheia_hermeneus::anthropic::StreamEvent as LlmStreamEvent;
use aletheia_nous::pipeline::TurnResult;
use aletheia_nous::stream::TurnStreamEvent;

use aletheia_mneme::types::SessionStatus;

use crate::error::{ApiError, BadRequestSnafu, ConflictSnafu, InternalSnafu, NousNotFoundSnafu};
use crate::extract::Claims;
use crate::idempotency::{LookupResult, MAX_KEY_LENGTH};
use crate::middleware::RequestId;
use crate::state::SessionsState;
use crate::stream::{SseEvent, TurnOutcome, UsageData, WebchatEvent};

use super::types::{SendMessageRequest, StreamTurnRequest};
use super::{find_session, resolve_session};

/// Guard that aborts a spawned task when dropped.
///
/// Stored alongside the SSE response stream so that when the client
/// disconnects and Axum drops the response future, the in-flight LLM
/// turn is cancelled rather than running to completion.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Stream wrapper that holds an `AbortOnDrop` guard alongside the inner stream.
///
/// When this stream is dropped (client disconnect), the guard aborts the
/// associated spawned task. The `Stream` impl delegates entirely to the
/// inner stream.
///
/// WHY: `Unpin` bound is sufficient because `ReceiverStream` and its
/// `Map` combinator both implement `Unpin`.
struct GuardedStream<S> {
    inner: S,
    _guard: AbortOnDrop,
}

impl<S: tokio_stream::Stream + Unpin> tokio_stream::Stream for GuardedStream<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.inner).poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

/// POST /api/v1/sessions/{id}/messages: send a message and stream the response via SSE.
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
// NOTE(#940): ~89 lines excluding match arms: single SSE handler with preflight checks,
// idempotency guard, and spawned turn task. The match arms account for the bulk of raw
// line count; the control flow is a single cohesive request lifecycle.
#[expect(
    clippy::too_many_lines,
    reason = "handler includes preflight checks, idempotency guard, and spawned turn task"
)]
pub async fn send_message(
    State(state): State<SessionsState>,
    _claims: Claims,
    headers: axum::http::HeaderMap,
    axum::extract::Extension(request_id): axum::extract::Extension<RequestId>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let idempotency_key = extract_idempotency_key(&headers)?;

    if let Some(ref key) = idempotency_key {
        match state.idempotency_cache.check_or_insert(key) {
            LookupResult::Miss => {}
            LookupResult::Hit { body, .. } => {
                tracing::info!(idempotency_key = %key, "idempotency cache hit — returning cached completion");
                // NOTE: Decode the cached turn summary stored by the original request.
                let cached: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                // serde_json::Value::Index returns Value::Null for absent keys (no panic)
                #[expect(
                    clippy::indexing_slicing,
                    reason = "serde_json::Value Index returns Null for absent keys, never panics"
                )]
                let stop_reason = cached["stop_reason"]
                    .as_str()
                    .unwrap_or("idempotency_replay")
                    .to_owned();
                #[expect(
                    clippy::indexing_slicing,
                    reason = "serde_json::Value Index returns Null for absent keys, never panics"
                )]
                let input_tokens = cached["input_tokens"].as_u64().unwrap_or(0);
                #[expect(
                    clippy::indexing_slicing,
                    reason = "serde_json::Value Index returns Null for absent keys, never panics"
                )]
                let output_tokens = cached["output_tokens"].as_u64().unwrap_or(0);

                let (tx, rx) = mpsc::channel::<SseEvent>(1);
                let _ = tx
                    .send(SseEvent::MessageComplete {
                        stop_reason,
                        usage: UsageData {
                            input_tokens,
                            output_tokens,
                        },
                    })
                    .await;
                drop(tx);
                let stream = GuardedStream {
                    inner: ReceiverStream::new(rx).map(sse_event_to_axum),
                    _guard: AbortOnDrop(tokio::spawn(async {})),
                };
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

    // WHY: archived sessions must not accept new messages (#1250).
    if session.status != SessionStatus::Active {
        return Err(ConflictSnafu {
            message: "cannot send message to a session that is not active",
        }
        .build());
    }

    let content = body.content;

    if content.is_empty() {
        return Err(BadRequestSnafu {
            message: "content must not be empty",
        }
        .build());
    }

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
        request_id = %request_id,
        idempotency_key = idempotency_key.as_deref().unwrap_or(""),
    );
    let shutdown_token = state.shutdown.child_token();
    let turn_handle = tokio::spawn(
        async move {
            // WHY: cancel the in-flight turn when the server shuts down so Axum's graceful
            // shutdown can drain open SSE connections rather than hanging indefinitely (#1723).
            let turn_fut = handle.send_turn_with_session_id(
                &session_key,
                Some(sid.clone()),
                &content,
                aletheia_nous::handle::DEFAULT_SEND_TIMEOUT,
            );
            let result = tokio::select! {
                r = turn_fut => r,
                () = shutdown_token.cancelled() => {
                    tracing::info!("shutdown: cancelling in-flight SSE turn");
                    return;
                }
            };
            match result {
                Ok(result) => {
                    emit_turn_result_events(&tx, &result).await;

                    // NOTE: Store the turn summary so cache-hit replays return real data.
                    if let Some(ref key) = idem_key {
                        let body = serde_json::json!({
                            "stop_reason": result.stop_reason,
                            "input_tokens": result.usage.input_tokens,
                            "output_tokens": result.usage.output_tokens,
                        })
                        .to_string();
                        idem_cache.complete(key, axum::http::StatusCode::OK, body);
                    }
                }
                Err(err) => {
                    // WHY: Log full error internally; the active span carries request_id and
                    // session/nous context. Never forward internal details to the client (#844).
                    tracing::error!(error = %err, "turn failed");

                    // WHY: Remove idempotency entry on error so the client can retry.
                    if let Some(ref key) = idem_key {
                        idem_cache.remove(key);
                    }

                    let (err_code, err_message) = turn_error_info(&err);
                    let _ = tx
                        .send(SseEvent::Error {
                            code: err_code.to_owned(),
                            message: err_message.to_owned(),
                        })
                        .await;
                    // WHY: Always send a completion marker so the client knows the
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

    // WHY: Wrap the stream so the turn task is aborted when the client disconnects.
    // Without this, a disconnected client leaves the LLM inference running.
    let stream = GuardedStream {
        inner: ReceiverStream::new(rx).map(sse_event_to_axum),
        _guard: AbortOnDrop(turn_handle),
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

/// POST /api/v1/sessions/stream: stream a conversation turn (TUI protocol).
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
// NOTE(#940): ~109 lines excluding match arms: sequential SSE bridge setup with
// turn spawn and completion event emission. Match arms inflate raw line count.
#[expect(
    clippy::too_many_lines,
    reason = "streaming bridge setup is inherently sequential"
)]
#[instrument(skip(state, _claims, body), fields(agent_id = %body.agent_id))]
pub async fn stream_turn(
    State(state): State<SessionsState>,
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
        request_id = %request_id,
    );

    // WHY: Returns a JoinHandle so the turn task can wait for all deltas to drain
    // before emitting turn_complete (prevents the race where turn_complete
    // arrives at the TUI before the final text_delta events).
    let bridge_tx = webchat_tx.clone();
    let bridge_handle = tokio::spawn(
        async move {
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
        }
        .instrument(tracing::info_span!("sse_bridge")),
    );

    let shutdown_token = state.shutdown.child_token();
    let stream_turn_handle = tokio::spawn(
        async move {
            // WHY: cancel the in-flight turn when the server shuts down so Axum's graceful
            // shutdown can drain open SSE connections rather than hanging indefinitely (#1723).
            let turn_fut = handle.send_turn_streaming_with_session_id(
                &session_key,
                Some(sid.clone()),
                &message,
                nous_tx,
                aletheia_nous::handle::DEFAULT_SEND_TIMEOUT,
            );
            let result = tokio::select! {
                r = turn_fut => r,
                () = shutdown_token.cancelled() => {
                    tracing::info!("shutdown: cancelling in-flight streaming turn");
                    return;
                }
            };
            match result {
                Ok(result) => {
                    // WHY: Wait for the bridge to finish forwarding all buffered deltas
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
                    // WHY: Log full error internally; span carries session/nous context (#844).
                    tracing::error!(error = %err, "streaming turn failed");
                    let _ = bridge_handle.await;
                    let (_, err_message) = turn_error_info(&err);
                    let _ = webchat_tx
                        .send(WebchatEvent::Error {
                            message: err_message.to_owned(),
                        })
                        .await;
                    // WHY: Always send a completion marker so the TUI knows the stream
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

    // WHY: Abort streaming turn task when the client disconnects.
    let stream = GuardedStream {
        inner: ReceiverStream::new(webchat_rx).map(|event| match serde_json::to_string(&event) {
            Ok(data) => Ok(Event::default().event(event.event_type()).data(data)),
            Err(e) => {
                warn!(error = %e, "failed to serialize SSE event");
                Ok(Event::default()
                    .event("error")
                    .data(r#"{"message":"serialization failed"}"#))
            }
        }),
        _guard: AbortOnDrop(stream_turn_handle),
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("heartbeat"),
    ))
}

/// GET /api/v1/events: global SSE keep-alive channel.
///
/// Returns a persistent SSE connection with periodic keep-alive comments.
/// Full server-side broadcast (system events, agent status changes) requires
/// wiring a `tokio::sync::broadcast` channel into `AppState`: that is tracked
/// in issue #1248 and is out of scope here.
#[utoipa::path(
    get,
    path = "/api/v1/events",
    responses(
        (status = 200, description = "Global SSE keep-alive stream", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn events(
    _claims: Claims,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    // WHY: emit periodic comment-only events so the connection stays alive and
    // proxies do not close it. Real domain events require a broadcast channel
    // wired into AppState: deferred to issue #1248.
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(30)))
        .map(|_| Ok::<Event, Infallible>(Event::default().comment("keepalive")));

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
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
    match serde_json::to_string(&event) {
        Ok(data) => Ok(Event::default().event(event.event_type()).data(data)),
        Err(e) => {
            warn!(error = %e, "failed to serialize SSE event");
            Ok(Event::default()
                .event("error")
                .data(r#"{"message":"serialization failed"}"#))
        }
    }
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

/// Categorize a nous turn error into a client-visible (code, message) pair.
///
/// Codes and messages identify the failure class without leaking internal
/// paths, SQL, or provider credentials. See #844 for the security rationale.
fn turn_error_info(err: &aletheia_nous::error::Error) -> (&'static str, &'static str) {
    use aletheia_nous::error::Error;
    match err {
        Error::PipelineTimeout { .. } | Error::AskTimeout { .. } => {
            ("turn_timeout", "turn timed out")
        }
        Error::GuardRejected { .. } => ("guard_rejected", "request rejected by safety guard"),
        Error::InboxFull { .. } | Error::ServiceDegraded { .. } => {
            ("service_busy", "agent is temporarily unavailable")
        }
        Error::Llm { source, .. } => classify_llm_error(source),
        _ => ("turn_failed", "An internal error occurred"),
    }
}

/// Map an LLM provider error to a client-visible (code, message) pair.
fn classify_llm_error(err: &aletheia_hermeneus::error::Error) -> (&'static str, &'static str) {
    use aletheia_hermeneus::error::Error;
    match err {
        Error::RateLimited { .. } => ("rate_limited", "rate limit exceeded"),
        Error::ApiError { status, .. } if *status == 429 => ("rate_limited", "rate limit exceeded"),
        Error::AuthFailed { .. } => ("provider_unavailable", "provider temporarily unavailable"),
        Error::ApiError { status, .. } if *status == 503 => {
            ("provider_unavailable", "provider temporarily unavailable")
        }
        _ => ("turn_failed", "An internal error occurred"),
    }
}

/// Emit turn result as individual SSE events to a single client channel.
///
/// Each SSE endpoint serves exactly one client: there is no multi-subscriber
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
