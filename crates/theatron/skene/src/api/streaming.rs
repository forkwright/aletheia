//! Per-session streaming from `POST /api/v1/sessions/stream`.
//!
//! Each call to [`stream_message`] starts a new HTTP SSE request and returns
//! a receiver that yields [`StreamEvent`]s. The stream is self-terminating:
//! it closes after `TurnComplete` or `TurnAbort`. A stream `error` event is a
//! diagnostic; terminal error turns are represented by `TurnComplete` with
//! `outcome.stop_reason == Some("error")` and `outcome.error.is_some()`.

use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use tokio::sync::mpsc;
use tracing::Instrument;

use koina::http::CONTENT_TYPE_EVENT_STREAM;

use crate::events::{StreamEnvelope, StreamEvent};
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};
use crate::sse::SseStream;

use super::error::{parse_pylon_error_body, parse_retry_after_secs};

/// If no streaming event is received within this window, the connection is
/// treated as hung. Matches the SSE connection's `READ_TIMEOUT` in `sse.rs`
/// for a consistent timeout policy across both connection types.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// Streams a turn response from POST /api/v1/sessions/stream.
/// Returns a channel that yields parsed `StreamEvent`s.
///
/// `client` must be the streaming instance from `ApiClient::streaming_client()`: auth headers
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
    let url = keryx::url::join_base_path(base_url, "/api/v1/sessions/stream");
    let client_turn_id = koina::ulid::Ulid::new().to_string();

    let body = serde_json::json!({
        "message": text,
        "nous_id": nous_id,
        "session_key": session_key,
        "client_turn_id": client_turn_id.clone(),
    });

    let builder = client
        .post(&url)
        .json(&body)
        .header("Accept", CONTENT_TYPE_EVENT_STREAM);

    let span = tracing::info_span!(
        "stream_message",
        nous.id = nous_id,
        session.key = session_key,
        client_turn_id = %client_turn_id
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
            let message = if status == StatusCode::TOO_MANY_REQUESTS {
                let retry = parse_retry_after_secs(resp.headers());
                retry.map_or_else(
                    || "429 rate limited".to_string(),
                    |secs| format!("429 rate limited (retry after {secs}s)"),
                )
            } else {
                let reason = status.canonical_reason().unwrap_or("Unknown");
                let body = match resp.text().await {
                    Ok(body) => body,
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to read stream error response body");
                        String::new()
                    }
                };
                parse_pylon_error_body(&body)
                    .map_or_else(|| format!("{} {}", status.as_u16(), reason), |d| d.message)
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

            if let Some(envelope) = parse_stream_event_envelope(&event.event, &event.data, event.id)
            {
                let is_terminal = matches!(
                    &envelope.payload,
                    StreamEvent::TurnComplete { .. } | StreamEvent::TurnAbort { .. }
                );
                if tx.send(envelope.payload).await.is_err() {
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

/// Parse a raw SSE event into a `StreamEnvelope`.
///
/// Returns `None` only for intentionally-silent events (e.g.
/// `queue_drained`). All other cases — decode failures and unknown event
/// types — are surfaced as [`StreamEvent::DecodeError`] or
/// [`StreamEvent::UnknownEvent`] so they are not silently dropped.
#[expect(
    clippy::too_many_lines,
    reason = "flat match arms; splitting would obscure the 1:1 event-type mapping"
)]
fn parse_stream_event_envelope(
    event_type: &str,
    data: &str,
    event_id: Option<String>,
) -> Option<StreamEnvelope> {
    let wrap = |payload: StreamEvent| -> Option<StreamEnvelope> {
        Some(StreamEnvelope { event_id, payload })
    };

    let json: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event_type, error = %e, "failed to parse stream event JSON");
            return wrap(StreamEvent::DecodeError {
                event_type: event_type.to_string(),
                raw_data: data.to_string(),
                error: e.to_string(),
            });
        }
    };

    match event_type {
        "message_start" | "turn_start" => wrap(StreamEvent::TurnStart {
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
        "text_delta" => wrap(StreamEvent::TextDelta(
            str_field(&json, "text", event_type)?.to_string(),
        )),
        "thinking_delta" => wrap(StreamEvent::ThinkingDelta(
            str_field(&json, "text", event_type)?.to_string(),
        )),
        "tool_use" | "tool_start" => wrap(StreamEvent::ToolStart {
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
            wrap(StreamEvent::ToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
                result,
            })
        }
        "tool_approval_required" => wrap(StreamEvent::ToolApprovalRequired {
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
        "tool_approval_resolved" => wrap(StreamEvent::ToolApprovalResolved {
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
            wrap(StreamEvent::PlanProposed { plan })
        }
        "plan_step_start" => {
            let step_id_u64 = u64_any_field(&json, &["step_id", "stepId"], event_type)?;
            let step_id = u32::try_from(step_id_u64).unwrap_or(u32::MAX);
            wrap(StreamEvent::PlanStepStart {
                plan_id: PlanId::from(
                    str_any_field(&json, &["plan_id", "planId"], event_type)?.to_string(),
                ),
                step_id,
            })
        }
        "plan_step_complete" => {
            let step_id_u64 = u64_any_field(&json, &["step_id", "stepId"], event_type)?;
            let step_id = u32::try_from(step_id_u64).unwrap_or(u32::MAX);
            wrap(StreamEvent::PlanStepComplete {
                plan_id: PlanId::from(
                    str_any_field(&json, &["plan_id", "planId"], event_type)?.to_string(),
                ),
                step_id,
                status: str_field(&json, "status", event_type)?.to_string(),
            })
        }
        "plan_complete" => wrap(StreamEvent::PlanComplete {
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
            wrap(StreamEvent::TurnComplete { outcome })
        }
        "turn_abort" => wrap(StreamEvent::TurnAbort {
            reason: str_field(&json, "reason", event_type)?.to_string(),
        }),
        "error" => wrap(StreamEvent::Error(
            str_field(&json, "message", event_type)?.to_string(),
        )),
        "queue_drained" => {
            // WHY: intentionally silent — queue_drained is a server-side
            // housekeeping event with no semantic meaning for UI consumers.
            tracing::debug!("queue drained: {}", json);
            None
        }
        other => {
            tracing::debug!("unknown stream event: {other}");
            wrap(StreamEvent::UnknownEvent {
                event_type: other.to_string(),
                raw_data: data.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    use crate::api::client::build_streaming_client;

    use super::*;

    fn parse(event_type: &str, data: &str) -> Option<StreamEvent> {
        parse_stream_event_envelope(event_type, data, None).map(|e| e.payload)
    }

    fn parse_with_id(
        event_type: &str,
        data: &str,
        event_id: Option<String>,
    ) -> Option<StreamEnvelope> {
        parse_stream_event_envelope(event_type, data, event_id)
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

    #[test]
    fn parse_text_delta_valid() {
        let data = r#"{"text":"hello"}"#;
        let result = parse("text_delta", data);
        assert!(matches!(result, Some(StreamEvent::TextDelta(ref t)) if t == "hello"));
    }

    #[test]
    fn parse_message_start_snake_case_valid() {
        let data = r#"{"type":"message_start","session_id":"s1","nous_id":"syn","turn_id":"t1"}"#;
        let result = parse("message_start", data);
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
        let result = parse("turn_complete", data);
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
        let result = parse("message_complete", data);
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

    #[tokio::test]
    async fn stream_read_loop_delivers_completion_after_error_event() {
        let body = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"session_id\":\"s1\",\"nous_id\":\"syn\",\"turn_id\":\"t1\"}\n\n",
            "event: error\n",
            "data: {\"type\":\"error\",\"message\":\"provider unavailable\"}\n\n",
            "event: message_complete\n",
            "data: {\"type\":\"message_complete\",\"outcome\":{\"text\":\"\",\"nous_id\":\"syn\",\"session_id\":\"s1\",\"model\":\"mock\",\"tool_calls\":0,\"input_tokens\":7,\"output_tokens\":3,\"cache_read_tokens\":2,\"cache_write_tokens\":1,\"stop_reason\":\"error\",\"error\":\"provider unavailable\"}}\n\n",
        );
        let (base_url, server) = serve_sse_once(body);
        let client = build_streaming_client(None).expect("build streaming test client");
        let mut rx = stream_message(client, &base_url, "syn", "main", "hello");
        let mut events = Vec::new();

        loop {
            let next = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("stream read loop should finish promptly");
            let Some(event) = next else {
                break;
            };
            events.push(event);
        }
        server.join().expect("test server thread should finish");

        assert!(
            matches!(events.get(1), Some(StreamEvent::Error(message)) if message == "provider unavailable"),
            "stream must deliver the diagnostic error before completion: {events:?}"
        );
        let Some(StreamEvent::TurnComplete { outcome }) = events.get(2) else {
            panic!("stream must continue through terminal message_complete: {events:?}");
        };
        assert_eq!(outcome.input_tokens, 7);
        assert_eq!(outcome.output_tokens, 3);
        assert_eq!(outcome.cache_read_tokens, 2);
        assert_eq!(outcome.cache_write_tokens, 1);
        assert_eq!(outcome.stop_reason.as_deref(), Some("error"));
        assert_eq!(outcome.error.as_deref(), Some("provider unavailable"));
    }

    // WHY: message_complete is the turn terminator; an unparseable outcome
    // (model: null from the gateway) silently dropped it and hung the UI.
    #[test]
    fn parse_message_complete_null_model_valid() {
        let data = r#"{"type":"message_complete","outcome":{"text":"done","nous_id":"syn","session_id":"s1","model":null,"tool_calls":0,"input_tokens":1,"output_tokens":1,"cache_read_tokens":0,"cache_write_tokens":0}}"#;
        let result = parse("message_complete", data);
        if let Some(StreamEvent::TurnComplete { outcome }) = result {
            assert!(outcome.model.is_none());
        } else {
            panic!("expected TurnComplete");
        }
    }

    #[test]
    fn parse_tool_result_valid() {
        let data = r#"{"toolName":"exec","toolId":"t1","isError":false,"durationMs":150}"#;
        let result = parse("tool_result", data);
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
        let result = parse("tool_result", data);
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
        let result = parse("tool_use", data);
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

    // ── decode failure + unknown event tests (#4928) ──────────────────────

    #[test]
    fn invalid_json_returns_decode_error() {
        let result = parse("text_delta", "{broken");
        if let Some(StreamEvent::DecodeError {
            event_type,
            raw_data,
            error,
        }) = result
        {
            assert_eq!(event_type, "text_delta");
            assert_eq!(raw_data, "{broken");
            assert!(!error.is_empty());
        } else {
            panic!("expected DecodeError, got {result:?}");
        }
    }

    #[test]
    fn unknown_event_type_returns_unknown_event() {
        let data = r#"{"payload":"custom"}"#;
        let result = parse("custom:extension", data);
        if let Some(StreamEvent::UnknownEvent {
            event_type,
            raw_data,
        }) = result
        {
            assert_eq!(event_type, "custom:extension");
            assert_eq!(raw_data, data);
        } else {
            panic!("expected UnknownEvent, got {result:?}");
        }
    }

    #[test]
    fn queue_drained_returns_none() {
        // Intentionally silent event — no payload for UI consumers.
        let data = r#"{"count":0}"#;
        let result = parse("queue_drained", data);
        assert!(
            result.is_none(),
            "queue_drained must remain None: {result:?}"
        );
    }

    #[test]
    fn event_id_is_threaded_through_envelope() {
        let data = r#"{"text":"hello"}"#;
        let envelope = parse_with_id("text_delta", data, Some("42".to_string()));
        let envelope = envelope.expect("envelope should be Some");
        assert_eq!(envelope.event_id.as_deref(), Some("42"));
        assert!(matches!(envelope.payload, StreamEvent::TextDelta(_)));
    }

    #[test]
    fn decode_error_carries_event_id() {
        let envelope = parse_with_id("text_delta", "{broken", Some("evt-99".to_string()));
        let envelope = envelope.expect("DecodeError envelope should be Some");
        assert_eq!(envelope.event_id.as_deref(), Some("evt-99"));
        assert!(matches!(envelope.payload, StreamEvent::DecodeError { .. }));
    }
}
