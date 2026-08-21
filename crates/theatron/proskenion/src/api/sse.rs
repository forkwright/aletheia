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
//!
//! Event parsing (`skene::api::sse::parse_sse_event`) is adopted directly
//! from skene rather than re-derived here — this module owns only what's
//! genuinely proskenion-specific: `CancellationToken`-based graceful
//! shutdown for the Dioxus coroutine lifecycle, and delayed loss
//! confirmation (`LOSS_CONFIRM_ATTEMPTS`/`LOSS_CONFIRM_WINDOW`) so a single
//! transient blip never flips UI state or fires a lost/restored toast pair.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures_util::StreamExt;
use reqwest::Client;
use skene::api::sse::parse_sse_event;
use skene::sse::SseStream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use skene::api::error::format_http_error_body;
use skene::api::types::SseEvent;

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
    // WHY: sent as Last-Event-ID on reconnect so the server can replay
    // missed events from the last acknowledged cursor (RFC 7541).
    let mut last_event_id: Option<String> = None;

    loop {
        if child.is_cancelled() {
            return;
        }

        let mut req = client.get(&url).header("Accept", "text/event-stream");
        if let Some(ref id) = last_event_id {
            req = req.header("Last-Event-ID", id.as_str());
        }
        let resp = match tokio::select! {
            biased;
            _ = child.cancelled() => return,
            result = req.send() => result,
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
                Ok(Some(Ok(event))) => event,
                Ok(Some(Err(e))) => {
                    // WHY: keryx v1.4.0 surfaces mid-stream transport failures
                    // as Err items. Break to the silent reconnect path below;
                    // only a confirmed loss is reported to the UI.
                    tracing::warn!(error = %e, "SSE transport error — reconnecting");
                    break;
                }
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

            // Track the last event ID for Last-Event-ID on reconnect.
            if let Some(id) = event.id.clone() {
                last_event_id = Some(id);
            }

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

#[cfg(test)]
mod tests {
    use super::*;

    // WHY(#5892): these two tests pin the specific gap the adoption closed —
    // both would have returned `None`/silently dropped under proskenion's
    // former local `parse_sse_event`, which had no `checkpoint:*` arms and
    // collapsed decode failures to `None` instead of a typed variant. Full
    // event-type coverage lives in skene's own test suite
    // (`skene::api::sse::tests`); duplicating it here would just be the same
    // parallel-maintenance burden this adoption was meant to remove.

    #[test]
    fn checkpoint_created_reaches_proskenion_through_the_shared_parser() {
        // WHY: `services/sse.rs` has handled `SseEvent::CheckpointCreated`
        // since before this adoption, but the local parser never produced
        // it — the handler was silently starved. This is the regression
        // test for that gap.
        let data = r#"{"projectId":"p1","checkpointId":"cp-1"}"#;
        let result = parse_sse_event("checkpoint:created", data);
        assert!(
            matches!(result, Some(SseEvent::CheckpointCreated { .. })),
            "expected CheckpointCreated, got {result:?}"
        );
    }

    #[test]
    fn decode_failure_is_a_typed_event_not_a_silent_drop() {
        let result = parse_sse_event("turn:before", "not json");
        assert!(
            matches!(result, Some(SseEvent::DecodeError { .. })),
            "expected DecodeError, got {result:?}"
        );
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
        // The window gate must be satisfied for this to isolate the attempt
        // gate. `unwrap_or_else(Instant::now)` would leave both gates unmet and
        // the test would still pass, proving less than its name claims.
        let past = Instant::now().checked_sub(LOSS_CONFIRM_WINDOW);
        assert!(past.is_some(), "the window gate must be satisfiable");
        assert!(!loss_confirmed(1, past));
    }

    #[test]
    fn loss_not_confirmed_within_window() {
        assert!(!loss_confirmed(LOSS_CONFIRM_ATTEMPTS, Some(Instant::now())));
    }

    #[test]
    fn loss_confirmed_past_both_gates() {
        // WHY(#6908): this asserted inside `if past.elapsed() >= WINDOW`, over a
        // `past` that fell back to `Instant::now()` when `checked_sub` failed.
        // Both together meant a clock near its origin produced a test that
        // asserted nothing and still reported success. A check that passes when
        // it did not run is worse than one that is absent.
        let past = Instant::now().checked_sub(LOSS_CONFIRM_WINDOW);
        assert!(
            past.is_some(),
            "the monotonic clock must be at least LOSS_CONFIRM_WINDOW past its origin"
        );
        assert!(loss_confirmed(LOSS_CONFIRM_ATTEMPTS, past));
    }
}
