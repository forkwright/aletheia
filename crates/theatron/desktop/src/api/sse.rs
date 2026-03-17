//! Global SSE connection to `GET /api/v1/events`.
//!
//! Provides cross-session awareness: agent status changes, session lifecycle,
//! and memory distillation progress. The connection auto-reconnects with
//! exponential backoff (1s to 30s) and treats 45s of silence as a stale
//! connection.
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

use futures_util::StreamExt;
use reqwest::Client;
use theatron_core::sse::SseStream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::types::{NousId, SessionId, SseEvent, TurnId};

/// If no SSE event (including pings) arrives within this window, the
/// connection is treated as stale. The server sends heartbeats every 30s,
/// so 45s gives 50% margin before triggering reconnect.
const HEARTBEAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// Initial backoff delay after a connection failure.
const INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);

/// Maximum backoff delay: caps exponential growth.
const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

/// Manages the global SSE connection to `/api/v1/events`.
///
/// Runs in a background tokio task. Parsed events flow through an mpsc
/// channel. The connection automatically reconnects with exponential
/// backoff on failure and treats prolonged silence as disconnect.
///
/// Supports graceful shutdown via `CancellationToken`. When the token
/// fires, the background task exits cleanly.
pub struct SseConnection {
    rx: mpsc::Receiver<SseEvent>,
    cancel: CancellationToken,
    _handle: tokio::task::JoinHandle<()>,
}

impl SseConnection {
    /// Connect using a shared HTTP client. Auth headers must already be
    /// embedded in the client. `Accept: text/event-stream` is set
    /// per-request to override any client-level JSON default.
    ///
    /// The returned `SseConnection` emits `Connected`/`Disconnected`
    /// lifecycle events in addition to parsed server events.
    #[tracing::instrument(skip_all)]
    pub fn connect(client: Client, base_url: &str, cancel: CancellationToken) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let url = format!("{}/api/v1/events", base_url.trim_end_matches('/'));
        let child = cancel.child_token();

        let span = tracing::info_span!("sse_connection");
        let handle = tokio::spawn(
            async move {
                let mut backoff = INITIAL_BACKOFF;

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
                            let _ = tx.send(SseEvent::Disconnected).await;
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
                        let body = resp.text().await.unwrap_or_default();
                        let message = extract_error_message(&body, status.as_u16(), reason);
                        tracing::warn!("SSE error: {message}");
                        let _ = tx.send(SseEvent::Disconnected).await;
                        backoff = advance_backoff(backoff);
                        tokio::select! {
                            biased;
                            _ = child.cancelled() => return,
                            _ = tokio::time::sleep(backoff) => {}
                        }
                        continue;
                    }

                    let _ = tx.send(SseEvent::Connected).await;
                    tracing::info!("SSE connected");
                    backoff = INITIAL_BACKOFF;
                    let mut es = SseStream::new(resp.bytes_stream());

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
                                tracing::warn!(
                                    timeout_secs = HEARTBEAT_TIMEOUT.as_secs(),
                                    "SSE heartbeat timeout — treating as disconnect"
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

                    let _ = tx.send(SseEvent::Disconnected).await;
                    tracing::info!(backoff_secs = backoff.as_secs(), "SSE reconnecting");
                    tokio::select! {
                        biased;
                        _ = child.cancelled() => return,
                        // NOTE: backoff elapsed, retry connection
                        _ = tokio::time::sleep(backoff) => {}
                    }
                }
            }
            .instrument(span),
        );

        SseConnection {
            rx,
            cancel,
            _handle: handle,
        }
    }

    /// Receive the next parsed SSE event. Returns `None` when the
    /// connection task exits (shutdown or channel closed).
    pub async fn next(&mut self) -> Option<SseEvent> {
        self.rx.recv().await
    }

    /// Signal the background task to shut down.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

/// Advance exponential backoff: double the interval, capped at `MAX_BACKOFF`.
#[must_use]
fn advance_backoff(current: std::time::Duration) -> std::time::Duration {
    (current * 2).min(MAX_BACKOFF)
}

/// Extract a human-readable error message from an HTTP error response body.
fn extract_error_message(body: &str, status_code: u16, reason: &str) -> String {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        json.get("message")
            .or_else(|| json.get("error"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{status_code} {reason}"))
    } else {
        format!("{status_code} {reason}")
    }
}

fn str_field<'a>(json: &'a serde_json::Value, field: &str, event_type: &str) -> Option<&'a str> {
    json.get(field).and_then(|v| v.as_str()).or_else(|| {
        tracing::warn!(event_type, field, "missing required field in SSE event");
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
        "ping" => Some(SseEvent::Ping),
        other => {
            tracing::debug!("unknown SSE event type: {other}");
            None
        }
    }
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
        let data = r#"{}"#;
        let result = parse_sse_event("ping", data);
        assert!(matches!(result, Some(SseEvent::Ping)));
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
}
