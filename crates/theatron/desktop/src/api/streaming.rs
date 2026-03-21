//! Per-session streaming from `POST /api/v1/sessions/stream`.
//!
//! Each call to `stream_turn` starts a new HTTP SSE request and returns
//! a receiver that yields `StreamEvent`s. The stream is self-terminating:
//! it closes after `TurnComplete`, `TurnAbort`, or `Error`.
//!
//! # Abort support
//!
//! Pass a `CancellationToken` to `stream_turn`. When cancelled, the
//! background task drops the SSE stream immediately, freeing the
//! HTTP connection. The Dioxus component triggers cancellation via a
//! stop button bound to the token.
//!
//! # Dioxus integration
//!
//! ```ignore
//! let cancel = CancellationToken::new();
//! let mut rx = stream_turn(client, &url, &agent, &key, &msg, cancel.child_token());
//!
//! // In a coroutine:
//! while let Some(event) = rx.recv().await {
//!     match event {
//!         StreamEvent::TextDelta(text) => {
//!             streaming_state.write().text.push_str(&text);
//!         }
//!         StreamEvent::TurnComplete { outcome } => { /* finalize */ }
//!         _ => { /* handle other events */ }
//!     }
//! }
//!
//! // On abort button:
//! cancel.cancel();
//! ```

use futures_util::StreamExt;
use reqwest::Client;
use theatron_core::sse::SseStream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use theatron_core::events::StreamEvent;
use theatron_core::id::{NousId, SessionId, ToolId, TurnId};

/// Start streaming a turn response.
///
/// Returns a channel receiver that yields `StreamEvent`s until the turn
/// completes, aborts, or errors. The background task respects the
/// `cancel` token for user-initiated abort.
///
/// `client` must have auth headers pre-configured. `Accept: text/event-stream`
/// is set per-request.
#[tracing::instrument(skip_all, fields(nous_id, session_key))]
pub(crate) fn stream_turn(
    client: Client,
    base_url: &str,
    nous_id: &str,
    session_key: &str,
    message: &str,
    cancel: CancellationToken,
) -> mpsc::Receiver<StreamEvent> {
    let (tx, rx) = mpsc::channel(256);
    let url = format!("{}/api/v1/sessions/stream", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "message": message,
        "agentId": nous_id,
        "sessionKey": session_key,
    });

    let builder = client
        .post(&url)
        .json(&body)
        .header("Accept", "text/event-stream");

    let span = tracing::info_span!("stream_turn");
    tokio::spawn(
        async move {
            let resp = match tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    tracing::info!("stream cancelled before connect");
                    let _ = tx.send(StreamEvent::TurnAbort {
                        reason: "cancelled by user".to_string(),
                    }).await;
                    return;
                }
                result = builder.send() => result,
            } {
                Ok(resp) => resp,
                Err(e) => {
                    let _ = tx
                        .send(StreamEvent::Error(format!("failed to connect: {e}")))
                        .await;
                    return;
                }
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let reason = status.canonical_reason().unwrap_or("Unknown");
                let body = resp.text().await.unwrap_or_default();
                let message = extract_error_message(&body, status.as_u16(), reason);
                let _ = tx.send(StreamEvent::Error(message)).await;
                return;
            }

            let mut es = SseStream::new(resp.bytes_stream());

            loop {
                let maybe_event = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        tracing::info!("stream cancelled by user");
                        let _ = tx.send(StreamEvent::TurnAbort {
                            reason: "cancelled by user".to_string(),
                        }).await;
                        return;
                    }
                    event = es.next() => event,
                };

                let Some(event) = maybe_event else { break };

                if let Some(parsed) = parse_stream_event(&event.event, &event.data) {
                    let is_terminal = matches!(
                        &parsed,
                        StreamEvent::TurnComplete { .. }
                            | StreamEvent::TurnAbort { .. }
                            | StreamEvent::Error(_)
                    );
                    if tx.send(parsed).await.is_err() {
                        break;
                    }
                    if is_terminal {
                        break;
                    }
                }
            }
        }
        .instrument(span),
    );

    rx
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
        tracing::warn!(event_type, field, "missing required field in stream event");
        None
    })
}

fn parse_stream_event(event_type: &str, data: &str) -> Option<StreamEvent> {
    let json: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event_type, error = %e, "failed to parse stream event JSON");
            return None;
        }
    };

    match event_type {
        "turn_start" => Some(StreamEvent::TurnStart {
            session_id: SessionId::from(str_field(&json, "sessionId", event_type)?.to_string()),
            nous_id: NousId::from(str_field(&json, "nousId", event_type)?.to_string()),
            turn_id: TurnId::from(str_field(&json, "turnId", event_type)?.to_string()),
        }),
        "text_delta" => Some(StreamEvent::TextDelta(
            str_field(&json, "text", event_type)?.to_string(),
        )),
        "thinking_delta" => Some(StreamEvent::ThinkingDelta(
            str_field(&json, "text", event_type)?.to_string(),
        )),
        "tool_start" => Some(StreamEvent::ToolStart {
            tool_name: str_field(&json, "toolName", event_type)?.to_string(),
            tool_id: ToolId::from(str_field(&json, "toolId", event_type)?.to_string()),
            input: json.get("input").cloned(),
        }),
        "tool_result" => {
            let tool_name = str_field(&json, "toolName", event_type)?.to_string();
            let tool_id = ToolId::from(str_field(&json, "toolId", event_type)?.to_string());
            let is_error = json.get("isError").and_then(|v| v.as_bool()).or_else(|| {
                tracing::warn!(
                    event_type,
                    field = "isError",
                    "missing required field in stream event"
                );
                None
            })?;
            let duration_ms = json
                .get("durationMs")
                .and_then(|v| v.as_u64())
                .or_else(|| {
                    tracing::warn!(
                        event_type,
                        field = "durationMs",
                        "missing required field in stream event"
                    );
                    None
                })?;
            Some(StreamEvent::ToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
                result: json
                    .get("result")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
        }
        "tool_approval_required" => Some(StreamEvent::ToolApprovalRequired {
            turn_id: TurnId::from(str_field(&json, "turnId", event_type)?.to_string()),
            tool_name: str_field(&json, "toolName", event_type)?.to_string(),
            tool_id: ToolId::from(str_field(&json, "toolId", event_type)?.to_string()),
            input: json
                .get("input")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            risk: str_field(&json, "risk", event_type)?.to_string(),
            reason: str_field(&json, "reason", event_type)?.to_string(),
        }),
        "tool_approval_resolved" => Some(StreamEvent::ToolApprovalResolved {
            tool_id: ToolId::from(str_field(&json, "toolId", event_type)?.to_string()),
            decision: str_field(&json, "decision", event_type)?.to_string(),
        }),
        "turn_complete" => {
            let outcome = json
                .get("outcome")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .or_else(|| {
                    tracing::warn!(event_type, "missing or invalid outcome in stream event");
                    None
                })?;
            Some(StreamEvent::TurnComplete { outcome })
        }
        "turn_abort" => Some(StreamEvent::TurnAbort {
            reason: str_field(&json, "reason", event_type)?.to_string(),
        }),
        "error" => Some(StreamEvent::Error(
            str_field(&json, "message", event_type)?.to_string(),
        )),
        // Plan events: logged but not yet surfaced in the desktop UI.
        "plan_proposed" | "plan_step_start" | "plan_step_complete" | "plan_complete" => {
            tracing::debug!(event_type, "plan event (not yet rendered in desktop)");
            None
        }
        "queue_drained" => {
            tracing::debug!("queue drained: {json}");
            None
        }
        other => {
            tracing::debug!("unknown stream event: {other}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_delta_valid() {
        let data = r#"{"text":"hello"}"#;
        let result = parse_stream_event("text_delta", data);
        assert!(matches!(result, Some(StreamEvent::TextDelta(ref t)) if t == "hello"));
    }

    #[test]
    fn parse_thinking_delta_valid() {
        let data = r#"{"text":"reasoning step"}"#;
        let result = parse_stream_event("thinking_delta", data);
        assert!(matches!(result, Some(StreamEvent::ThinkingDelta(ref t)) if t == "reasoning step"));
    }

    #[test]
    fn parse_turn_start_valid() {
        let data = r#"{"sessionId":"s1","nousId":"syn","turnId":"t1"}"#;
        let result = parse_stream_event("turn_start", data);
        if let Some(StreamEvent::TurnStart {
            session_id,
            nous_id,
            turn_id,
        }) = result
        {
            assert_eq!(&*session_id, "s1");
            assert_eq!(&*nous_id, "syn");
            assert_eq!(&*turn_id, "t1");
        } else {
            panic!("expected TurnStart");
        }
    }

    #[test]
    fn parse_turn_complete_valid() {
        let data = r#"{"outcome":{"text":"done","nousId":"syn","sessionId":"s1","model":"gpt","toolCalls":0,"inputTokens":100,"outputTokens":50,"cacheReadTokens":0,"cacheWriteTokens":0}}"#;
        let result = parse_stream_event("turn_complete", data);
        if let Some(StreamEvent::TurnComplete { outcome }) = result {
            assert_eq!(outcome.text, "done");
            assert_eq!(&*outcome.nous_id, "syn");
        } else {
            panic!("expected TurnComplete");
        }
    }

    #[test]
    fn parse_tool_result_valid() {
        let data = r#"{"toolName":"exec","toolId":"t1","isError":false,"durationMs":150}"#;
        let result = parse_stream_event("tool_result", data);
        if let Some(StreamEvent::ToolResult {
            tool_name,
            is_error,
            duration_ms,
            result: tool_result,
            ..
        }) = result
        {
            assert_eq!(tool_name, "exec");
            assert!(!is_error);
            assert_eq!(duration_ms, 150);
            assert!(tool_result.is_none());
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn parse_tool_start_valid() {
        let data = r#"{"toolName":"read_file","toolId":"t1"}"#;
        let result = parse_stream_event("tool_start", data);
        if let Some(StreamEvent::ToolStart {
            tool_name, tool_id, ..
        }) = result
        {
            assert_eq!(tool_name, "read_file");
            assert_eq!(&*tool_id, "t1");
        } else {
            panic!("expected ToolStart");
        }
    }

    #[test]
    fn parse_error_event() {
        let data = r#"{"message":"rate limited"}"#;
        let result = parse_stream_event("error", data);
        assert!(matches!(result, Some(StreamEvent::Error(ref m)) if m == "rate limited"));
    }

    #[test]
    fn parse_turn_abort() {
        let data = r#"{"reason":"guard rejected"}"#;
        let result = parse_stream_event("turn_abort", data);
        if let Some(StreamEvent::TurnAbort { reason }) = result {
            assert_eq!(reason, "guard rejected");
        } else {
            panic!("expected TurnAbort");
        }
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        let result = parse_stream_event("text_delta", "{broken");
        assert!(result.is_none());
    }

    #[test]
    fn parse_queue_drained_returns_none() {
        let data = r#"{"count":0}"#;
        let result = parse_stream_event("queue_drained", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_unknown_event_returns_none() {
        let data = r#"{"foo":"bar"}"#;
        let result = parse_stream_event("custom:event", data);
        assert!(result.is_none());
    }

    #[test]
    fn parse_tool_approval_required() {
        let data = r#"{
            "turnId": "t1",
            "toolName": "exec",
            "toolId": "tool-1",
            "input": {"command": "rm -rf /"},
            "risk": "high",
            "reason": "destructive command"
        }"#;
        let result = parse_stream_event("tool_approval_required", data);
        if let Some(StreamEvent::ToolApprovalRequired {
            tool_name,
            risk,
            reason,
            ..
        }) = result
        {
            assert_eq!(tool_name, "exec");
            assert_eq!(risk, "high");
            assert_eq!(reason, "destructive command");
        } else {
            panic!("expected ToolApprovalRequired");
        }
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
}
