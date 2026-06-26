//! Global SSE connection to `GET /api/v1/events/subscribe`.
//!
//! Subscribes to the domain event stream for `fact.created`, `turn.complete`,
//! and `nous.lifecycle`, providing cross-session awareness for newly created
//! facts, completed turns, and agent lifecycle changes. The connection
//! auto-reconnects with exponential backoff (1s to 30s) and treats 45s of
//! *byte-level* silence as a stale connection (server keepalives are SSE comments
//! the parser never surfaces as events). Losses are reported to the UI only once
//! confirmed; clean reconnects are silent.
//!
//! # Dioxus integration
//!
//! In the TUI, `SseConnection::next()` feeds a `tokio::select!` loop.
//! In Dioxus, the pattern shifts to a background coroutine that writes
//! into signals:
//!
//! ```ignore
//! use_coroutine(|_rx| async move {
//!     let mut sse = SseConnection::connect(client, &base_url, cancel);
//!     while let Some(event) = sse.next().await {
//!         // write into Dioxus signals from here
//!     }
//! });
//! ```
//!
//! The `SseConnection` struct is intentionally framework-agnostic so it
//! works with both the TUI event loop and Dioxus coroutines.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures_util::StreamExt;
use reqwest::Client;
use skene::sse::SseStream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use skene::api::error::{format_error_fields_for_display, format_http_error_body};
use skene::api::types::SseEvent;
use skene::id::{NousId, SessionId, TurnId};

/// If no bytes arrive on the wire within this window, the connection is
/// treated as stale. The subscription stream emits domain events when they
/// occur and an SSE *comment* keepalive every 15s in between; the parser
/// swallows comment lines by design — liveness must therefore be judged at
/// the byte level, never by parsed-event arrival.
const HEARTBEAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// Initial backoff delay after a connection failure.
const INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);

/// Maximum backoff delay: caps exponential growth.
const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

/// Consecutive failed connect attempts required before the loss is reported.
const LOSS_CONFIRM_ATTEMPTS: u32 = 2;

/// Minimum elapsed time since the stream dropped before the loss is reported.
const LOSS_CONFIRM_WINDOW: std::time::Duration = std::time::Duration::from_secs(15);

/// Manages the global SSE connection to `/api/v1/events/subscribe`.
///
/// Runs in a background tokio task. Parsed domain events flow through an
/// mpsc channel. The connection automatically reconnects with exponential
/// backoff on failure and treats prolonged silence as disconnect.
///
/// Supports graceful shutdown via `CancellationToken`. When the token
/// fires, the background task exits cleanly.
pub(crate) struct SseConnection {
    rx: mpsc::Receiver<SseEvent>,
    _handle: tokio::task::JoinHandle<()>,
}

impl SseConnection {
    /// Connect using a shared HTTP client. Auth headers must already be
    /// embedded in the client. `Accept: text/event-stream` is set
    /// per-request to override any client-level JSON default.
    ///
    /// Connects to `/api/v1/events/subscribe` and filters for the domain
    /// topics `fact.created`, `turn.complete`, and `nous.lifecycle`. The
    /// returned `SseConnection` emits `Connected`/`Disconnected` lifecycle
    /// events in addition to parsed server events.
    #[tracing::instrument(skip_all)]
    pub(crate) fn connect(client: Client, base_url: &str, cancel: CancellationToken) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let url = format!(
            "{}/api/v1/events/subscribe?topics=fact.created,turn.complete,nous.lifecycle",
            base_url.trim_end_matches('/')
        );
        let child = cancel.child_token();

        let span = tracing::info_span!("sse_connection");
        let handle = tokio::spawn(run_sse_connection(client, url, child, tx).instrument(span));

        SseConnection {
            rx,
            _handle: handle,
        }
    }

    /// Receive the next parsed SSE event. Returns `None` when the
    /// connection task exits (shutdown or channel closed).
    pub async fn next(&mut self) -> Option<SseEvent> {
        self.rx.recv().await
    }
}

async fn run_sse_connection(
    client: Client,
    url: String,
    child: CancellationToken,
    tx: mpsc::Sender<SseEvent>,
) {
    let mut backoff = INITIAL_BACKOFF;
    // WHY: a dropped stream is reconnected silently; `Disconnected` is only
    // emitted once the loss is confirmed (LOSS_CONFIRM_ATTEMPTS failures
    // spanning LOSS_CONFIRM_WINDOW), so a clean reconnect never flips UI
    // state or fires lost/restored toast pairs.
    let mut lost_at: Option<Instant> = None;
    let mut failed_attempts: u32 = 0;

    loop {
        if child.is_cancelled() {
            return;
        }

        let resp = match tokio::select! {
            biased;
            _ = child.cancelled() => return,
            result = client
                .get(&url)
                .header("Accept", "text/event-stream")
                .send() => result,
        } {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("SSE connection failed: {e}");
                failed_attempts = failed_attempts.saturating_add(1);
                lost_at.get_or_insert_with(Instant::now);
                if loss_confirmed(failed_attempts, lost_at)
                    && tx.send(SseEvent::Disconnected).await.is_err()
                {
                    return;
                }
                tokio::select! {
                    biased;
                    _ = child.cancelled() => return,
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = advance_backoff(backoff);
                continue;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let reason = status.canonical_reason().unwrap_or("Unknown");
            let body = match resp.text().await {
                Ok(body) => body,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to read SSE error response body");
                    String::new()
                }
            };
            let message = extract_error_message(&body, status.as_u16(), reason);
            tracing::warn!("SSE error: {message}");
            failed_attempts = failed_attempts.saturating_add(1);
            lost_at.get_or_insert_with(Instant::now);
            if loss_confirmed(failed_attempts, lost_at)
                && tx.send(SseEvent::Disconnected).await.is_err()
            {
                return;
            }
            backoff = advance_backoff(backoff);
            tokio::select! {
                biased;
                _ = child.cancelled() => return,
                _ = tokio::time::sleep(backoff) => {}
            }
            continue;
        }

        if tx.send(SseEvent::Connected).await.is_err() {
            return;
        }
        tracing::info!("SSE connected");
        backoff = INITIAL_BACKOFF;
        failed_attempts = 0;

        // WHY: the subscription stream sends domain events as they occur and
        // a `: heartbeat` comment every 15s when idle. Heartbeat comments are
        // swallowed by the SSE parser per spec, so `es.next()` can stay
        // pending indefinitely on a perfectly healthy idle connection. Record
        // raw byte arrival; only byte-level silence past HEARTBEAT_TIMEOUT is
        // a dead link.
        let connected_at = Instant::now();
        let last_activity_ms = Arc::new(AtomicU64::new(0));
        let activity = Arc::clone(&last_activity_ms);
        let byte_stream = resp.bytes_stream().inspect(move |chunk| {
            if chunk.is_ok() {
                let elapsed = u64::try_from(connected_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                activity.store(elapsed, Ordering::Relaxed);
            }
        });
        let mut es = SseStream::new(byte_stream);

        loop {
            let maybe_event = tokio::select! {
                biased;
                _ = child.cancelled() => return,
                result = tokio::time::timeout(HEARTBEAT_TIMEOUT, es.next()) => result,
            };

            let event = match maybe_event {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_elapsed) => {
                    let now_ms =
                        u64::try_from(connected_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                    let idle_ms = now_ms.saturating_sub(last_activity_ms.load(Ordering::Relaxed));
                    if idle_ms < u64::try_from(HEARTBEAT_TIMEOUT.as_millis()).unwrap_or(u64::MAX) {
                        // NOTE: heartbeat comments are byte activity without
                        // parsed events; the link is alive — keep waiting.
                        continue;
                    }
                    tracing::warn!(
                        timeout_secs = HEARTBEAT_TIMEOUT.as_secs(),
                        idle_ms,
                        "SSE byte-level silence — treating as disconnect"
                    );
                    break;
                }
            };

            if let Some(parsed) = parse_sse_event(&event.event, &event.data)
                && tx.send(parsed).await.is_err()
            {
                // Receiver dropped: shut down.
                return;
            }
        }

        // NOTE: stream ended — begin a silent reconnect; only a confirmed
        // loss (see above) is reported to the UI.
        lost_at = Some(Instant::now());
        tracing::info!(
            backoff_secs = backoff.as_secs(),
            "SSE stream ended — reconnecting"
        );
        tokio::select! {
            biased;
            _ = child.cancelled() => return,
            // NOTE: backoff elapsed, retry connection
            _ = tokio::time::sleep(backoff) => {}
        }
    }
}

/// Whether a connection loss has persisted long enough to report.
///
/// Both gates must hold: at least [`LOSS_CONFIRM_ATTEMPTS`] consecutive
/// failed attempts AND [`LOSS_CONFIRM_WINDOW`] elapsed since the loss began.
/// Single blips and clean reconnects recover silently.
#[must_use]
fn loss_confirmed(failed_attempts: u32, lost_at: Option<Instant>) -> bool {
    failed_attempts >= LOSS_CONFIRM_ATTEMPTS
        && lost_at.is_some_and(|t| t.elapsed() >= LOSS_CONFIRM_WINDOW)
}

/// Advance exponential backoff: double the interval, capped at `MAX_BACKOFF`.
#[must_use]
fn advance_backoff(current: std::time::Duration) -> std::time::Duration {
    (current * 2).min(MAX_BACKOFF)
}

/// Extract a human-readable error message from an HTTP error response body.
fn extract_error_message(body: &str, status_code: u16, reason: &str) -> String {
    format_http_error_body(status_code, reason, body)
}

fn str_field<'a>(json: &'a serde_json::Value, field: &str, event_type: &str) -> Option<&'a str> {
    json.get(field).and_then(|v| v.as_str()).or_else(|| {
        tracing::warn!(event_type, field, "missing required field in SSE event");
        None
    })
}

fn u32_field(json: &serde_json::Value, field: &str, event_type: &str) -> Option<u32> {
    json.get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| {
            tracing::warn!(
                event_type,
                field,
                "missing or invalid u32 field in SSE event"
            );
            None
        })
}

fn bool_field(json: &serde_json::Value, field: &str, event_type: &str) -> Option<bool> {
    json.get(field)
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            tracing::warn!(
                event_type,
                field,
                "missing or invalid bool field in SSE event"
            );
            None
        })
}

fn parse_sse_event(event_type: &str, data: &str) -> Option<SseEvent> {
    let json: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event_type, error = %e, "failed to parse SSE event JSON");
            return None;
        }
    };

    match event_type {
        "init" => {
            let active_turns = json
                .get("activeTurns")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .or_else(|| {
                    tracing::warn!("SSE init: missing or invalid activeTurns");
                    None
                })?;
            Some(SseEvent::Init { active_turns })
        }
        "turn:before" => Some(SseEvent::TurnBefore {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            session_id: SessionId::from(str_field(&json, "sessionId", event_type)?.to_string()),
            turn_id: TurnId::from(str_field(&json, "turnId", event_type)?.to_string()),
        }),
        "turn:after" => Some(SseEvent::TurnAfter {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            session_id: SessionId::from(str_field(&json, "sessionId", event_type)?.to_string()),
        }),
        "tool:called" => Some(SseEvent::ToolCalled {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            tool_name: str_field(&json, "toolName", event_type)?.to_string(),
        }),
        "tool:failed" => Some(SseEvent::ToolFailed {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            tool_name: str_field(&json, "toolName", event_type)?.to_string(),
            error: json
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown")
                .to_string(),
        }),
        "status:update" => Some(SseEvent::StatusUpdate {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            status: str_field(&json, "status", event_type)?.to_string(),
        }),
        "session:created" => Some(SseEvent::SessionCreated {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            session_id: SessionId::from(str_field(&json, "sessionId", event_type)?.to_string()),
        }),
        "session:archived" => Some(SseEvent::SessionArchived {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            session_id: SessionId::from(str_field(&json, "sessionId", event_type)?.to_string()),
        }),
        "distill:before" => Some(SseEvent::DistillBefore {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
        }),
        "distill:stage" => Some(SseEvent::DistillStage {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            stage: str_field(&json, "stage", event_type)?.to_string(),
        }),
        "distill:after" => Some(SseEvent::DistillAfter {
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
        }),
        "turn.complete" => turn_complete_event(&json, event_type),
        "fact.created" => fact_created_event(&json, event_type),
        "nous.lifecycle" => nous_lifecycle_event(&json, event_type),
        "ping" => Some(SseEvent::Ping),
        "stream_lagged" => stream_lagged_event(&json),
        "error" => Some(SseEvent::Error {
            message: sse_error_message(&json),
        }),
        other => {
            tracing::debug!("unknown SSE event type: {other}");
            None
        }
    }
}

fn sse_error_message(json: &serde_json::Value) -> String {
    let message = json
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("server error");
    format_error_fields_for_display(
        message,
        None,
        json.get("code").and_then(serde_json::Value::as_str),
        json.get("request_id")
            .or_else(|| json.get("requestId"))
            .and_then(serde_json::Value::as_str),
        json.get("details"),
    )
}

fn stream_lagged_event(json: &serde_json::Value) -> Option<SseEvent> {
    let dropped = json
        .get("dropped")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            tracing::warn!("SSE stream_lagged: missing or invalid dropped");
            None
        })?;
    Some(SseEvent::StreamLagged { dropped })
}

fn turn_complete_event(json: &serde_json::Value, event_type: &str) -> Option<SseEvent> {
    Some(SseEvent::TurnComplete {
        nous_id: NousId::from(str_field(json, "nous_id", event_type)?.to_string()),
        session_id: SessionId::from(str_field(json, "session_id", event_type)?.to_string()),
        turn_id: TurnId::from(str_field(json, "turn_id", event_type)?.to_string()),
        input_tokens: u32_field(json, "input_tokens", event_type)?,
        output_tokens: u32_field(json, "output_tokens", event_type)?,
    })
}

fn fact_created_event(json: &serde_json::Value, event_type: &str) -> Option<SseEvent> {
    Some(SseEvent::FactCreated {
        fact_id: str_field(json, "fact_id", event_type)?.to_string(),
        nous_id: NousId::from(str_field(json, "nous_id", event_type)?.to_string()),
        content_preview: str_field(json, "content_preview", event_type)?.to_string(),
    })
}

fn nous_lifecycle_event(json: &serde_json::Value, event_type: &str) -> Option<SseEvent> {
    Some(SseEvent::NousLifecycle {
        nous_id: NousId::from(str_field(json, "nous_id", event_type)?.to_string()),
        event: str_field(json, "event", event_type)?.to_string(),
        restart_required: bool_field(json, "restart_required", event_type)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_turn_before_valid() {
        let data = r#"{"nousId":"syn","sessionId":"sess-1","turnId":"turn-1"}"#;
        let result = parse_sse_event("turn:before", data);
        assert!(result.is_some());
        if let Some(SseEvent::TurnBefore {
            nous_id,
            session_id,
            turn_id,
        }) = result
        {
            assert_eq!(&*nous_id, "syn");
            assert_eq!(&*session_id, "sess-1");
            assert_eq!(&*turn_id, "turn-1");
        } else {
            panic!("expected TurnBefore");
        }
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        let result = parse_sse_event("turn:before", "not json");
        assert!(result.is_none());
    }

    #[test]
    fn parse_missing_field_returns_none() {
        let data = r#"{"nousId":"syn"}"#;
        let result = parse_sse_event("turn:before", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_unknown_event_returns_none() {
        let data = r#"{"foo":"bar"}"#;
        let result = parse_sse_event("custom:unknown", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_init_with_active_turns() {
        let data = r#"{"activeTurns":[{"nousId":"syn","sessionId":"s1","turnId":"t1"}]}"#;
        let result = parse_sse_event("init", data);
        assert!(result.is_some());
        if let Some(SseEvent::Init { active_turns }) = result {
            assert_eq!(active_turns.len(), 1);
            assert_eq!(&*active_turns[0].nous_id, "syn");
        } else {
            panic!("expected Init");
        }
    }

    #[test]
    fn parse_ping() {
        let data = "{}";
        let result = parse_sse_event("ping", data);
        assert!(matches!(result, Some(SseEvent::Ping)));
    }

    #[test]
    fn parse_stream_lagged_valid() {
        let data = r#"{"dropped":42}"#;
        let result = parse_sse_event("stream_lagged", data);
        assert!(matches!(
            result,
            Some(SseEvent::StreamLagged { dropped: 42 })
        ));
    }

    #[test]
    fn parse_stream_lagged_missing_dropped_returns_none() {
        let result = parse_sse_event("stream_lagged", "{}");
        assert!(result.is_none());
    }

    #[test]
    fn parse_stream_lagged_invalid_dropped_returns_none() {
        let result = parse_sse_event("stream_lagged", r#"{"dropped":"many"}"#);
        assert!(result.is_none());
    }

    #[test]
    fn parse_error_preserves_code_request_id_and_details() {
        let data = r#"{"code":"stream_failed","message":"provider unavailable","request_id":"req-sse","details":{"provider":"synthetic"}}"#;
        let result = parse_sse_event("error", data);
        let Some(SseEvent::Error { message }) = result else {
            panic!("expected Error");
        };
        assert!(message.contains("provider unavailable"));
        assert!(message.contains("code stream_failed"));
        assert!(message.contains("request_id req-sse"));
        assert!(message.contains(r#""provider":"synthetic""#));
    }

    #[test]
    fn parse_tool_called() {
        let data = r#"{"nousId":"syn","toolName":"read_file"}"#;
        let result = parse_sse_event("tool:called", data);
        if let Some(SseEvent::ToolCalled { nous_id, tool_name }) = result {
            assert_eq!(&*nous_id, "syn");
            assert_eq!(tool_name, "read_file");
        } else {
            panic!("expected ToolCalled");
        }
    }

    #[test]
    fn parse_tool_failed_with_default_error() {
        let data = r#"{"nousId":"syn","toolName":"exec"}"#;
        let result = parse_sse_event("tool:failed", data);
        if let Some(SseEvent::ToolFailed {
            error, tool_name, ..
        }) = result
        {
            assert_eq!(tool_name, "exec");
            assert_eq!(error, "unknown");
        } else {
            panic!("expected ToolFailed");
        }
    }

    #[test]
    fn parse_distill_stage() {
        let data = r#"{"nousId":"syn","stage":"extracting"}"#;
        let result = parse_sse_event("distill:stage", data);
        if let Some(SseEvent::DistillStage { nous_id, stage }) = result {
            assert_eq!(&*nous_id, "syn");
            assert_eq!(stage, "extracting");
        } else {
            panic!("expected DistillStage");
        }
    }

    #[test]
    fn parse_session_created() {
        let data = r#"{"nousId":"syn","sessionId":"s-new"}"#;
        let result = parse_sse_event("session:created", data);
        if let Some(SseEvent::SessionCreated {
            nous_id,
            session_id,
        }) = result
        {
            assert_eq!(&*nous_id, "syn");
            assert_eq!(&*session_id, "s-new");
        } else {
            panic!("expected SessionCreated");
        }
    }

    #[test]
    fn parse_turn_complete_valid() {
        let data = r#"{"nous_id":"syn","session_id":"sess-1","turn_id":"turn-1","input_tokens":100,"output_tokens":50}"#;
        let result = parse_sse_event("turn.complete", data);
        if let Some(SseEvent::TurnComplete {
            nous_id,
            session_id,
            turn_id,
            input_tokens,
            output_tokens,
        }) = result
        {
            assert_eq!(&*nous_id, "syn");
            assert_eq!(&*session_id, "sess-1");
            assert_eq!(&*turn_id, "turn-1");
            assert_eq!(input_tokens, 100);
            assert_eq!(output_tokens, 50);
        } else {
            panic!("expected TurnComplete");
        }
    }

    #[test]
    fn parse_turn_complete_missing_tokens_returns_none() {
        let data = r#"{"nous_id":"syn","session_id":"sess-1","turn_id":"turn-1"}"#;
        assert!(parse_sse_event("turn.complete", data).is_none());
    }

    #[test]
    fn parse_fact_created_valid() {
        let data = r#"{"fact_id":"fact-1","nous_id":"syn","content_preview":"hello world"}"#;
        let result = parse_sse_event("fact.created", data);
        if let Some(SseEvent::FactCreated {
            fact_id,
            nous_id,
            content_preview,
        }) = result
        {
            assert_eq!(fact_id, "fact-1");
            assert_eq!(&*nous_id, "syn");
            assert_eq!(content_preview, "hello world");
        } else {
            panic!("expected FactCreated");
        }
    }

    #[test]
    fn parse_fact_created_missing_field_returns_none() {
        let data = r#"{"fact_id":"fact-1","nous_id":"syn"}"#;
        assert!(parse_sse_event("fact.created", data).is_none());
    }

    #[test]
    fn parse_nous_lifecycle_valid() {
        let data = r#"{"nous_id":"syn","event":"created","restart_required":true}"#;
        let result = parse_sse_event("nous.lifecycle", data);
        if let Some(SseEvent::NousLifecycle {
            nous_id,
            event,
            restart_required,
        }) = result
        {
            assert_eq!(&*nous_id, "syn");
            assert_eq!(event, "created");
            assert!(restart_required);
        } else {
            panic!("expected NousLifecycle");
        }
    }

    #[test]
    fn parse_nous_lifecycle_missing_event_returns_none() {
        let data = r#"{"nous_id":"syn","restart_required":false}"#;
        assert!(parse_sse_event("nous.lifecycle", data).is_none());
    }

    #[test]
    fn advance_backoff_doubles() {
        let b = advance_backoff(std::time::Duration::from_secs(1));
        assert_eq!(b, std::time::Duration::from_secs(2));
    }

    #[test]
    fn advance_backoff_caps_at_max() {
        let b = advance_backoff(std::time::Duration::from_secs(20));
        assert_eq!(b, MAX_BACKOFF);
    }

    #[test]
    fn extract_error_message_json() {
        let body = r#"{"message":"rate limited"}"#;
        assert_eq!(
            extract_error_message(body, 429, "Too Many Requests"),
            "rate limited"
        );
    }

    #[test]
    fn extract_error_message_fallback() {
        assert_eq!(
            extract_error_message("not json", 500, "Internal"),
            "500 Internal"
        );
    }

    #[test]
    fn extract_error_message_error_field() {
        let body = r#"{"error":"forbidden"}"#;
        assert_eq!(extract_error_message(body, 403, "Forbidden"), "forbidden");
    }

    #[test]
    fn extract_error_message_preserves_pylon_envelope() {
        let body = r#"{"error":{"code":"validation_error","message":"invalid subscription","request_id":"req-http","details":{"errors":[{"field":"topic","code":"required","message":"topic is required"}]}}}"#;
        let message = extract_error_message(body, 422, "Unprocessable Entity");
        assert!(message.contains("invalid subscription"));
        assert!(message.contains("status 422"));
        assert!(message.contains("code validation_error"));
        assert!(message.contains("request_id req-http"));
        assert!(message.contains(r#""field":"topic""#));
    }

    #[test]
    fn loss_not_confirmed_without_loss_start() {
        assert!(!loss_confirmed(5, None));
    }

    #[test]
    fn loss_not_confirmed_below_attempt_threshold() {
        let past = Instant::now()
            .checked_sub(LOSS_CONFIRM_WINDOW)
            .unwrap_or_else(Instant::now);
        assert!(!loss_confirmed(1, Some(past)));
    }

    #[test]
    fn loss_not_confirmed_within_window() {
        assert!(!loss_confirmed(LOSS_CONFIRM_ATTEMPTS, Some(Instant::now())));
    }

    #[test]
    fn loss_confirmed_past_both_gates() {
        let past = Instant::now()
            .checked_sub(LOSS_CONFIRM_WINDOW)
            .unwrap_or_else(Instant::now);
        if past.elapsed() >= LOSS_CONFIRM_WINDOW {
            assert!(loss_confirmed(LOSS_CONFIRM_ATTEMPTS, Some(past)));
        }
    }
}
