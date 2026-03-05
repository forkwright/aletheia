use futures_util::StreamExt;
use reqwest_eventsource::{Event as EsEvent, EventSource};
use tokio::sync::mpsc;

use super::types::SseEvent;

/// Manages the global SSE connection to /api/events.
/// Runs in a background task, sends parsed events through a channel.
pub struct SseConnection {
    rx: mpsc::Receiver<SseEvent>,
    _handle: tokio::task::JoinHandle<()>,
}

impl SseConnection {
    pub fn connect(base_url: &str, token: Option<&str>) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let url = format!("{}/api/v1/events", base_url.trim_end_matches('/'));
        let token_owned = token.map(|t| t.to_string());

        let handle = tokio::spawn(async move {
            let mut backoff_secs: u64 = 1;

            loop {
                let mut req = reqwest::Client::new()
                    .get(&url)
                    .header("Accept", "text/event-stream");
                if let Some(ref t) = token_owned {
                    req = req.bearer_auth(t);
                }
                let mut es = EventSource::new(req).expect("valid SSE request");

                let _ = tx.send(SseEvent::Connected).await;
                let mut connected = false;

                while let Some(event) = es.next().await {
                    match event {
                        Ok(EsEvent::Open) => {
                            tracing::info!("SSE connected");
                            connected = true;
                            backoff_secs = 1; // Reset backoff on successful connection
                        }
                        Ok(EsEvent::Message(msg)) => {
                            if let Some(parsed) = parse_sse_event(&msg.event, &msg.data) {
                                if tx.send(parsed).await.is_err() {
                                    return; // Receiver dropped, shut down
                                }
                            }
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
        });

        SseConnection {
            rx,
            _handle: handle,
        }
    }

    pub async fn next(&mut self) -> Option<SseEvent> {
        self.rx.recv().await
    }
}

fn parse_sse_event(event_type: &str, data: &str) -> Option<SseEvent> {
    let json: serde_json::Value = serde_json::from_str(data).ok()?;

    match event_type {
        "init" => {
            let active_turns = serde_json::from_value(json.get("activeTurns")?.clone()).ok()?;
            Some(SseEvent::Init { active_turns })
        }
        "turn:before" => Some(SseEvent::TurnBefore {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            session_id: json.get("sessionId")?.as_str()?.to_string(),
            turn_id: json.get("turnId")?.as_str()?.to_string(),
        }),
        "turn:after" => Some(SseEvent::TurnAfter {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            session_id: json.get("sessionId")?.as_str()?.to_string(),
        }),
        "tool:called" => Some(SseEvent::ToolCalled {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            tool_name: json.get("toolName")?.as_str()?.to_string(),
        }),
        "tool:failed" => Some(SseEvent::ToolFailed {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            tool_name: json.get("toolName")?.as_str()?.to_string(),
            error: json
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown")
                .to_string(),
        }),
        "status:update" => Some(SseEvent::StatusUpdate {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            status: json.get("status")?.as_str()?.to_string(),
        }),
        "session:created" => Some(SseEvent::SessionCreated {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            session_id: json.get("sessionId")?.as_str()?.to_string(),
        }),
        "session:archived" => Some(SseEvent::SessionArchived {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            session_id: json.get("sessionId")?.as_str()?.to_string(),
        }),
        "distill:before" => Some(SseEvent::DistillBefore {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
        }),
        "distill:stage" => Some(SseEvent::DistillStage {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            stage: json.get("stage")?.as_str()?.to_string(),
        }),
        "distill:after" => Some(SseEvent::DistillAfter {
            nous_id: json.get("nousId")?.as_str()?.to_string(),
        }),
        "ping" => Some(SseEvent::Ping),
        other => {
            tracing::debug!("unknown SSE event type: {other}");
            None
        }
    }
}
