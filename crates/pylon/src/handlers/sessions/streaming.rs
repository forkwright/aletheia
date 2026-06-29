//! SSE streaming handlers for session message delivery.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, KeepAliveStream, Sse};
use sha2::{Digest as _, Sha256};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::{IntervalStream, ReceiverStream};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, instrument, warn};

use hermeneus::anthropic::StreamEvent as LlmStreamEvent;
use hermeneus::provider::ProviderRoute;
use nous::pipeline::TurnResult;
use nous::stream::TurnStreamEvent;

use mneme::types::SessionStatus;

use symbolon::types::Role;
use taxis::config::AletheiaConfig;

use crate::error::{
    ApiError, BadRequestSnafu, ConflictSnafu, FieldError, InternalSnafu, NousNotFoundSnafu,
    StreamTurnConflictSnafu, ValidationFailedSnafu,
};
use crate::extract::{Claims, require_nous_access, require_role};
use crate::idempotency::LookupResult;
use crate::middleware::RequestId;
use crate::state::{EventBusState, SessionsState};
use crate::stream::{SseEvent, TurnOutcome, TurnStreamEvent as PylonTurnStreamEvent, UsageData};
use crate::turn_buffer::{
    REPLAY_GAP_REASON_BUFFER_CAPACITY, RecordOutcome, TURN_ABORT_REASON_CLIENT_DISCONNECT,
    TURN_ABORT_REASON_SERVER_SHUTDOWN, TURN_ABORT_REASON_TIMEOUT, TurnBufferHandle,
};

use super::types::{SendMessageRequest, StreamTurnRequest};
use super::{find_session, resolve_session};

const HEX_HIGH_NIBBLE_SHIFT: u8 = 4;
const HEX_LOW_NIBBLE_MASK: u8 = 0x0f;
const HEX_DECIMAL_DIGITS: u8 = 10;
const ASCII_DIGIT_ZERO: u8 = b'0';
const ASCII_LOWER_A: u8 = b'a';

type AxumEventStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<Event, Infallible>> + Send + 'static>>;
type BoxedSse = Sse<KeepAliveStream<AxumEventStream>>;

fn usage_data_from_provider(usage: hermeneus::types::Usage) -> UsageData {
    UsageData {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
    }
}

fn provider_stream_event_to_turn_event(event: LlmStreamEvent) -> PylonTurnStreamEvent {
    match event {
        LlmStreamEvent::TextDelta { text } => PylonTurnStreamEvent::TextDelta { text },
        LlmStreamEvent::ThinkingDelta { thinking } => {
            PylonTurnStreamEvent::ThinkingDelta { text: thinking }
        }
        LlmStreamEvent::InputJsonDelta { partial_json } => {
            PylonTurnStreamEvent::ProviderInputJsonDelta { partial_json }
        }
        LlmStreamEvent::ContentBlockStart { index, block_type } => {
            PylonTurnStreamEvent::ProviderContentBlockStart { index, block_type }
        }
        LlmStreamEvent::ContentBlockStop { index } => {
            PylonTurnStreamEvent::ProviderContentBlockStop { index }
        }
        LlmStreamEvent::MessageStart { usage } => PylonTurnStreamEvent::ProviderMessageStart {
            usage: usage_data_from_provider(usage),
        },
        LlmStreamEvent::MessageStop { stop_reason, usage } => {
            PylonTurnStreamEvent::ProviderMessageStop {
                stop_reason: stop_reason.as_str().to_owned(),
                usage: usage_data_from_provider(usage),
            }
        }
        LlmStreamEvent::UnsupportedEvent { event_type } => {
            PylonTurnStreamEvent::ProviderUnsupportedEvent { event_type }
        }
        _ => PylonTurnStreamEvent::ProviderUnsupportedEvent {
            event_type: "unknown".to_owned(),
        },
    }
}

fn boxed_event_stream<S>(stream: S) -> AxumEventStream
where
    S: tokio_stream::Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    Box::pin(stream)
}

/// Build an SSE [`KeepAlive`] using the configured gateway heartbeat interval.
///
/// WHY(#5156): All SSE streams share one keepalive cadence so the gateway,
/// clients, and reverse proxies agree on the transport contract.
async fn gateway_keepalive(
    config: &tokio::sync::RwLock<AletheiaConfig>,
    text: &'static str,
) -> KeepAlive {
    let secs = config.read().await.gateway.sse_heartbeat_interval_secs;
    KeepAlive::new()
        .interval(Duration::from_secs(secs))
        .text(text)
}

fn turn_complete_event_payload(
    session_id: &str,
    nous_id: &str,
    turn_id: &str,
    result: &TurnResult,
) -> serde_json::Value {
    serde_json::json!({
        "session_id": session_id,
        "nous_id": nous_id,
        "turn_id": turn_id,
        "input_tokens": result.usage.input_tokens,
        "output_tokens": result.usage.output_tokens,
        "cache_read_tokens": result.usage.cache_read_tokens,
        "cache_write_tokens": result.usage.cache_write_tokens,
        "stop_reason": result.stop_reason.as_str(),
        "model": result.model_used.as_str(),
        "provider": result.provider_used.as_deref(),
    })
}

/// Guard that aborts a spawned task and releases an in-flight idempotency key
/// when the SSE stream is dropped.
///
/// Stored alongside the SSE response stream so that when the client
/// disconnects and Axum drops the response future, the in-flight LLM
/// turn is cancelled rather than running to completion. The optional
/// idempotency guard is dropped after the task is aborted, cleaning up any
/// `InFlight` entry that the turn did not explicitly mark completed (#5453).
struct AbortOnDrop {
    task: tokio::task::JoinHandle<()>,
    turn_cancel: CancellationToken,
    _idem_guard: Option<IdempotencyGuard>,
    /// WHY(#4794): If the client drops the stream, mark the turn buffer terminal
    /// even if the spawned task is aborted before it can finish its own cleanup.
    /// The handle is cloned so the cleanup task can outlive the drop.
    turn_buffer: Option<TurnBufferHandle>,
    abort_reason: &'static str,
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        // WHY(#4794): Ensure the replay buffer never stays Running after the client
        // disconnects. A spawned task handles the mark so we do not block the
        // synchronous Drop impl on an async mutex.
        if let Some(ref handle) = self.turn_buffer {
            let handle = handle.clone();
            let reason = self.abort_reason;
            tokio::spawn(async move {
                handle.mark_aborted(reason).await;
            });
        }
        self.turn_cancel.cancel();
        self.task.abort();
    }
}

/// RAII guard that removes an in-flight idempotency entry on drop unless the
/// guarded turn has been explicitly marked completed.
///
/// WHY(#5453): A client disconnect aborts the SSE task; without this guard the
/// idempotency key would stay `InFlight` until TTL. The `completed` flag is
/// shared between the stream-side and task-side guards so a successfully
/// finished turn is never erased by the disconnect path.
struct IdempotencyGuard {
    cache: Arc<crate::idempotency::IdempotencyCache>,
    principal: String,
    key: String,
    session_id: String,
    body_fingerprint: String,
    completed: Arc<AtomicBool>,
}

impl IdempotencyGuard {
    #[cfg(test)]
    fn new(
        cache: Arc<crate::idempotency::IdempotencyCache>,
        principal: String,
        key: String,
        session_id: String,
        body_fingerprint: String,
    ) -> Self {
        Self {
            cache,
            principal,
            key,
            session_id,
            body_fingerprint,
            completed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create two guards sharing the same completion flag.
    ///
    /// WHY: one guard lives in the spawned turn task and marks completion; the
    /// other lives in `AbortOnDrop` and cleans up on disconnect.
    fn new_pair(
        cache: Arc<crate::idempotency::IdempotencyCache>,
        principal: String,
        key: String,
        session_id: String,
        body_fingerprint: String,
    ) -> (Self, Self) {
        let completed = Arc::new(AtomicBool::new(false));
        let task_guard = Self {
            cache: Arc::clone(&cache),
            principal: principal.clone(),
            key: key.clone(),
            session_id: session_id.clone(),
            body_fingerprint: body_fingerprint.clone(),
            completed: Arc::clone(&completed),
        };
        let stream_guard = Self {
            cache,
            principal,
            key,
            session_id,
            body_fingerprint,
            completed,
        };
        (task_guard, stream_guard)
    }

    fn mark_completed(&self) {
        self.completed.store(true, Ordering::Release);
    }
}

impl Drop for IdempotencyGuard {
    fn drop(&mut self) {
        if !self.completed.load(Ordering::Acquire) {
            self.cache.remove(
                &self.principal,
                &self.key,
                &self.session_id,
                &self.body_fingerprint,
            );
        }
    }
}

/// Stream wrapper that holds an `AbortOnDrop` guard alongside the inner stream.
///
/// When this stream is dropped (client disconnect), the guard aborts the
/// associated spawned task and releases any in-flight idempotency key. The
/// `Stream` impl delegates entirely to the inner stream.
///
/// WHY(#5165): Disconnect aborts the turn; reconnection replays buffered
/// events but does not resume the aborted turn. The guard ensures cleanup.
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
        ("Idempotency-Key" = Option<String>, Header, description = "Optional idempotency key (max 64 chars). Duplicate keys with the same resolved session and request body return the cached response; duplicate keys with an in-flight request, a different session, or a different request body return 409 Conflict."),
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
//
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response. Dropping the returned SSE
/// stream aborts the spawned turn task and cleans up the idempotency guard
/// (#5453).
#[expect(
    clippy::too_many_lines,
    reason = "handler includes preflight checks, idempotency guard, and spawned turn task"
)]
pub async fn send_message(
    State(state): State<SessionsState>,
    claims: Claims,
    headers: axum::http::HeaderMap,
    axum::extract::Extension(request_id): axum::extract::Extension<RequestId>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<BoxedSse, ApiError> {
    require_role(&claims, Role::Operator)?;

    let idempotency_key =
        extract_idempotency_key(&headers, state.idempotency_cache.max_key_length)?;

    let session = find_session(&state, &session_id).await?;
    require_nous_access(&claims, &session.nous_id)?;

    // WHY: archived sessions must not accept new messages (#1250).
    if session.status != SessionStatus::Active {
        return Err(ConflictSnafu {
            message: "cannot send message to a session that is not active",
        }
        .build());
    }

    let content = body.content;

    // WHY(#3275): field-level validation for send_message content.
    let max_msg_bytes = state.config.read().await.api_limits.max_message_bytes;
    if content.is_empty() {
        return Err(ValidationFailedSnafu {
            errors: vec![FieldError {
                field: "content".to_owned(),
                code: "required".to_owned(),
                message: "must not be empty".to_owned(),
            }],
        }
        .build());
    }
    if content.len() > max_msg_bytes {
        return Err(ValidationFailedSnafu {
            errors: vec![FieldError {
                field: "content".to_owned(),
                code: "too_long".to_owned(),
                message: format!("exceeds maximum size of {max_msg_bytes} bytes"),
            }],
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
            .resolve_provider(
                &config.generation.model,
                config
                    .generation
                    .provider
                    .as_deref()
                    .map_or(ProviderRoute::ModelOnly, ProviderRoute::Explicit),
            )
            .is_err()
    {
        let provider = config
            .generation
            .provider
            .as_deref()
            .map_or_else(String::new, |provider| format!(" via provider {provider}"));
        return Err(InternalSnafu {
            message: format!(
                "no provider for model {}{}",
                config.generation.model, provider
            ),
        }
        .build());
    }

    let body_fingerprint = send_message_body_fingerprint(&content);
    let turn_id = koina::ulid::Ulid::new().to_string();

    if let Some(ref key) = idempotency_key {
        match state.idempotency_cache.check_or_insert_with_in_flight_body(
            &claims.sub,
            key,
            &session_id,
            &body_fingerprint,
            Some(send_message_idempotency_body(&turn_id, "running")),
        ) {
            LookupResult::Miss => {}
            LookupResult::Hit { body, .. } => {
                let existing_turn_id =
                    cached_send_message_turn_id(&body).unwrap_or_else(|| turn_id.clone());
                tracing::info!(
                    idempotency_key = %key,
                    turn_id = %existing_turn_id,
                    "idempotency cache hit — replaying buffered turn stream"
                );
                if let Some(response) =
                    replay_buffered_sse_stream(&state, &session_id, &existing_turn_id).await
                {
                    return Ok(response);
                }
                return Err(StreamTurnConflictSnafu {
                    message: "idempotent turn exists but its replay buffer is no longer available",
                    turn_id: existing_turn_id,
                }
                .build());
            }
            LookupResult::Conflict { body } => {
                let existing_turn_id = body
                    .as_deref()
                    .and_then(cached_send_message_turn_id)
                    .unwrap_or_else(|| turn_id.clone());
                return Err(StreamTurnConflictSnafu {
                    message: "request with this idempotency key is still in progress",
                    turn_id: existing_turn_id,
                }
                .build());
            }
            LookupResult::Rejected { reason } => {
                return Err(ConflictSnafu {
                    message: reason.message(),
                }
                .build());
            }
        }
    }

    // WHY(#5453): Create the shared idempotency guard immediately after
    // inserting the `InFlight` entry so an early return (e.g. unknown session)
    // or a client disconnect cannot strand the key until TTL.
    let (idem_guard_task, idem_guard_stream) = match idempotency_key.as_ref() {
        Some(key) => {
            let (task_guard, stream_guard) = IdempotencyGuard::new_pair(
                Arc::clone(&state.idempotency_cache),
                claims.sub.clone(),
                key.clone(),
                session_id.clone(),
                body_fingerprint.clone(),
            );
            (Some(task_guard), Some(stream_guard))
        }
        None => (None, None),
    };

    let session_key = session.session_key.clone();
    // WHY(#3276): channel carries (seq, event) pairs so the stream mapper can
    // set the SSE `id:` field for Last-Event-ID client recovery.
    let (tx, rx) = mpsc::channel::<(u64, SseEvent)>(32);
    let sid = session_id.clone();

    let idem_key = idempotency_key.clone();
    let idem_principal = claims.sub.clone();
    let idem_session_id = session_id.clone();
    let idem_body_fingerprint = body_fingerprint.clone();
    let idem_cache = Arc::clone(&state.idempotency_cache);

    // WHY(#3276): Create a turn buffer so events emitted before a disconnect
    // survive and can be replayed on reconnect. The turn task is aborted on
    // disconnect, so only already-buffered events are available for replay.
    let turn_buf = state
        .turn_buffer_registry
        .get_or_create(&session_id, &turn_id)
        .await;
    let buf_handle = TurnBufferHandle::new(turn_buf);

    let request_id_str = request_id.0.clone();
    let turn_span = tracing::info_span!(
        "send_turn",
        session.id = %session_id,
        session.key = %session_key,
        nous.id = %session.nous_id,
        request_id = %request_id,
        turn_id = %turn_id,
        idempotency_key = idempotency_key.as_deref().unwrap_or(""),
    );
    let shutdown_token = state.shutdown.child_token();
    let turn_cancel = CancellationToken::new();
    let turn_cancel_task = turn_cancel.clone();
    let buf_handle_task = buf_handle.clone();
    let event_bus = Arc::clone(&state.event_bus);
    let turn_handle = tokio::spawn(
        async move {
            // WHY(#2113): Emit an immediate acknowledgment so the client never sees an empty
            // body, even if the turn fails before producing any content events.
            let event = SseEvent::MessageStart {
                status: "accepted".to_owned(),
                session_id: Some(sid.clone()),
                nous_id: Some(session.nous_id.clone()),
                turn_id: Some(turn_id.clone()),
                request_id: Some(request_id_str.clone()),
            };
            if let Some(recorded) = record_sse_event(&buf_handle_task, &event).await {
                let _ = tx.send(recorded).await;
            }

            // WHY(#4828): legacy message streaming has no approval endpoint wired.
            // Shared dispatch therefore executes None/Advisory tools and
            // policy-denies Required/Mandatory tools instead of silently approving.
            // WHY: cancel the in-flight turn when the server shuts down so Axum's graceful
            // shutdown can drain open SSE connections rather than hanging indefinitely (#1723).
            let turn_fut = handle.send_turn_with_cancel(
                &session_key,
                Some(sid.clone()),
                &content,
                nous::handle::DEFAULT_SEND_TIMEOUT,
                turn_cancel_task.clone(),
            );
            let result = tokio::select! {
                r = turn_fut => r,
                () = shutdown_token.cancelled() => {
                    tracing::info!("shutdown: cancelling in-flight SSE turn");
                    turn_cancel_task.cancel();
                    emit_turn_abort_sse(
                        &tx,
                        &buf_handle_task,
                        TURN_ABORT_REASON_SERVER_SHUTDOWN,
                        Some(&request_id_str),
                    )
                    .await;
                    return;
                }
            };
            match result {
                Ok(result) => {
                    emit_turn_result_events_buffered(
                        &tx,
                        &buf_handle_task,
                        &result,
                        Some(&request_id_str),
                    )
                    .await;
                    buf_handle_task.mark_completed().await;

                    event_bus
                        .publish(crate::event_bus::DomainEvent::new(
                            event_bus.next_id(),
                            "turn.complete",
                            turn_complete_event_payload(&sid, &session.nous_id, &turn_id, &result),
                        ))
                        .await;

                    // WHY(#4865): Store the canonical turn id, not a lossy
                    // completion summary. Duplicate completed requests replay
                    // the original buffered event sequence by this id.
                    if let Some(ref key) = idem_key {
                        idem_cache.complete(
                            &idem_principal,
                            key,
                            &idem_session_id,
                            &idem_body_fingerprint,
                            axum::http::StatusCode::OK,
                            send_message_idempotency_body(&turn_id, "completed"),
                        );
                    }
                    // WHY(#5453): Mark the idempotency guard completed so the shared
                    // stream-side guard does not delete the now-cached entry when the
                    // response is eventually dropped.
                    if let Some(ref guard) = idem_guard_task {
                        guard.mark_completed();
                    }
                }
                Err(err) => {
                    // WHY: Log full error internally; the active span carries request_id and
                    // session/nous context. Never forward internal details to the client (#844).
                    tracing::error!(error = %err, "turn failed");

                    // WHY: Remove idempotency entry on error so the client can retry.
                    if let Some(ref key) = idem_key {
                        idem_cache.remove(
                            &idem_principal,
                            key,
                            &idem_session_id,
                            &idem_body_fingerprint,
                        );
                    }

                    // WHY(#4794): Cancellations and timeouts are terminal aborts, not generic
                    // turn failures. Record the explicit reason so reconnect sees a terminal
                    // state instead of waiting forever on a Running buffer.
                    let is_abort = matches!(&err, nous::error::Error::TurnCancelled { .. })
                        || is_turn_timeout_error(&err);
                    if is_abort {
                        let reason = if matches!(&err, nous::error::Error::TurnCancelled { .. }) {
                            TURN_ABORT_REASON_CLIENT_DISCONNECT
                        } else {
                            TURN_ABORT_REASON_TIMEOUT
                        };
                        emit_turn_abort_sse(&tx, &buf_handle_task, reason, Some(&request_id_str))
                            .await;
                    }

                    let (err_code, err_message) = turn_error_info(&err);
                    let event = SseEvent::Error {
                        code: err_code,
                        message: err_message,
                        request_id: Some(request_id_str.clone()),
                    };
                    if let Some(recorded) = record_sse_event(&buf_handle_task, &event).await {
                        let _ = tx.send(recorded).await;
                    }
                    // WHY(#5164): Even when an `error` event is emitted, the following
                    // `message_complete` is the authoritative terminal marker. Clients must
                    // use `message_complete` to detect the end of the stream.
                    let event = SseEvent::MessageComplete {
                        stop_reason: "error".to_owned(),
                        usage: UsageData {
                            input_tokens: 0,
                            output_tokens: 0,
                            cache_read_tokens: 0,
                            cache_write_tokens: 0,
                        },
                        provider: None,
                        request_id: Some(request_id_str.clone()),
                    };
                    if let Some(recorded) = record_sse_event(&buf_handle_task, &event).await {
                        let _ = tx.send(recorded).await;
                    }
                    if !is_abort {
                        buf_handle_task.mark_failed().await;
                    }
                }
            }
        }
        .instrument(turn_span),
    );

    // WHY: Wrap the stream so the turn task is aborted when the client disconnects.
    // Without this, a disconnected client leaves the LLM inference running. The
    // idempotency guard is tied to the same lifetime so an in-flight key is not
    // stranded (#5453).
    let stream = GuardedStream {
        inner: ReceiverStream::new(rx).map(sse_event_to_axum_with_id),
        _guard: AbortOnDrop {
            task: turn_handle,
            turn_cancel,
            _idem_guard: idem_guard_stream,
            turn_buffer: Some(buf_handle.clone()),
            abort_reason: TURN_ABORT_REASON_CLIENT_DISCONNECT,
        },
    };

    Ok(Sse::new(boxed_event_stream(stream))
        .keep_alive(gateway_keepalive(&state.config, "ping").await))
}

/// POST /api/v1/sessions/stream: stream a conversation turn (turn stream protocol).
///
/// Accepts a `StreamTurnRequest` (`nous_id`, `message`, `session_key`, `client_turn_id`) and
/// returns SSE events in `TurnStreamEvent` format (used by TUI and desktop clients):
/// `message_start`, provider lifecycle events, `text_delta`, `thinking_delta`, `tool_use`,
/// `tool_result`, `message_complete`, `error`.
#[utoipa::path(
    post,
    path = "/api/v1/sessions/stream",
    request_body(
        content = serde_json::Value,
        description = "Stream turn request: `{nous_id, message, session_key?, client_turn_id?}`. `client_turn_id` is a client-generated ULID scoped to one user action for idempotency.",
        content_type = "application/json"
    ),
    responses(
        (status = 200, description = "SSE event stream (TurnStreamEvent format)", content_type = "text/event-stream"),
        (status = 400, description = "Bad request", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Nous not found", body = crate::error::ErrorResponse),
        (status = 409, description = "Stream turn idempotency conflict", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
// NOTE(#940): ~109 lines excluding match arms: sequential SSE bridge setup with
// turn spawn and completion event emission. Match arms inflate raw line count.
//
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response. Dropping the returned SSE
/// stream aborts the spawned streaming turn task (#5165).
#[expect(
    clippy::too_many_lines,
    reason = "streaming bridge setup is inherently sequential"
)]
#[instrument(skip(state, claims, body), fields(nous_id = %body.nous_id))]
pub async fn stream_turn(
    State(state): State<SessionsState>,
    claims: Claims,
    axum::extract::Extension(request_id): axum::extract::Extension<RequestId>,
    Json(body): Json<StreamTurnRequest>,
) -> Result<BoxedSse, ApiError> {
    require_role(&claims, Role::Operator)?;
    require_nous_access(&claims, &body.nous_id)?;
    let agent_id = body.nous_id;
    let message = body.message;
    let session_key = body.session_key;
    let client_turn_id = body.client_turn_id;

    // WHY(#3275): collect all field errors and return in one response.
    let api_limits = &state.config.read().await.api_limits;
    let max_msg_bytes = api_limits.max_message_bytes;
    let max_id_bytes = api_limits.max_identifier_bytes;

    let mut field_errors = Vec::new();
    if message.is_empty() {
        field_errors.push(FieldError {
            field: "message".to_owned(),
            code: "required".to_owned(),
            message: "must not be empty".to_owned(),
        });
    } else if message.len() > max_msg_bytes {
        field_errors.push(FieldError {
            field: "message".to_owned(),
            code: "too_long".to_owned(),
            message: format!("exceeds maximum size of {max_msg_bytes} bytes"),
        });
    }
    if agent_id.len() > max_id_bytes {
        field_errors.push(FieldError {
            field: "nous_id".to_owned(),
            code: "too_long".to_owned(),
            message: format!("exceeds maximum length of {max_id_bytes} bytes"),
        });
    }
    if session_key.len() > max_id_bytes {
        field_errors.push(FieldError {
            field: "session_key".to_owned(),
            code: "too_long".to_owned(),
            message: format!("exceeds maximum length of {max_id_bytes} bytes"),
        });
    }
    if let Some(ref id) = client_turn_id {
        if id.is_empty() {
            field_errors.push(FieldError {
                field: "client_turn_id".to_owned(),
                code: "required".to_owned(),
                message: "must not be empty".to_owned(),
            });
        } else if id.len() > max_id_bytes {
            field_errors.push(FieldError {
                field: "client_turn_id".to_owned(),
                code: "too_long".to_owned(),
                message: format!("exceeds maximum length of {max_id_bytes} bytes"),
            });
        } else if id.parse::<koina::ulid::Ulid>().is_err() {
            field_errors.push(FieldError {
                field: "client_turn_id".to_owned(),
                code: "invalid".to_owned(),
                message: "must be a valid ULID".to_owned(),
            });
        }
    }
    if !field_errors.is_empty() {
        return Err(ValidationFailedSnafu {
            errors: field_errors,
        }
        .build());
    }

    let turn_ulid = client_turn_id
        .as_deref()
        .and_then(|id| id.parse::<koina::ulid::Ulid>().ok())
        .unwrap_or_else(koina::ulid::Ulid::new);
    let turn_id = turn_ulid.to_string();
    let idempotency_key = client_turn_id
        .as_ref()
        .map(|_| stream_turn_idempotency_key(&turn_id));

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

    let configured_model = state
        .nous_manager
        .get_config(&agent_id)
        .map(|c| c.generation.model.clone());

    let session_id =
        resolve_session(&state, &agent_id, &session_key, configured_model.as_deref()).await?;

    let body_fingerprint = stream_turn_body_fingerprint(&agent_id, &session_key, &message);
    if let Some(ref key) = idempotency_key {
        match state.idempotency_cache.check_or_insert(
            &claims.sub,
            key,
            &session_id,
            &body_fingerprint,
        ) {
            LookupResult::Miss => {}
            LookupResult::Hit { body, .. } => {
                let existing_turn_id =
                    cached_stream_turn_id(&body).unwrap_or_else(|| turn_id.clone());
                tracing::info!(
                    client_turn_id = %existing_turn_id,
                    "stream turn idempotency cache hit"
                );
                if let Some(response) =
                    existing_turn_stream(&state, &session_id, &existing_turn_id).await
                {
                    return Ok(response);
                }
                return Err(StreamTurnConflictSnafu {
                    message: "stream turn already exists but its replay buffer is no longer available",
                    turn_id: existing_turn_id,
                }
                .build());
            }
            LookupResult::Conflict { body } => {
                let existing_turn_id = body
                    .as_deref()
                    .and_then(cached_stream_turn_id)
                    .unwrap_or_else(|| turn_id.clone());
                tracing::info!(
                    client_turn_id = %existing_turn_id,
                    "stream turn duplicate while original request is in flight"
                );
                if let Some(response) =
                    existing_turn_stream(&state, &session_id, &existing_turn_id).await
                {
                    return Ok(response);
                }
                return Err(StreamTurnConflictSnafu {
                    message: "stream turn already exists but is not ready for replay",
                    turn_id: existing_turn_id,
                }
                .build());
            }
            LookupResult::Rejected { reason } => {
                return Err(StreamTurnConflictSnafu {
                    message: reason.message(),
                    turn_id,
                }
                .build());
            }
        }
    }

    let stream_request_id = request_id.0.clone();
    // WHY(#3276): channel carries (seq, event) pairs for Last-Event-ID support.
    let (turn_tx, turn_rx) = mpsc::channel::<(u64, PylonTurnStreamEvent)>(32);
    let (nous_tx, mut nous_rx) = mpsc::channel::<TurnStreamEvent>(64);

    // WHY(#3958, ADR-005): create the approval channel and a turn guard so
    // tool_approval_required events can register exact `(turn_id, tool_id)`
    // senders. The guard removes only this turn's pending keys when the
    // streaming task ends; the gate itself defaults-deny on timeout, so a
    // dropped client connection denies pending Required/Mandatory tool calls
    // rather than letting them block the pipeline indefinitely.
    let (approval_tx, approval_rx) = mpsc::channel::<nous::approval::ApprovalDecision>(8);
    let approval_gate = Some(nous::approval::ApprovalGate::with_default_timeout(
        approval_rx,
    ));
    let approval_guard = state
        .approval_registry
        .register_turn(session_id.clone(), turn_id.clone());

    // WHY(#3276): Create a turn buffer so events survive client disconnection.
    let turn_buf = state
        .turn_buffer_registry
        .get_or_create(&session_id, &turn_id)
        .await;
    let buf_handle = TurnBufferHandle::new(turn_buf);

    let start_event = PylonTurnStreamEvent::MessageStart {
        session_id: session_id.clone(),
        nous_id: agent_id.clone(),
        turn_id: turn_id.clone(),
        request_id: Some(request_id.0.clone()),
    };
    if let Some(recorded) = record_turn_event(&buf_handle, &start_event).await {
        let _ = turn_tx.send(recorded).await;
    }

    let sid = session_id.clone();
    let aid = agent_id;

    let turn_span = tracing::info_span!(
        "stream_turn",
        session.id = %sid,
        session.key = %session_key,
        nous.id = %aid,
        request_id = %request_id,
        turn_id = %turn_id,
    );

    // WHY(#5727): Create the cancellation token before the bridge task so a
    // clone can be moved into the bridge. Cancelling it on client disconnect
    // causes the bridge to exit instead of continuing to drain queued events.
    let turn_cancel = CancellationToken::new();
    // WHY: Returns a JoinHandle so the turn task can wait for all deltas to drain
    // before emitting turn_complete (prevents the race where turn_complete
    // arrives at the TUI before the final text_delta events).
    let bridge_tx = turn_tx.clone();
    let bridge_buf = buf_handle.clone();
    let approval_registry = Arc::clone(&state.approval_registry);
    let approval_session_id = session_id.clone();
    let approval_turn_id = turn_id.clone();
    let approval_tx_for_bridge = approval_tx.clone();
    let turn_cancel_for_bridge = turn_cancel.clone();
    let bridge_handle = tokio::spawn(
        async move {
            loop {
                tokio::select! {
                    biased;
                    // WHY(#5727): Client disconnect cancels this token; stop
                    // consuming queued events so the bridge cannot register stale
                    // approval senders after the turn guard has dropped.
                    () = turn_cancel_for_bridge.cancelled() => break,
                    event = nous_rx.recv() => {
                        let Some(event) = event else { break; };
                        let turn_event = match event {
                            TurnStreamEvent::LlmDelta(llm_event) => {
                                provider_stream_event_to_turn_event(llm_event)
                            }
                            TurnStreamEvent::ToolStart {
                                tool_id,
                                tool_name,
                                input,
                            } => PylonTurnStreamEvent::ToolUse {
                                tool_name,
                                tool_id,
                                input,
                            },
                            TurnStreamEvent::ToolApprovalRequired {
                                turn_id: _nous_turn_id,
                                tool_id,
                                tool_name,
                                input,
                                risk,
                                reason,
                            } => PylonTurnStreamEvent::ToolApprovalRequired {
                                turn_id: {
                                    approval_registry
                                        .register_tool(
                                            &approval_session_id,
                                            &approval_turn_id,
                                            tool_id.clone(),
                                            approval_tx_for_bridge.clone(),
                                        )
                                        .await;
                                    approval_turn_id.clone()
                                },
                                tool_name,
                                tool_id,
                                input,
                                risk,
                                reason,
                            },
                            TurnStreamEvent::ToolApprovalResolved { tool_id, decision } => {
                                PylonTurnStreamEvent::ToolApprovalResolved { tool_id, decision }
                            }
                            TurnStreamEvent::ToolResult {
                                tool_id,
                                tool_name,
                                result,
                                is_error,
                                duration_ms,
                            } => PylonTurnStreamEvent::ToolResult {
                                tool_name,
                                tool_id,
                                result,
                                is_error,
                                duration_ms,
                            },
                            _ => PylonTurnStreamEvent::ProviderUnsupportedEvent {
                                event_type: "unknown_turn_stream_event".to_owned(),
                            },
                        };
                        let Some(recorded) = record_turn_event(&bridge_buf, &turn_event).await else {
                            continue;
                        };
                        if bridge_tx.send(recorded).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
        .instrument(tracing::info_span!("sse_bridge")),
    );

    let shutdown_token = state.shutdown.child_token();
    let turn_cancel_task = turn_cancel.clone();
    let buf_handle_task = buf_handle.clone();
    let event_bus = Arc::clone(&state.event_bus);
    let idem_key = idempotency_key.clone();
    let idem_principal = claims.sub.clone();
    let idem_session_id = session_id.clone();
    let idem_body_fingerprint = body_fingerprint.clone();
    let idem_cache = Arc::clone(&state.idempotency_cache);
    let idem_turn_id = turn_id.clone();
    let stream_turn_handle = tokio::spawn(
        async move {
            // WHY(#3958): hold the approval registry guard for the lifetime of
            // the streaming task so the session's sender stays registered
            // until the turn ends — then drops, unregistering it.
            let _approval_guard = approval_guard;
            // WHY: cancel the in-flight turn when the server shuts down so Axum's graceful
            // shutdown can drain open SSE connections rather than hanging indefinitely (#1723).
            let turn_fut = handle.send_turn_streaming_with_approval_and_turn_id(
                &session_key,
                Some(sid.clone()),
                &message,
                nous_tx,
                approval_gate,
                turn_ulid,
                nous::handle::DEFAULT_SEND_TIMEOUT,
                turn_cancel_task.clone(),
            );
            let result = tokio::select! {
                r = turn_fut => r,
                () = shutdown_token.cancelled() => {
                    tracing::info!("shutdown: cancelling in-flight streaming turn");
                    turn_cancel_task.cancel();
                    bridge_handle.abort();
                    emit_turn_abort_turn_stream(
                        &turn_tx,
                        &buf_handle_task,
                        TURN_ABORT_REASON_SERVER_SHUTDOWN,
                        Some(&stream_request_id),
                    )
                    .await;
                    return;
                }
            };
            match result {
                Ok(result) => {
                    // WHY: Wait for the bridge to finish forwarding all buffered deltas
                    // before sending turn_complete. This prevents the TUI from
                    // seeing turn_complete before the final text_delta events.
                    let _ = bridge_handle.await;

                    let event = PylonTurnStreamEvent::MessageComplete {
                        outcome: TurnOutcome {
                            text: result.content.clone(),
                            nous_id: aid.clone(),
                            session_id: sid.clone(),
                            model: Some(result.model_used.clone()),
                            provider: result.provider_used.clone(),
                            tool_calls: result.tool_calls.len(),
                            input_tokens: result.usage.input_tokens,
                            output_tokens: result.usage.output_tokens,
                            cache_read_tokens: result.usage.cache_read_tokens,
                            cache_write_tokens: result.usage.cache_write_tokens,
                            stop_reason: result.stop_reason.clone(),
                            error: None,
                        },
                    };
                    if let Some(recorded) = record_turn_event(&buf_handle_task, &event).await {
                        let _ = turn_tx.send(recorded).await;
                    }
                    buf_handle_task.mark_completed().await;

                    event_bus
                        .publish(crate::event_bus::DomainEvent::new(
                            event_bus.next_id(),
                            "turn.complete",
                            turn_complete_event_payload(&sid, &aid, &turn_id, &result),
                        ))
                        .await;

                    if let Some(ref key) = idem_key {
                        idem_cache.complete(
                            &idem_principal,
                            key,
                            &idem_session_id,
                            &idem_body_fingerprint,
                            axum::http::StatusCode::OK,
                            stream_turn_idempotency_body(&idem_turn_id, "completed"),
                        );
                    }
                }
                Err(err) => {
                    // WHY: Log full error internally; span carries session/nous context (#844).
                    tracing::error!(error = %err, "streaming turn failed");
                    let _ = bridge_handle.await;

                    // WHY(#4794): Cancellations and timeouts are terminal aborts. Record the
                    // explicit reason so reconnect sees a terminal state instead of hanging.
                    let is_abort = matches!(&err, nous::error::Error::TurnCancelled { .. })
                        || is_turn_timeout_error(&err);
                    if is_abort {
                        let reason = if matches!(&err, nous::error::Error::TurnCancelled { .. }) {
                            TURN_ABORT_REASON_CLIENT_DISCONNECT
                        } else {
                            TURN_ABORT_REASON_TIMEOUT
                        };
                        emit_turn_abort_turn_stream(
                            &turn_tx,
                            &buf_handle_task,
                            reason,
                            Some(&stream_request_id),
                        )
                        .await;
                    }

                    let (err_code, err_message) = turn_error_info(&err);
                    let event = PylonTurnStreamEvent::Error {
                        code: err_code,
                        message: err_message.clone(),
                        request_id: Some(stream_request_id.clone()),
                    };
                    if let Some(recorded) = record_turn_event(&buf_handle_task, &event).await {
                        let _ = turn_tx.send(recorded).await;
                    }
                    // WHY(#5164): Even when an `error` event is emitted, the following
                    // `message_complete` is the authoritative terminal marker. TUI clients
                    // must use `message_complete` to detect the end of the stream.
                    let event = PylonTurnStreamEvent::MessageComplete {
                        outcome: TurnOutcome {
                            text: String::new(),
                            nous_id: aid,
                            session_id: sid,
                            model: configured_model,
                            provider: None,
                            tool_calls: 0,
                            input_tokens: 0,
                            output_tokens: 0,
                            cache_read_tokens: 0,
                            cache_write_tokens: 0,
                            stop_reason: "error".to_owned(),
                            error: Some(err_message),
                        },
                    };
                    if let Some(recorded) = record_turn_event(&buf_handle_task, &event).await {
                        let _ = turn_tx.send(recorded).await;
                    }
                    if !is_abort {
                        buf_handle_task.mark_failed().await;
                    }
                    if let Some(ref key) = idem_key {
                        idem_cache.complete(
                            &idem_principal,
                            key,
                            &idem_session_id,
                            &idem_body_fingerprint,
                            axum::http::StatusCode::OK,
                            stream_turn_idempotency_body(&idem_turn_id, "failed"),
                        );
                    }
                }
            }
        }
        .instrument(turn_span),
    );

    // WHY: Abort streaming turn task when the client disconnects.
    let stream = GuardedStream {
        inner: ReceiverStream::new(turn_rx).map(|(seq, event)| {
            match serde_json::to_string(&event) {
                Ok(data) => Ok(Event::default()
                    .event(event.event_type())
                    .data(data)
                    .id(seq.to_string())),
                Err(e) => {
                    warn!(error = %e, "failed to serialize SSE event");
                    // WHY: Use the TurnStreamEvent::Error shape so the fallback event has the
                    // same structure as all other error events in the stream (#3160).
                    Ok(Event::default()
                        .event("error")
                        .data(
                            r#"{"type":"error","code":"serialization_error","message":"serialization failed"}"#,
                        )
                        .id(seq.to_string()))
                }
            }
        }),
        _guard: AbortOnDrop {
            task: stream_turn_handle,
            turn_cancel,
            _idem_guard: None,
            turn_buffer: Some(buf_handle.clone()),
            abort_reason: TURN_ABORT_REASON_CLIENT_DISCONNECT,
        },
    };

    Ok(Sse::new(boxed_event_stream(stream))
        .keep_alive(gateway_keepalive(&state.config, "heartbeat").await))
}

/// GET /api/v1/events: global SSE keep-alive channel.
///
/// Returns a persistent SSE connection with periodic keep-alive comments.
/// Domain-event broadcast is served separately by `GET /api/v1/events/subscribe`
/// (`handlers::events::subscribe` over the `EventBus`); this endpoint stays
/// comment-only.
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
    State(state): State<EventBusState>,
    _claims: Claims,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    // WHY: emit periodic comment-only events so the connection stays alive and
    // proxies do not close it. Domain events flow through the EventBus stream
    // on /api/v1/events/subscribe, not this channel.
    let heartbeat_secs = state
        .config
        .read()
        .await
        .gateway
        .sse_heartbeat_interval_secs;
    // WHY(#5156): Derive the interval from gateway config so the server
    // keepalive cadence stays in sync with the client read timeout and any
    // reverse-proxy idle timeouts.
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(heartbeat_secs)))
        .map(|_| Ok::<Event, Infallible>(Event::default().comment("keepalive")));

    Sse::new(stream).keep_alive(gateway_keepalive(&state.config, "ping").await)
}

/// Convert a `(seq, SseEvent)` pair into an Axum SSE [`Event`] with `id:` field.
///
/// WHY(#3276): The SSE `id:` field enables `Last-Event-ID` on reconnection.
/// The client's `EventSource` automatically sends `Last-Event-ID: N` when
/// reconnecting, and the server replays events from `N+1` onward.
#[expect(
    clippy::unnecessary_wraps,
    reason = "Result<_, Infallible> required by Stream<Item = Result<Event, Infallible>>"
)]
fn sse_event_to_axum_with_id((seq, event): (u64, SseEvent)) -> Result<Event, Infallible> {
    match serde_json::to_string(&event) {
        Ok(data) => Ok(Event::default()
            .event(event.event_type())
            .data(data)
            .id(seq.to_string())),
        Err(e) => {
            warn!(error = %e, "failed to serialize SSE event");
            Ok(Event::default()
                .event("error")
                .data(
                    r#"{"type":"error","code":"serialization_error","message":"serialization failed"}"#,
                )
                .id(seq.to_string()))
        }
    }
}

async fn existing_turn_stream(
    state: &SessionsState,
    session_id: &str,
    turn_id: &str,
) -> Option<BoxedSse> {
    let buf = state.turn_buffer_registry.get(session_id, turn_id).await?;
    let handle = TurnBufferHandle::new(buf);
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

    let shutdown_token = state.shutdown.child_token();
    let turn_cancel = CancellationToken::new();
    let task_cancel = turn_cancel.clone();
    let reconnect_task = tokio::spawn(reconnect_turn_task(
        tx,
        handle,
        0,
        shutdown_token,
        task_cancel,
        Duration::from_mins(5),
    ));

    let stream = GuardedStream {
        inner: ReceiverStream::new(rx),
        _guard: AbortOnDrop {
            task: reconnect_task,
            turn_cancel,
            _idem_guard: None,
            turn_buffer: None,
            abort_reason: "",
        },
    };

    Some(
        Sse::new(boxed_event_stream(stream))
            .keep_alive(gateway_keepalive(&state.config, "ping").await),
    )
}

async fn replay_buffered_sse_stream(
    state: &SessionsState,
    session_id: &str,
    turn_id: &str,
) -> Option<BoxedSse> {
    let buf = state.turn_buffer_registry.get(session_id, turn_id).await?;
    let handle = TurnBufferHandle::new(buf);
    let snapshot = handle.snapshot_after(0).await;
    if snapshot.events.is_empty() {
        return None;
    }

    let stream = tokio_stream::iter(snapshot.events.into_iter().map(|event| {
        Ok(Event::default()
            .event(event.event_type)
            .data(event.data)
            .id(event.seq.to_string()))
    }));

    Some(
        Sse::new(boxed_event_stream(stream))
            .keep_alive(gateway_keepalive(&state.config, "ping").await),
    )
}

fn send_message_body_fingerprint(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"send_message\0content\0");
    hasher.update(content.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(content.as_bytes());

    let digest = hasher.finalize();
    let mut hex = String::from("sha256:");
    for byte in digest {
        hex.push(lower_hex_char(byte >> HEX_HIGH_NIBBLE_SHIFT));
        hex.push(lower_hex_char(byte & HEX_LOW_NIBBLE_MASK));
    }
    hex
}

fn stream_turn_body_fingerprint(nous_id: &str, session_key: &str, message: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"stream_turn\0nous_id\0");
    hasher.update(nous_id.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(nous_id.as_bytes());
    hasher.update(b"\0session_key\0");
    hasher.update(session_key.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(session_key.as_bytes());
    hasher.update(b"\0message\0");
    hasher.update(message.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(message.as_bytes());

    let digest = hasher.finalize();
    let mut hex = String::from("sha256:");
    for byte in digest {
        hex.push(lower_hex_char(byte >> HEX_HIGH_NIBBLE_SHIFT));
        hex.push(lower_hex_char(byte & HEX_LOW_NIBBLE_MASK));
    }
    hex
}

fn stream_turn_idempotency_key(turn_id: &str) -> String {
    format!("stream_turn:{turn_id}")
}

fn send_message_idempotency_body(turn_id: &str, status: &str) -> String {
    turn_idempotency_body(turn_id, status)
}

fn stream_turn_idempotency_body(turn_id: &str, status: &str) -> String {
    turn_idempotency_body(turn_id, status)
}

fn turn_idempotency_body(turn_id: &str, status: &str) -> String {
    serde_json::json!({
        "turn_id": turn_id,
        "status": status,
    })
    .to_string()
}

fn cached_send_message_turn_id(body: &str) -> Option<String> {
    cached_turn_id(body)
}

fn cached_stream_turn_id(body: &str) -> Option<String> {
    cached_turn_id(body)
}

fn cached_turn_id(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("turn_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
}

fn lower_hex_char(nibble: u8) -> char {
    if nibble < HEX_DECIMAL_DIGITS {
        char::from(ASCII_DIGIT_ZERO + nibble)
    } else {
        char::from(ASCII_LOWER_A + (nibble - HEX_DECIMAL_DIGITS))
    }
}

fn sse_replay_gap_event(dropped_after_seq: u64, retained_limit: usize) -> SseEvent {
    SseEvent::ReplayGap {
        reason: REPLAY_GAP_REASON_BUFFER_CAPACITY.to_owned(),
        dropped_after_seq,
        retained_limit,
    }
}

fn turn_replay_gap_event(dropped_after_seq: u64, retained_limit: usize) -> PylonTurnStreamEvent {
    PylonTurnStreamEvent::ReplayGap {
        reason: REPLAY_GAP_REASON_BUFFER_CAPACITY.to_owned(),
        dropped_after_seq,
        retained_limit,
    }
}

/// Build a `turn_abort` SSE event for the legacy message stream protocol.
fn sse_turn_abort_event(reason: &str, request_id: Option<&str>) -> SseEvent {
    SseEvent::TurnAbort {
        reason: reason.to_owned(),
        request_id: request_id.map(ToOwned::to_owned),
    }
}

/// Build a `turn_abort` SSE event for the turn stream protocol.
fn turn_stream_turn_abort_event(reason: &str, request_id: Option<&str>) -> PylonTurnStreamEvent {
    PylonTurnStreamEvent::TurnAbort {
        reason: reason.to_owned(),
        request_id: request_id.map(ToOwned::to_owned),
    }
}

/// Record and emit a `turn_abort` event on the legacy message stream.
async fn emit_turn_abort_sse(
    tx: &mpsc::Sender<(u64, SseEvent)>,
    buf: &TurnBufferHandle,
    reason: &str,
    request_id: Option<&str>,
) {
    let event = sse_turn_abort_event(reason, request_id);
    if let Some(recorded) = record_sse_event(buf, &event).await {
        let _ = tx.send(recorded).await;
    }
    buf.mark_aborted(reason).await;
}

/// Record and emit a `turn_abort` event on the turn stream protocol.
async fn emit_turn_abort_turn_stream(
    tx: &mpsc::Sender<(u64, PylonTurnStreamEvent)>,
    buf: &TurnBufferHandle,
    reason: &str,
    request_id: Option<&str>,
) {
    let event = turn_stream_turn_abort_event(reason, request_id);
    if let Some(recorded) = record_turn_event(buf, &event).await {
        let _ = tx.send(recorded).await;
    }
    buf.mark_aborted(reason).await;
}

/// Return true if the turn error represents a time-limit exceeded condition.
fn is_turn_timeout_error(err: &nous::error::Error) -> bool {
    matches!(
        err,
        nous::error::Error::PipelineTimeout { .. } | nous::error::Error::AskTimeout { .. }
    )
}

/// Extract and validate the optional `Idempotency-Key` header.
fn extract_idempotency_key(
    headers: &axum::http::HeaderMap,
    max_key_length: usize,
) -> Result<Option<String>, ApiError> {
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
    if key.len() > max_key_length {
        return Err(BadRequestSnafu {
            message: format!("Idempotency-Key must be at most {max_key_length} characters"),
        }
        .build());
    }
    Ok(Some(key.to_owned()))
}

/// Categorize a nous turn error into a client-visible (code, message) pair.
///
/// Codes and messages identify the failure class without leaking internal
/// paths, SQL, or provider credentials. See #844 for the security rationale.
///
/// WHY `(String, String)`: dynamic messages let clients see the failure category
/// and a sanitized root cause without needing to parse server logs (#3162).
fn turn_error_info(err: &nous::error::Error) -> (String, String) {
    use nous::error::Error;

    if let Some(user_error) = nous::user_error::to_user_facing(err) {
        return (user_error.code().to_owned(), user_error.to_string());
    }

    match err {
        Error::PipelineTimeout {
            stage,
            timeout_secs,
            ..
        } => (
            "turn_timeout".to_owned(),
            format!("pipeline stage '{stage}' timed out after {timeout_secs}s"),
        ),
        Error::AskTimeout {
            nous_id,
            timeout_secs,
            ..
        } => (
            "turn_timeout".to_owned(),
            format!("cross-agent ask to '{nous_id}' timed out after {timeout_secs}s"),
        ),
        Error::GuardRejected { reason, .. } => (
            "guard_rejected".to_owned(),
            format!("request rejected by safety guard: {reason}"),
        ),
        Error::InboxFull { .. } | Error::ServiceDegraded { .. } => (
            "service_busy".to_owned(),
            "agent is temporarily unavailable".to_owned(),
        ),
        Error::ContextAssembly { message, .. } => (
            "context_error".to_owned(),
            format!("context assembly failed: {message}"),
        ),
        Error::ContextAssemblyIo { file, .. } => (
            "context_error".to_owned(),
            format!("context assembly failed: required file '{file}' unreadable"),
        ),
        Error::LoopDetected {
            iterations,
            pattern,
            ..
        } => (
            "loop_detected".to_owned(),
            format!("loop detected after {iterations} iterations: {pattern}"),
        ),
        Error::PipelineStage { stage, message, .. } => (
            "pipeline_error".to_owned(),
            format!("pipeline stage '{stage}' failed: {message}"),
        ),
        Error::PipelinePanic { .. } => (
            "pipeline_error".to_owned(),
            "pipeline encountered an unexpected internal error".to_owned(),
        ),
        Error::Llm { source, .. } => classify_llm_error(source),
        _ => (
            "turn_failed".to_owned(),
            "an internal error occurred".to_owned(),
        ),
    }
}

/// Map an LLM provider error to a client-visible (code, message) pair.
///
/// WHY: surface the failure category (timeout, auth, rate limit, model) so
/// clients can react programmatically. The original provider message is
/// included for non-sensitive errors; auth errors omit credential detail.
fn classify_llm_error(err: &hermeneus::error::Error) -> (String, String) {
    use hermeneus::error::Error;
    match err {
        Error::RateLimited { retry_after_ms, .. } => (
            "rate_limited".to_owned(),
            format!("rate limit exceeded, retry after {retry_after_ms}ms"),
        ),
        Error::ApiError { status, .. } if *status == 429 => {
            ("rate_limited".to_owned(), "rate limit exceeded".to_owned())
        }
        Error::AuthFailed { .. } => (
            "auth_failure".to_owned(),
            "provider authentication failed — run 'aletheia credential status' to diagnose"
                .to_owned(),
        ),
        Error::ApiError { status, .. } if *status == 503 || *status == 529 => (
            "provider_unavailable".to_owned(),
            format!("provider returned {status} — temporarily unavailable"),
        ),
        Error::ApiError {
            status, message, ..
        } if (400..500).contains(status) => (
            "invalid_request".to_owned(),
            format!(
                "provider rejected request ({status}): {}",
                redact_secrets(message)
            ),
        ),
        Error::ApiError {
            status, message, ..
        } if (500..600).contains(status) => (
            "provider_error".to_owned(),
            format!("provider error ({status}): {}", redact_secrets(message)),
        ),
        Error::UnsupportedModel { model, .. } => (
            "unsupported_model".to_owned(),
            format!("model '{model}' is not supported by this provider"),
        ),
        Error::ApiRequest { message, .. } => {
            let msg = message.to_lowercase();
            if msg.contains("timeout") {
                (
                    "provider_timeout".to_owned(),
                    format!("provider request timed out: {}", redact_secrets(message)),
                )
            } else {
                (
                    "provider_error".to_owned(),
                    format!("provider request failed: {}", redact_secrets(message)),
                )
            }
        }
        _ => (
            "provider_error".to_owned(),
            "an LLM provider error occurred".to_owned(),
        ),
    }
}

/// Regex matching common secret patterns in error messages.
///
/// WHY: match common key prefixes (sk-ant-, sk-, key-, bearer tokens)
/// and hex/base64 sequences that look like credentials (32+ chars).
#[expect(
    clippy::expect_used,
    reason = "compile-time-constant regex literals cannot fail"
)]
static SECRET_PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)(sk-ant-[a-zA-Z0-9_-]+|sk-[a-zA-Z0-9_-]{20,}|key-[a-zA-Z0-9_-]{20,}|bearer\s+[a-zA-Z0-9._-]{20,}|[a-f0-9]{40,})"
    )
    .expect("compile-time-constant regex literals cannot fail") // INVARIANT: regex literal is validated at compile time
});

/// Strip potential secrets (API keys, bearer tokens) from an error message
/// before including it in a client-visible SSE event.
///
/// WHY(#844): error messages from providers may contain credential fragments
/// in rejection messages (e.g. "invalid key sk-ant-..."). This strips
/// anything that looks like a secret token.
fn redact_secrets(msg: &str) -> String {
    SECRET_PATTERN.replace_all(msg, "[REDACTED]").into_owned()
}

/// Emit turn result as individual SSE events with buffer recording.
///
/// WHY(#3276): Each event is recorded in the turn buffer before being sent
/// to the client channel, so events survive client disconnection.
///
/// WHY(#3384): `request_id` is threaded through so `message_complete` carries it
/// for distributed tracing correlation on the client side.
async fn emit_turn_result_events_buffered(
    tx: &mpsc::Sender<(u64, SseEvent)>,
    buf: &TurnBufferHandle,
    result: &TurnResult,
    request_id: Option<&str>,
) {
    if !result.content.is_empty() {
        let event = SseEvent::TextDelta {
            text: result.content.clone(),
        };
        if let Some(recorded) = record_sse_event(buf, &event).await {
            let _ = tx.send(recorded).await;
        }
    }

    for tc in &result.tool_calls {
        let event = SseEvent::ToolUse {
            id: tc.id.clone(),
            name: tc.name.clone(),
            input: tc.input.clone(),
        };
        if let Some(recorded) = record_sse_event(buf, &event).await {
            let _ = tx.send(recorded).await;
        }

        if let Some(ref result_content) = tc.result {
            let event = SseEvent::ToolResult {
                tool_use_id: tc.id.clone(),
                content: result_content.clone(),
                is_error: tc.is_error,
            };
            if let Some(recorded) = record_sse_event(buf, &event).await {
                let _ = tx.send(recorded).await;
            }
        }
    }

    let event = SseEvent::MessageComplete {
        stop_reason: result.stop_reason.clone(),
        usage: UsageData {
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
            cache_read_tokens: result.usage.cache_read_tokens,
            cache_write_tokens: result.usage.cache_write_tokens,
        },
        provider: result.provider_used.clone(),
        request_id: request_id.map(ToOwned::to_owned),
    };
    if let Some(recorded) = record_sse_event(buf, &event).await {
        let _ = tx.send(recorded).await;
    }
}

/// Record an [`SseEvent`] to the turn buffer. Returns the retained event to send.
async fn record_sse_event(buf: &TurnBufferHandle, event: &SseEvent) -> Option<(u64, SseEvent)> {
    let event_type = event.event_type().to_owned();
    let data = serde_json::to_string(event).unwrap_or_default();
    match buf.record(&event_type, &data).await {
        RecordOutcome::Recorded { seq } => Some((seq, event.clone())),
        RecordOutcome::ReplayGap {
            seq,
            dropped_after_seq,
            retained_limit,
        } => Some((seq, sse_replay_gap_event(dropped_after_seq, retained_limit))),
        RecordOutcome::Dropped => None,
    }
}

/// Record a [`TurnStreamEvent`] to the turn buffer. Returns the retained event to send.
async fn record_turn_event(
    buf: &TurnBufferHandle,
    event: &PylonTurnStreamEvent,
) -> Option<(u64, PylonTurnStreamEvent)> {
    let event_type = event.event_type().to_owned();
    let data = serde_json::to_string(event).unwrap_or_default();
    match buf.record(&event_type, &data).await {
        RecordOutcome::Recorded { seq } => Some((seq, event.clone())),
        RecordOutcome::ReplayGap {
            seq,
            dropped_after_seq,
            retained_limit,
        } => Some((
            seq,
            turn_replay_gap_event(dropped_after_seq, retained_limit),
        )),
        RecordOutcome::Dropped => None,
    }
}

async fn reconnect_turn_task(
    tx: mpsc::Sender<Result<Event, Infallible>>,
    handle: TurnBufferHandle,
    last_event_id: u64,
    shutdown_token: CancellationToken,
    task_cancel: CancellationToken,
    max_live: Duration,
) {
    let mut last_seq = last_event_id;
    let initial = handle.snapshot_after(last_seq).await;
    let live = initial.state == crate::turn_buffer::TurnState::Running;
    let state_name = match initial.state {
        crate::turn_buffer::TurnState::Running => "running",
        crate::turn_buffer::TurnState::Completed => "completed",
        crate::turn_buffer::TurnState::Failed => "failed",
        crate::turn_buffer::TurnState::Aborted { .. } => "aborted",
    };
    let control_data = serde_json::json!({
        "type": "turn_reconnect_state",
        "state": state_name,
        "live": live,
    })
    .to_string();
    let control_event = Event::default()
        .event("turn_reconnect_state")
        .data(control_data);
    if tx.send(Ok(control_event)).await.is_err() {
        return;
    }

    // Replay and stream: snapshot_after is race-free (WHY: see #5453).
    let mut snapshot = initial;
    // WHY(#4794): clone tx so the post-timeout branch can still send the
    // turn_abort event after the inner async move consumes the original.
    let tx_timeout = tx.clone();
    // SAFETY: cancel-safe. timeout wraps a future and is cancel-safe.
    let timed_out = tokio::time::timeout(max_live, async move {
        loop {
            for event in snapshot.events {
                let sse_event = Event::default()
                    .event(event.event_type)
                    .data(event.data)
                    .id(event.seq.to_string());
                last_seq = event.seq;
                if tx.send(Ok(sse_event)).await.is_err() {
                    return;
                }
            }

            if snapshot.state != crate::turn_buffer::TurnState::Running {
                break;
            }

            tokio::select! {
                biased;
                // SAFETY: cancel-safe. CancellationToken::cancelled() is cancel-safe.
                () = shutdown_token.cancelled() => {
                    tracing::info!("shutdown: cancelling in-flight SSE reconnect");
                    let abort_data = serde_json::json!({
                        "type": "turn_abort",
                        "reason": TURN_ABORT_REASON_SERVER_SHUTDOWN,
                    })
                    .to_string();
                    let abort_event = Event::default()
                        .event("turn_abort")
                        .data(abort_data);
                    let _ = tx.send(Ok(abort_event)).await;
                    break;
                }
                // SAFETY: cancel-safe. CancellationToken::cancelled() is cancel-safe.
                () = task_cancel.cancelled() => break,
                // SAFETY: cancel-safe. Notified::notified() is cancel-safe.
                () = snapshot.notified => {}
            }
            snapshot = handle.snapshot_after(last_seq).await;
        }
    })
    .await;
    if timed_out.is_err() {
        tracing::warn!("reconnect_turn exceeded max live time; closing stream");
        let abort_data = serde_json::json!({
            "type": "turn_abort",
            "reason": TURN_ABORT_REASON_TIMEOUT,
        })
        .to_string();
        let abort_event = Event::default().event("turn_abort").data(abort_data);
        let _ = tx_timeout.send(Ok(abort_event)).await;
    }
}

/// Reconnect to a turn's SSE event stream.
///
/// Supports `Last-Event-ID` header for resuming from the last received event.
/// Replays buffered events after `Last-Event-ID`. If the original request is
/// still connected and the turn has not yet completed, failed, or aborted,
/// newly buffered events continue to stream until the turn finishes.
///
/// NOTE(#5165): Disconnecting the original `POST /sessions/{id}/messages`
/// request aborts the turn task and records a `turn_abort` event, so a
/// reconnect sees a terminal state instead of waiting indefinitely.
///
/// Returns 404 if the turn buffer has expired or was never created.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{session_id}/turns/{turn_id}/events",
    params(
        ("session_id" = String, Path, description = "Session ID"),
        ("turn_id" = String, Path, description = "Turn ID (from message_start event)"),
        ("Last-Event-ID" = Option<String>, Header, description = "Last received event sequence number for reconnection"),
    ),
    responses(
        (status = 200, description = "SSE event stream (replay + live)", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Turn not found or expired", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[instrument(skip(state, claims, headers))]
pub async fn reconnect_turn(
    State(state): State<SessionsState>,
    claims: Claims,
    headers: axum::http::HeaderMap,
    axum::extract::Path((session_id, turn_id)): axum::extract::Path<(String, String)>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let session = find_session(&state, &session_id).await?;
    require_role(&claims, Role::Operator)?;
    require_nous_access(&claims, &session.nous_id)?;

    // WHY: Parse Last-Event-ID from the standard SSE reconnection header.
    let last_event_id: u64 = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    debug!(
        session_id = %session_id,
        turn_id = %turn_id,
        last_event_id,
        "SSE reconnection request"
    );

    let buf = state
        .turn_buffer_registry
        .get(&session_id, &turn_id)
        .await
        .ok_or_else(|| {
            crate::error::SessionNotFoundSnafu {
                id: format!("{session_id}/turn/{turn_id}"),
            }
            .build()
        })?;

    let handle = TurnBufferHandle::new(buf);
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);

    // WHY(#5678): Wire shutdown so reconnect tasks do not outlive graceful
    // shutdown. The turn_cancel token is also passed into AbortOnDrop so
    // client disconnect aborts the task (mirrors the send_message pattern).
    let shutdown_token = state.shutdown.child_token();
    let turn_cancel = CancellationToken::new();
    let task_cancel = turn_cancel.clone();

    // WHY(#5678): Bound the task lifetime to the turn buffer TTL (5 min) so a
    // reconnect to an orphaned Running buffer cannot block indefinitely.
    let max_live = Duration::from_mins(5);

    let reconnect_task = tokio::spawn(reconnect_turn_task(
        tx,
        handle,
        last_event_id,
        shutdown_token,
        task_cancel,
        max_live,
    ));

    // WHY(#5678): GuardedStream aborts the reconnect task on client disconnect
    // and cancels `turn_cancel` so the loop exits cleanly (mirrors send_message).
    let stream = GuardedStream {
        inner: ReceiverStream::new(rx),
        _guard: AbortOnDrop {
            task: reconnect_task,
            turn_cancel,
            _idem_guard: None,
            turn_buffer: None,
            abort_reason: "",
        },
    };

    Ok(Sse::new(stream).keep_alive(gateway_keepalive(&state.config, "ping").await))
}

#[cfg(test)]
#[path = "streaming_tests.rs"]
mod streaming_tests;
