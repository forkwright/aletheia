//! Per-session streaming from `POST /api/v1/sessions/stream`.
//!
//! Each call to [`stream_message`] starts a new HTTP SSE request and returns
//! a receiver that yields [`StreamEvent`]s. The stream is self-terminating:
//! it closes after `TurnComplete`, `TurnAbort`, or `Error`.

use futures_util::StreamExt;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::Instrument;

use aletheia_koina::http::CONTENT_TYPE_EVENT_STREAM;

use crate::events::StreamEvent;
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};
use crate::sse::SseStream;

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
        "agentId": nous_id,
        "sessionKey": session_key,
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
    tokio::spawn(
        // kanon:ignore RUST/spawn-no-instrument
        async move {
            let resp = match builder.send().await {
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
                let _ = tx.send(StreamEvent::Error(message)).await;
                return;
            }

            let mut es = SseStream::new(resp.bytes_stream());

            while let Some(event) = es.next().await {
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

fn str_field<'a>(json: &'a serde_json::Value, field: &str, event_type: &str) -> Option<&'a str> {
    json.get(field).and_then(|v| v.as_str()).or_else(|| {
        tracing::warn!(event_type, field, "missing required field in stream event");
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
            let is_error = json
                .get("isError")
                .and_then(serde_json::Value::as_bool)
                .or_else(|| {
                    tracing::warn!(
                        event_type,
                        field = "isError",
                        "missing required field in stream event"
                    );
                    None
                })?;
            let duration_ms = json
                .get("durationMs")
                .and_then(serde_json::Value::as_u64)
                .or_else(|| {
                    tracing::warn!(
                        event_type,
                        field = "durationMs",
                        "missing required field in stream event"
                    );
                    None
                })?;
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
            let step_id_u64 = json
                .get("stepId")
                .and_then(serde_json::Value::as_u64)
                .or_else(|| {
                    tracing::warn!(
                        event_type,
                        field = "stepId",
                        "missing required field in stream event"
                    );
                    None
                })?;
            let step_id = u32::try_from(step_id_u64).unwrap_or(u32::MAX);
            Some(StreamEvent::PlanStepStart {
                plan_id: PlanId::from(str_field(&json, "planId", event_type)?.to_string()),
                step_id,
            })
        }
        "plan_step_complete" => {
            let step_id_u64 = json
                .get("stepId")
                .and_then(serde_json::Value::as_u64)
                .or_else(|| {
                    tracing::warn!(
                        event_type,
                        field = "stepId",
                        "missing required field in stream event"
                    );
                    None
                })?;
            let step_id = u32::try_from(step_id_u64).unwrap_or(u32::MAX);
            Some(StreamEvent::PlanStepComplete {
                plan_id: PlanId::from(str_field(&json, "planId", event_type)?.to_string()),
                step_id,
                status: str_field(&json, "status", event_type)?.to_string(),
            })
        }
        "plan_complete" => Some(StreamEvent::PlanComplete {
            plan_id: PlanId::from(str_field(&json, "planId", event_type)?.to_string()),
            status: str_field(&json, "status", event_type)?.to_string(),
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
