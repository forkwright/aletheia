//! Domain-event SSE subscription to `GET /api/v1/events/subscribe`.
//!
//! Subscribes to the `EventBus` topics `fact.created`, `turn.complete`, and
//! `nous.lifecycle`, providing cross-session awareness: newly created facts,
//! completed turns, and agent lifecycle changes. The legacy `GET /api/v1/events`
//! endpoint is keepalive-only and is not used here.
//!
//! Auto-reconnects with exponential backoff (1s to 30s) and treats 45s of
//! silence as a stale connection.
//!
//! The connection tracks the last received SSE event ID and sends it as
//! `Last-Event-ID` on reconnect, enabling the server to replay missed events
//! from the last acknowledged cursor.

use futures_util::StreamExt;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::id::{NousId, SessionId, TurnId};
use crate::sse::SseStream;

use super::error::{format_error_fields_for_display, format_http_error_body};
use super::types::SseEvent;

/// If no SSE event is received within this window, the connection is treated as
/// stale and a reconnect is triggered. Must be > 2× the server's keepalive
/// interval (15s) to tolerate jitter. The 45s value matches proskenion's
/// `HEARTBEAT_TIMEOUT` and provides 3× margin over the server ping interval.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// Topics subscribed to on the domain-event SSE endpoint.
const SUBSCRIBE_TOPICS: &str = "fact.created,turn.complete,nous.lifecycle";

/// Manages the global SSE connection to `/api/v1/events/subscribe`.
/// Runs in a background task, sends parsed events through a channel.
pub struct SseConnection {
    // kanon:ignore RUST/pub-visibility
    rx: mpsc::Receiver<SseEvent>,
    _handle: tokio::task::JoinHandle<()>,
}

impl SseConnection {
    /// Connect using the streaming HTTP client from `ApiClient::streaming_client()`.
    /// Auth headers are already embedded in the client. `Accept: text/event-stream`
    /// is set per-request to override the client-level `Accept: application/json` default.
    #[tracing::instrument(skip_all)]
    pub fn connect(client: Client, base_url: &str) -> Self {
        // kanon:ignore RUST/pub-visibility
        let (tx, rx) = mpsc::channel(256);
        let url = format!(
            "{}?topics={}",
            keryx::url::join_base_path(base_url, "/api/v1/events/subscribe"),
            SUBSCRIBE_TOPICS
        );

        let span = tracing::info_span!("sse_connection", %url);
        // kanon:ignore RUST/spawn-no-instrument — future is instrumented with `.instrument(span)` before being passed to spawn
        let handle = tokio::spawn(
            async move {
                let mut backoff_secs: u64 = 1;
                // WHY: sent as Last-Event-ID on reconnect so the server can
                // replay missed events from the last acknowledged cursor.
                let mut last_event_id: Option<String> = None;

                loop {
                    let mut req = client.get(&url).header("Accept", "text/event-stream");
                    if let Some(ref id) = last_event_id {
                        req = req.header("Last-Event-ID", id.as_str());
                    }
                    let resp = match req.send().await {
                        Ok(resp) => resp,
                        Err(e) => {
                            tracing::error!("SSE connection failed: {e}");
                            if tx.send(SseEvent::Disconnected).await.is_err() {
                                return;
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                            backoff_secs = (backoff_secs * 2).min(30);
                            continue;
                        }
                    };

                    if !resp.status().is_success() {
                        let status = resp.status();
                        let reason = status.canonical_reason().unwrap_or("Unknown");
                        // kanon:ignore RUST/no-result-unwrap-or-default — empty body on text() failure is acceptable; status code is the primary error signal
                        let body = resp.text().await.unwrap_or_default();
                        let message = format_http_error_body(status.as_u16(), reason, &body);
                        tracing::warn!("SSE error: {message}");
                        if tx.send(SseEvent::Disconnected).await.is_err() {
                            return;
                        }
                        backoff_secs = (backoff_secs * 2).min(30);
                        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                        continue;
                    }

                    if tx.send(SseEvent::Connected).await.is_err() {
                        return;
                    }
                    tracing::info!("SSE connected");
                    backoff_secs = 1;
                    let mut es = SseStream::new(resp.bytes_stream());

                    loop {
                        let maybe_event = tokio::time::timeout(READ_TIMEOUT, es.next()).await;
                        let event = match maybe_event {
                            Ok(Some(event)) => event,
                            Ok(None) => break,
                            Err(_elapsed) => {
                                // WHY: No event received within READ_TIMEOUT. A healthy
                                // server sends pings more frequently than this window, so
                                // silence here indicates a hung or dropped connection.
                                tracing::warn!(
                                    timeout_secs = READ_TIMEOUT.as_secs(),
                                    "SSE read timeout — treating as disconnect"
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
                            // WHY: receiver dropped: shut down the SSE loop
                            return;
                        }
                    }

                    if tx.send(SseEvent::Disconnected).await.is_err() {
                        return;
                    }
                    tracing::info!("SSE reconnecting in {backoff_secs}s");
                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                }
            }
            .instrument(span),
        );

        SseConnection {
            rx,
            _handle: handle,
        }
    }

    /// Receive the next parsed SSE event.
    #[tracing::instrument(skip_all)]
    pub async fn next(&mut self) -> Option<SseEvent> {
        self.rx.recv().await
    }
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

/// Parse a raw SSE event into a domain `SseEvent`.
///
/// Returns `None` only when `data` is empty or contains only a comment.
/// All other failure modes are surfaced as typed variants:
/// - [`SseEvent::DecodeError`] for JSON parse failures
/// - [`SseEvent::UnknownEvent`] for unrecognized event types
fn parse_sse_event(event_type: &str, data: &str) -> Option<SseEvent> {
    let json: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event_type, error = %e, "failed to parse SSE event JSON");
            return Some(SseEvent::DecodeError {
                event_type: event_type.to_string(),
                raw_data: data.to_string(),
                error: e.to_string(),
            });
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
        "turn.complete" => turn_complete_event(&json, event_type),
        "fact.created" => fact_created_event(&json, event_type),
        "nous.lifecycle" => nous_lifecycle_event(&json, event_type),
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
        "checkpoint:created" => Some(SseEvent::CheckpointCreated {
            project_id: str_field(&json, "projectId", event_type)?.to_string(),
            checkpoint_id: str_field(&json, "checkpointId", event_type)?.to_string(),
        }),
        "checkpoint:updated" => Some(SseEvent::CheckpointUpdated {
            project_id: str_field(&json, "projectId", event_type)?.to_string(),
            checkpoint_id: str_field(&json, "checkpointId", event_type)?.to_string(),
            status: str_field(&json, "status", event_type)?.to_string(),
        }),
        "ping" => Some(SseEvent::Ping),
        "stream_lagged" => stream_lagged_event(&json),
        "error" => Some(SseEvent::Error {
            message: sse_error_message(&json),
        }),
        other => {
            tracing::debug!("unknown SSE event type: {other}");
            Some(SseEvent::UnknownEvent {
                event_type: other.to_string(),
                raw_data: data.to_string(),
            })
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
        session_id: SessionId::from(str_field(json, "session_id", event_type)?.to_string()),
        nous_id: NousId::from(str_field(json, "nous_id", event_type)?.to_string()),
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
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions may panic on failure"
)]
#[expect(
    clippy::expect_used,
    reason = "test helper failures should panic with context"
)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use crate::api::client::build_streaming_client;

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
    fn invalid_json_returns_decode_error() {
        let result = parse_sse_event("turn:before", "not json");
        if let Some(SseEvent::DecodeError {
            event_type,
            raw_data,
            error,
        }) = result
        {
            assert_eq!(event_type, "turn:before");
            assert_eq!(raw_data, "not json");
            assert!(!error.is_empty());
        } else {
            panic!("expected DecodeError, got {result:?}");
        }
    }

    #[test]
    fn missing_field_returns_none() {
        // Missing required fields still return None (logged via tracing::warn).
        // Typed missing-field errors are a follow-on improvement.
        let data = r#"{"nousId":"syn"}"#;
        let result = parse_sse_event("turn:before", data);
        assert!(result.is_none());
    }

    #[test]
    fn unknown_event_returns_unknown_event() {
        let data = r#"{"foo":"bar"}"#;
        let result = parse_sse_event("custom:unknown", data);
        if let Some(SseEvent::UnknownEvent {
            event_type,
            raw_data,
        }) = result
        {
            assert_eq!(event_type, "custom:unknown");
            assert_eq!(raw_data, data);
        } else {
            panic!("expected UnknownEvent, got {result:?}");
        }
    }

    #[test]
    fn parse_checkpoint_created_valid() {
        let data = r#"{"projectId":"p1","checkpointId":"cp-1"}"#;
        let result = parse_sse_event("checkpoint:created", data);
        assert!(result.is_some());
        if let Some(SseEvent::CheckpointCreated {
            project_id,
            checkpoint_id,
        }) = result
        {
            assert_eq!(project_id, "p1");
            assert_eq!(checkpoint_id, "cp-1");
        } else {
            panic!("expected CheckpointCreated");
        }
    }

    #[test]
    fn parse_checkpoint_updated_valid() {
        let data = r#"{"projectId":"p1","checkpointId":"cp-2","status":"approved"}"#;
        let result = parse_sse_event("checkpoint:updated", data);
        assert!(result.is_some());
        if let Some(SseEvent::CheckpointUpdated {
            project_id,
            checkpoint_id,
            status,
        }) = result
        {
            assert_eq!(project_id, "p1");
            assert_eq!(checkpoint_id, "cp-2");
            assert_eq!(status, "approved");
        } else {
            panic!("expected CheckpointUpdated");
        }
    }

    #[test]
    fn parse_checkpoint_created_missing_field_returns_none() {
        let data = r#"{"projectId":"p1"}"#;
        let result = parse_sse_event("checkpoint:created", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_checkpoint_updated_missing_status_returns_none() {
        let data = r#"{"projectId":"p1","checkpointId":"cp-1"}"#;
        let result = parse_sse_event("checkpoint:updated", data);
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
    fn parse_stream_lagged_valid() {
        let data = r#"{"dropped":42}"#;
        let result = parse_sse_event("stream_lagged", data);
        assert!(result.is_some());
        if let Some(SseEvent::StreamLagged { dropped }) = result {
            assert_eq!(dropped, 42);
        } else {
            panic!("expected StreamLagged");
        }
    }

    #[test]
    fn parse_stream_lagged_missing_dropped_returns_none() {
        let data = "{}";
        let result = parse_sse_event("stream_lagged", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_stream_lagged_invalid_dropped_returns_none() {
        let data = r#"{"dropped":"many"}"#;
        let result = parse_sse_event("stream_lagged", data);
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
    fn parse_fact_created_valid() {
        let data = r#"{"fact_id":"f-1","nous_id":"syn","content_preview":"hello world"}"#;
        let result = parse_sse_event("fact.created", data);
        assert!(result.is_some());
        if let Some(SseEvent::FactCreated {
            fact_id,
            nous_id,
            content_preview,
        }) = result
        {
            assert_eq!(fact_id, "f-1");
            assert_eq!(&*nous_id, "syn");
            assert_eq!(content_preview, "hello world");
        } else {
            panic!("expected FactCreated");
        }
    }

    #[test]
    fn parse_fact_created_missing_field_returns_none() {
        let data = r#"{"fact_id":"f-1"}"#;
        let result = parse_sse_event("fact.created", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_turn_complete_valid() {
        let data = r#"{"session_id":"s1","nous_id":"syn","turn_id":"t1","input_tokens":10,"output_tokens":5}"#;
        let result = parse_sse_event("turn.complete", data);
        assert!(result.is_some());
        if let Some(SseEvent::TurnComplete {
            session_id,
            nous_id,
            turn_id,
            input_tokens,
            output_tokens,
        }) = result
        {
            assert_eq!(&*session_id, "s1");
            assert_eq!(&*nous_id, "syn");
            assert_eq!(&*turn_id, "t1");
            assert_eq!(input_tokens, 10);
            assert_eq!(output_tokens, 5);
        } else {
            panic!("expected TurnComplete");
        }
    }

    #[test]
    fn parse_turn_complete_missing_field_returns_none() {
        let data = r#"{"session_id":"s1","nous_id":"syn"}"#;
        let result = parse_sse_event("turn.complete", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_turn_complete_invalid_tokens_returns_none() {
        let data = r#"{"session_id":"s1","nous_id":"syn","turn_id":"t1","input_tokens":"many","output_tokens":5}"#;
        let result = parse_sse_event("turn.complete", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_nous_lifecycle_valid() {
        let data = r#"{"nous_id":"syn","event":"created","restart_required":true}"#;
        let result = parse_sse_event("nous.lifecycle", data);
        assert!(result.is_some());
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
    fn parse_nous_lifecycle_missing_field_returns_none() {
        let data = r#"{"nous_id":"syn","event":"created"}"#;
        let result = parse_sse_event("nous.lifecycle", data);
        assert!(result.is_none());
    }

    fn serve_sse_once(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let addr = listener.local_addr().expect("read local test server addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut buf = [0_u8; 2048];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write SSE test response");
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn connection_receives_domain_event_sse_response() {
        let body = concat!(
            "event: fact.created\n",
            "data: {\"fact_id\":\"f1\",\"nous_id\":\"syn\",\"content_preview\":\"hello\"}\n\n",
            "event: turn.complete\n",
            "data: {\"session_id\":\"s1\",\"nous_id\":\"syn\",\"turn_id\":\"t1\",\"input_tokens\":10,\"output_tokens\":5}\n\n",
            "event: nous.lifecycle\n",
            "data: {\"nous_id\":\"syn\",\"event\":\"created\",\"restart_required\":true}\n\n",
        );
        let (base_url, server) = serve_sse_once(body);
        let client = build_streaming_client(None, None).expect("build streaming test client");
        let mut conn = SseConnection::connect(client, &base_url);

        let mut events = Vec::new();
        while let Some(event) = tokio::time::timeout(Duration::from_secs(2), conn.next())
            .await
            .expect("should receive event within timeout")
        {
            events.push(event);
            if events.len() == 4 {
                // Connected + three domain events.
                break;
            }
        }
        drop(conn);
        server.join().expect("test server thread should finish");

        assert!(matches!(events[0], SseEvent::Connected));
        assert!(matches!(events[1], SseEvent::FactCreated { ref fact_id, .. } if fact_id == "f1"));
        assert!(
            matches!(events[2], SseEvent::TurnComplete { input_tokens, output_tokens, .. } if input_tokens == 10 && output_tokens == 5)
        );
        assert!(
            matches!(events[3], SseEvent::NousLifecycle { restart_required, .. } if restart_required)
        );
    }
}
