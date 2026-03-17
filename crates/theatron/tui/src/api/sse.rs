use futures_util::StreamExt;
use reqwest::Client;
use reqwest_eventsource::{Event as EsEvent, EventSource};
use tokio::sync::mpsc;
use tracing::Instrument;

use aletheia_koina::http::CONTENT_TYPE_EVENT_STREAM;

use crate::id::{NousId, SessionId, TurnId};

use super::types::SseEvent;

/// If no SSE event is received within this window, the connection is treated as
/// stale and a reconnect is triggered. Default covers quiet periods between pings
/// while still detecting hung connections promptly.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Manages the global SSE connection to /api/v1/events.
/// Runs in a background task, sends parsed events through a channel.
pub struct SseConnection {
    rx: mpsc::Receiver<SseEvent>,
    _handle: tokio::task::JoinHandle<()>,
}

impl SseConnection {
    /// Connect using the shared HTTP client from `ApiClient::raw_client()`.
    /// Auth headers are already embedded in the client. `Accept: text/event-stream`
    /// is set per-request to override the client-level `Accept: application/json` default.
    #[tracing::instrument(skip_all)]
    pub fn connect(client: Client, base_url: &str) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let url = format!("{}/api/v1/events", base_url.trim_end_matches('/'));

        let span = tracing::info_span!("sse_connection");
        let handle = tokio::spawn(
            async move {
                let mut backoff_secs: u64 = 1;

                loop {
                    let req = client.get(&url).header("Accept", CONTENT_TYPE_EVENT_STREAM);
                    let mut es = match EventSource::new(req) {
                        Ok(es) => es,
                        Err(e) => {
                            tracing::error!("failed to create SSE EventSource: {e}");
                            let _ = tx.send(SseEvent::Disconnected).await;
                            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                            backoff_secs = (backoff_secs * 2).min(30);
                            continue;
                        }
                    };

                    let _ = tx.send(SseEvent::Connected).await;
                    let mut connected = false;

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
                                es.close();
                                break;
                            }
                        };

                        match event {
                            Ok(EsEvent::Open) => {
                                tracing::info!("SSE connected");
                                connected = true;
                                backoff_secs = 1;
                            }
                            Ok(EsEvent::Message(msg)) => {
                                if let Some(parsed) = parse_sse_event(&msg.event, &msg.data)
                                    && tx.send(parsed).await.is_err()
                                {
                                    // WHY: receiver dropped: shut down the SSE loop
                                    return;
                                }
                            }
                            Err(reqwest_eventsource::Error::InvalidStatusCode(status, resp)) => {
                                let reason = status.canonical_reason().unwrap_or("Unknown");
                                let body = resp.text().await.unwrap_or_default();
                                let message = if let Ok(json) =
                                    serde_json::from_str::<serde_json::Value>(&body)
                                {
                                    json.get("message")
                                        .or_else(|| json.get("error"))
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| {
                                            format!("{} {}", status.as_u16(), reason)
                                        })
                                } else {
                                    format!("{} {}", status.as_u16(), reason)
                                };
                                tracing::warn!("SSE error: {message}");
                                es.close();
                                break;
                            }
                            Err(e) => {
                                tracing::warn!("SSE error: {e}");
                                es.close();
                                break;
                            }
                        }
                    }

                    let _ = tx.send(SseEvent::Disconnected).await;
                    if !connected {
                        backoff_secs = (backoff_secs * 2).min(30);
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
}
