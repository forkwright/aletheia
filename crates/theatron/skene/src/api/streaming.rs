//! Per-session streaming from `POST /api/v1/sessions/stream`.
//!
//! Each call to [`stream_message`] starts a new HTTP SSE request and returns
//! a receiver that yields [`StreamEvent`]s. The stream is self-terminating:
//! it closes after `TurnComplete`, `TurnAbort`, or `Error`.

use futures_util::StreamExt;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::Instrument;

use koina::http::CONTENT_TYPE_EVENT_STREAM;

use crate::events::StreamEvent;
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};
use crate::sse::SseStream;

/// If no streaming event is received within this window, the connection is
/// treated as hung. Matches the SSE connection's `READ_TIMEOUT` in `sse.rs`
/// for a consistent timeout policy across both connection types.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// Streams a turn response from POST /api/v1/sessions/stream.
/// Returns a channel that yields parsed `StreamEvents`.
///
/// `client` must be the shared instance from `ApiClient::raw_client()`: auth headers
/// are already embedded. `Accept: text/event-stream` is set per-request to override
/// the client-level `Accept: application/json` default.
#[tracing::instrument(skip_all)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Client is Arc-based; moved into the spawned task"
)]
pub fn stream_message(
    // kanon:ignore RUST/pub-visibility
    client: Client,
    base_url: &str,
    nous_id: &str,
    session_key: &str,
    text: &str,
) -> mpsc::Receiver<StreamEvent> {
    let (tx, rx) = mpsc::channel(256);
    let url = format!("{}/api/v1/sessions/stream", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "message": text,
        "nous_id": nous_id,
        "session_key": session_key,
    });

    let builder = client
        .post(&url)
        .json(&body)
        .header("Accept", CONTENT_TYPE_EVENT_STREAM);

    let span = tracing::info_span!(
        "stream_message",
        nous.id = nous_id,
        session.key = session_key
    );
    let task = async move {
        let resp = match builder.send().await {
            Ok(resp) => resp,
            Err(e) => {
                if tx
                    .send(StreamEvent::Error(format!("failed to connect: {e}")))
                    .await
                    .is_err()
                {
                    tracing::debug!("stream receiver dropped before connect error");
                }
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let reason = status.canonical_reason().unwrap_or("Unknown");
            let body = match resp.text().await {
                Ok(body) => body,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to read stream error response body");
                    String::new()
                }
            };
            let message = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                json.get("message")
                    .or_else(|| json.get("error"))
                    .and_then(|v| v.as_str())
                    .map_or_else(
                        || format!("{} {}", status.as_u16(), reason),
                        std::string::ToString::to_string,
                    )
            } else {
                format!("{} {}", status.as_u16(), reason)
            };
            if tx.send(StreamEvent::Error(message)).await.is_err() {
                tracing::debug!("stream receiver dropped before HTTP error");
            }
            return;
        }

        let mut es = SseStream::new(resp.bytes_stream());

        loop {
            let maybe_event = tokio::time::timeout(READ_TIMEOUT, es.next()).await;
            let event = match maybe_event {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_elapsed) => {
                    // WHY: No event received within READ_TIMEOUT. A healthy
                    // server sends data more frequently than this window, so
                    // silence here indicates a hung or dropped connection.
                    tracing::warn!(
                        timeout_secs = READ_TIMEOUT.as_secs(),
                        "stream read timeout — treating as error"
                    );
                    if tx
                        .send(StreamEvent::Error("stream timeout".to_string()))
                        .await
                        .is_err()
                    {
                        tracing::debug!("stream receiver dropped before timeout error");
                    }
                    break;
                }
            };

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
    };
    tokio::spawn(task.instrument(span));

    rx
}

fn str_field<'a>(json: &'a serde_json::Value, field: &str, event_type: &str) -> Option<&'a str> {
    json.get(field).and_then(|v| v.as_str()).or_else(|| {
        tracing::warn!(event_type, field, "missing required field in stream event");
        None
    })
}

fn str_any_field<'a>(
    json: &'a serde_json::Value,
    fields: &[&str],
    event_type: &str,
) -> Option<&'a str> {
    fields
        .iter()
        .find_map(|field| json.get(field).and_then(|v| v.as_str()))
        .or_else(|| {
            tracing::warn!(
                event_type,
                fields = ?fields,
                "missing required field in stream event"
            );
            None
        })
}

fn bool_any_field(json: &serde_json::Value, fields: &[&str], event_type: &str) -> Option<bool> {
    fields
        .iter()
        .find_map(|field| json.get(field).and_then(serde_json::Value::as_bool))
        .or_else(|| {
            tracing::warn!(
                event_type,
                fields = ?fields,
                "missing required field in stream event"
            );
            None
        })
}

fn u64_any_field(json: &serde_json::Value, fields: &[&str], event_type: &str) -> Option<u64> {
    fields
        .iter()
        .find_map(|field| json.get(field).and_then(serde_json::Value::as_u64))
        .or_else(|| {
            tracing::warn!(
                event_type,
                fields = ?fields,
                "missing required field in stream event"
            );
            None
        })
}

#[expect(
    clippy::too_many_lines,
    reason = "flat match arms; splitting would obscure the 1:1 event-type mapping"
)]
fn parse_stream_event(event_type: &str, data: &str) -> Option<StreamEvent> {
    let json: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event_type, error = %e, "failed to parse stream event JSON");
            return None;
        }
    };

    match event_type {
        "message_start" | "turn_start" => Some(StreamEvent::TurnStart {
            session_id: SessionId::from(
                str_any_field(&json, &["session_id", "sessionId"], event_type)?.to_string(),
            ),
            nous_id: NousId::from(
                str_any_field(&json, &["nous_id", "nousId"], event_type)?.to_string(),
            ),
            turn_id: TurnId::from(
                str_any_field(&json, &["turn_id", "turnId"], event_type)?.to_string(),
            ),
        }),
        "text_delta" => Some(StreamEvent::TextDelta(
            str_field(&json, "text", event_type)?.to_string(),
        )),
        "thinking_delta" => Some(StreamEvent::ThinkingDelta(
            str_field(&json, "text", event_type)?.to_string(),
        )),
        "tool_use" | "tool_start" => Some(StreamEvent::ToolStart {
            tool_name: str_any_field(&json, &["tool_name", "toolName"], event_type)?.to_string(),
            tool_id: ToolId::from(
                str_any_field(&json, &["tool_id", "toolId"], event_type)?.to_string(),
            ),
            input: json.get("input").cloned(),
        }),
        "tool_result" => {
            let tool_name =
                str_any_field(&json, &["tool_name", "toolName"], event_type)?.to_string();
            let tool_id =
                ToolId::from(str_any_field(&json, &["tool_id", "toolId"], event_type)?.to_string());
            let is_error = bool_any_field(&json, &["is_error", "isError"], event_type)?;
            let duration_ms = u64_any_field(&json, &["duration_ms", "durationMs"], event_type)?;
            let result = json
                .get("result")
                .and_then(|v| v.as_str())
                .map(std::string::ToString::to_string);
            Some(StreamEvent::ToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
                result,
            })
        }
        "tool_approval_required" => Some(StreamEvent::ToolApprovalRequired {
            turn_id: TurnId::from(
                str_any_field(&json, &["turn_id", "turnId"], event_type)?.to_string(),
            ),
            tool_name: str_any_field(&json, &["tool_name", "toolName"], event_type)?.to_string(),
            tool_id: ToolId::from(
                str_any_field(&json, &["tool_id", "toolId"], event_type)?.to_string(),
            ),
            input: json
                .get("input")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            risk: str_field(&json, "risk", event_type)?.to_string(),
            reason: str_field(&json, "reason", event_type)?.to_string(),
        }),
        "tool_approval_resolved" => Some(StreamEvent::ToolApprovalResolved {
            tool_id: ToolId::from(
                str_any_field(&json, &["tool_id", "toolId"], event_type)?.to_string(),
            ),
            decision: str_field(&json, "decision", event_type)?.to_string(),
        }),
        "plan_proposed" => {
            let plan = json
                .get("plan")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .or_else(|| {
                    tracing::warn!(event_type, "missing or invalid plan in stream event");
                    None
                })?;
            Some(StreamEvent::PlanProposed { plan })
        }
        "plan_step_start" => {
            let step_id_u64 = u64_any_field(&json, &["step_id", "stepId"], event_type)?;
            let step_id = u32::try_from(step_id_u64).unwrap_or(u32::MAX);
            Some(StreamEvent::PlanStepStart {
                plan_id: PlanId::from(
                    str_any_field(&json, &["plan_id", "planId"], event_type)?.to_string(),
                ),
                step_id,
            })
        }
        "plan_step_complete" => {
            let step_id_u64 = u64_any_field(&json, &["step_id", "stepId"], event_type)?;
            let step_id = u32::try_from(step_id_u64).unwrap_or(u32::MAX);
            Some(StreamEvent::PlanStepComplete {
                plan_id: PlanId::from(
                    str_any_field(&json, &["plan_id", "planId"], event_type)?.to_string(),
                ),
                step_id,
                status: str_field(&json, "status", event_type)?.to_string(),
            })
        }
        "plan_complete" => Some(StreamEvent::PlanComplete {
            plan_id: PlanId::from(
                str_any_field(&json, &["plan_id", "planId"], event_type)?.to_string(),
            ),
            status: str_field(&json, "status", event_type)?.to_string(),
        }),
        "message_complete" | "turn_complete" => {
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
        "queue_drained" => {
            tracing::debug!("queue drained: {}", json);
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
    fn parse_message_start_snake_case_valid() {
        let data = r#"{"type":"message_start","session_id":"s1","nous_id":"syn","turn_id":"t1"}"#;
        let result = parse_stream_event("message_start", data);
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
        assert!(result.is_some());
        if let Some(StreamEvent::TurnComplete { outcome }) = result {
            assert_eq!(outcome.text, "done");
            assert_eq!(&*outcome.nous_id, "syn");
        } else {
            panic!("expected TurnComplete");
        }
    }

    #[test]
    fn parse_message_complete_snake_case_valid() {
        let data = r#"{"type":"message_complete","outcome":{"text":"done","nous_id":"syn","session_id":"s1","model":"gpt","tool_calls":0,"input_tokens":100,"output_tokens":50,"cache_read_tokens":0,"cache_write_tokens":0}}"#;
        let result = parse_stream_event("message_complete", data);
        assert!(result.is_some());
        if let Some(StreamEvent::TurnComplete { outcome }) = result {
            assert_eq!(outcome.text, "done");
            assert_eq!(&*outcome.nous_id, "syn");
            assert_eq!(&*outcome.session_id, "s1");
            assert_eq!(outcome.input_tokens, 100);
            assert_eq!(outcome.output_tokens, 50);
        } else {
            panic!("expected TurnComplete");
        }
    }

    // WHY: message_complete is the turn terminator; an unparseable outcome
    // (model: null from the gateway) silently dropped it and hung the UI.
    #[test]
    fn parse_message_complete_null_model_valid() {
        let data = r#"{"type":"message_complete","outcome":{"text":"done","nous_id":"syn","session_id":"s1","model":null,"tool_calls":0,"input_tokens":1,"output_tokens":1,"cache_read_tokens":0,"cache_write_tokens":0}}"#;
        let result = parse_stream_event("message_complete", data);
        if let Some(StreamEvent::TurnComplete { outcome }) = result {
            assert!(outcome.model.is_none());
        } else {
            panic!("expected TurnComplete");
        }
    }

    #[test]
    fn parse_tool_result_valid() {
        let data = r#"{"toolName":"exec","toolId":"t1","isError":false,"durationMs":150}"#;
        let result = parse_stream_event("tool_result", data);
        assert!(result.is_some());
        if let Some(StreamEvent::ToolResult {
            tool_name,
            is_error,
            duration_ms,
            ..
        }) = result
        {
            assert_eq!(tool_name, "exec");
            assert!(!is_error);
            assert_eq!(duration_ms, 150);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn parse_tool_result_snake_case_valid() {
        let data = r#"{"type":"tool_result","tool_name":"exec","tool_id":"t1","is_error":false,"duration_ms":150,"result":"ok"}"#;
        let result = parse_stream_event("tool_result", data);
        assert!(result.is_some());
        if let Some(StreamEvent::ToolResult {
            tool_name,
            is_error,
            duration_ms,
            ..
        }) = result
        {
            assert_eq!(tool_name, "exec");
            assert!(!is_error);
            assert_eq!(duration_ms, 150);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn parse_tool_use_snake_case_valid() {
        let data = r#"{"type":"tool_use","tool_name":"read_file","tool_id":"t1","input":{"path":"README.md"}}"#;
        let result = parse_stream_event("tool_use", data);
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
}
