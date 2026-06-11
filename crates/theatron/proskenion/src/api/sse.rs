//! Global SSE connection to `GET /api/v1/events/subscribe`.
//!
//! Provides cross-session awareness: turn completion and session lifecycle.
//! The connection subscribes to pylon's domain-event topics (dot-separated,
//! e.g. `turn.complete`) and maps each onto the UI-level [`SseEvent`]. It
//! auto-reconnects with exponential backoff (1s to 30s) and treats 45s of
//! byte silence as a stale connection.
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

use futures_util::StreamExt;
use reqwest::Client;
use skene::sse::SseStream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use skene::api::types::SseEvent;
use skene::id::{NousId, SessionId};

/// If no bytes arrive from the server within this window, the connection
/// is treated as stale. The server emits a keepalive every 30s
/// (`gateway.sse_heartbeat_interval_secs` default), so 45s gives 50% margin.
///
/// NOTE: keepalives are SSE comment lines, which the parser consumes
/// without yielding an event — staleness is therefore measured on raw
/// byte activity, not on parsed events.
const HEARTBEAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// Initial backoff delay after a connection failure.
const INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(1);

/// Maximum backoff delay: caps exponential growth.
const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

/// Topics requested from `GET /api/v1/events/subscribe`.
///
/// NOTE: mirrors the server's topic registry (`GET /api/v1/events/discovery`).
/// Only `turn.complete` and `fact.created` have live publish sites today;
/// the session lifecycle topics are declared by the server and subscribed
/// here so they light up without a client change.
const SUBSCRIBE_TOPICS: &[&str] = &[
    "turn.complete",
    "fact.created",
    "session.started",
    "session.ended",
];

/// Build the topic-filtered subscribe URL for `base_url`.
fn subscribe_url(base_url: &str) -> String {
    format!(
        "{}/api/v1/events/subscribe?topics={}",
        base_url.trim_end_matches('/'),
        SUBSCRIBE_TOPICS.join(",")
    )
}

/// Manages the global SSE connection to `/api/v1/events/subscribe`.
///
/// Runs in a background tokio task. Parsed events flow through an mpsc
/// channel. The connection automatically reconnects with exponential
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
    /// The returned `SseConnection` emits `Connected`/`Disconnected`
    /// lifecycle events in addition to parsed server events.
    #[tracing::instrument(skip_all)]
    pub(crate) fn connect(client: Client, base_url: &str, cancel: CancellationToken) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let url = subscribe_url(base_url);
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
                if tx.send(SseEvent::Disconnected).await.is_err() {
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
            if tx.send(SseEvent::Disconnected).await.is_err() {
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

        // WHY: server keepalives are comment-only and the parser drops
        // comments without yielding, so byte arrival is the only liveness
        // signal on an idle stream. Track it on the raw byte stream.
        let connected_at = std::time::Instant::now();
        let last_rx_ms = Arc::new(AtomicU64::new(0));
        let byte_activity = Arc::clone(&last_rx_ms);
        let mut es = SseStream::new(resp.bytes_stream().inspect(move |_chunk| {
            byte_activity.store(elapsed_ms(connected_at), Ordering::Relaxed);
        }));

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
                    let idle_ms =
                        elapsed_ms(connected_at).saturating_sub(last_rx_ms.load(Ordering::Relaxed));
                    if std::time::Duration::from_millis(idle_ms) < HEARTBEAT_TIMEOUT {
                        // NOTE: no parsed event, but bytes (keepalives)
                        // arrived recently — the link is alive.
                        continue;
                    }
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

        if tx.send(SseEvent::Disconnected).await.is_err() {
            return;
        }
        tracing::info!(backoff_secs = backoff.as_secs(), "SSE reconnecting");
        tokio::select! {
            biased;
            _ = child.cancelled() => return,
            // NOTE: backoff elapsed, retry connection
            _ = tokio::time::sleep(backoff) => {}
        }
    }
}

/// Advance exponential backoff: double the interval, capped at `MAX_BACKOFF`.
#[must_use]
fn advance_backoff(current: std::time::Duration) -> std::time::Duration {
    (current * 2).min(MAX_BACKOFF)
}

/// Milliseconds elapsed since `since`, saturating at `u64::MAX`.
fn elapsed_ms(since: std::time::Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
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

/// Map a pylon domain event (`event: <topic>` + JSON payload) onto the
/// UI-level [`SseEvent`].
///
/// Payload keys are snake_case (the event-bus convention). Topics without
/// a UI mapping are dropped at debug level — the subscription is
/// topic-filtered server-side, so anything unexpected here is contract
/// drift, not an error.
fn parse_sse_event(event_type: &str, data: &str) -> Option<SseEvent> {
    let json: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event_type, error = %e, "failed to parse SSE event JSON");
            return None;
        }
    };

    match event_type {
        // NOTE: the server publishes turn completion only; there is no
        // turn-start topic, so `TurnBefore` is never produced here.
        "turn.complete" => Some(SseEvent::TurnAfter {
            nous_id: NousId::from(str_field(&json, "nous_id", event_type)?.to_string()),
            session_id: SessionId::from(str_field(&json, "session_id", event_type)?.to_string()),
        }),
        "session.started" => Some(SseEvent::SessionCreated {
            nous_id: NousId::from(str_field(&json, "nous_id", event_type)?.to_string()),
            session_id: SessionId::from(str_field(&json, "session_id", event_type)?.to_string()),
        }),
        "session.ended" => Some(SseEvent::SessionArchived {
            nous_id: NousId::from(str_field(&json, "nous_id", event_type)?.to_string()),
            session_id: SessionId::from(str_field(&json, "session_id", event_type)?.to_string()),
        }),
        "fact.created" => {
            // NOTE: no SseEvent variant models fact ingestion yet; the
            // topic is subscribed for forward-compat and dropped here.
            tracing::debug!(event_type, "no UI mapping for topic; dropping");
            None
        }
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
    fn subscribe_url_trims_trailing_slash() {
        assert_eq!(
            subscribe_url("http://192.168.1.100:18789/"),
            "http://192.168.1.100:18789/api/v1/events/subscribe\
             ?topics=turn.complete,fact.created,session.started,session.ended"
        );
    }

    #[test]
    fn subscribe_url_without_trailing_slash() {
        let url = subscribe_url("http://192.168.1.100:18789");
        assert!(url.starts_with("http://192.168.1.100:18789/api/v1/events/subscribe?topics="));
    }

    #[test]
    fn parse_turn_complete_valid() {
        // NOTE: mirrors the real publish payload (sessions/streaming.rs):
        // extra token-count fields must be tolerated.
        let data = r#"{"session_id":"sess-1","nous_id":"syn","turn_id":"turn-1","input_tokens":10,"output_tokens":20}"#;
        let result = parse_sse_event("turn.complete", data);
        if let Some(SseEvent::TurnAfter {
            nous_id,
            session_id,
        }) = result
        {
            assert_eq!(&*nous_id, "syn");
            assert_eq!(&*session_id, "sess-1");
        } else {
            panic!("expected TurnAfter");
        }
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        let result = parse_sse_event("turn.complete", "not json");
        assert!(result.is_none());
    }

    #[test]
    fn parse_missing_field_returns_none() {
        let data = r#"{"nous_id":"syn"}"#;
        let result = parse_sse_event("turn.complete", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_session_started() {
        let data = r#"{"nous_id":"syn","session_id":"s-new"}"#;
        let result = parse_sse_event("session.started", data);
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
    fn parse_session_ended() {
        let data = r#"{"nous_id":"syn","session_id":"s-old"}"#;
        let result = parse_sse_event("session.ended", data);
        if let Some(SseEvent::SessionArchived {
            nous_id,
            session_id,
        }) = result
        {
            assert_eq!(&*nous_id, "syn");
            assert_eq!(&*session_id, "s-old");
        } else {
            panic!("expected SessionArchived");
        }
    }

    #[test]
    fn parse_fact_created_dropped_gracefully() {
        let data = r#"{"fact_id":"f-1","nous_id":"syn","content_preview":"alice"}"#;
        assert!(parse_sse_event("fact.created", data).is_none());
    }

    #[test]
    fn parse_legacy_colon_name_returns_none() {
        // NOTE: regression guard — pre-rewrite colon names are not topics.
        let data = r#"{"nousId":"syn","sessionId":"s1","turnId":"t1"}"#;
        assert!(parse_sse_event("turn:before", data).is_none());
    }

    #[test]
    fn parse_unknown_event_returns_none() {
        let data = r#"{"foo":"bar"}"#;
        let result = parse_sse_event("custom.unknown", data);
        assert!(result.is_none());
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
