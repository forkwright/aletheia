//! Per-session streaming from `POST /api/v1/sessions/stream`.
//!
//! Each call to `stream_turn` starts a new HTTP SSE request and returns
//! a receiver that yields `StreamEvent`s. The stream is self-terminating:
//! it closes after `TurnComplete` or `TurnAbort`. A stream `error` event is
//! diagnostic; terminal failed turns are represented by `TurnComplete` with
//! `outcome.stop_reason == Some("error")` and `outcome.error.is_some()`.
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
//! let mut rx = stream_turn(client, &url, &agent, &key, &msg, &turn_id, cancel.child_token());
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

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Client;
use skene::api::streaming::STREAM_READ_TIMEOUT;
use skene::sse::SseStream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use skene::api::error::{format_error_fields_for_display, format_http_error_body};
use skene::events::{
    LEGACY_APPROVAL_DEFAULT_DECISION, LEGACY_APPROVAL_TIMEOUT_SECS, StreamEvent,
};
use skene::id::{NousId, PlanId, SessionId, ToolId, TurnId};

struct StreamTurnRequest<'a> {
    base_url: &'a str,
    nous_id: &'a str,
    session_key: &'a str,
    message: &'a str,
    client_turn_id: &'a str,
}

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
    client_turn_id: &str,
    cancel: CancellationToken,
) -> mpsc::Receiver<StreamEvent> {
    stream_turn_with_read_timeout(
        client,
        StreamTurnRequest {
            base_url,
            nous_id,
            session_key,
            message,
            client_turn_id,
        },
        cancel,
        STREAM_READ_TIMEOUT,
    )
}

fn stream_turn_with_read_timeout(
    client: Client,
    request: StreamTurnRequest<'_>,
    cancel: CancellationToken,
    read_timeout: Duration,
) -> mpsc::Receiver<StreamEvent> {
    let (tx, rx) = mpsc::channel(256);
    let url = format!(
        "{}/api/v1/sessions/stream",
        request.base_url.trim_end_matches('/')
    );

    let body = serde_json::json!({
        "message": request.message,
        "nous_id": request.nous_id,
        "session_key": request.session_key,
        "client_turn_id": request.client_turn_id,
    });

    let builder = client
        .post(&url)
        .json(&body)
        .header("Accept", "text/event-stream");

    let span = tracing::info_span!("stream_turn");
    let task = async move {
        let resp = match tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                tracing::info!("stream cancelled before connect");
                if tx.send(StreamEvent::TurnAbort {
                    reason: "cancelled by user".to_string(),
                }).await.is_err() {
                    tracing::debug!("stream receiver dropped before cancellation notice");
                }
                return;
            }
            result = builder.send() => result,
        } {
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
            let message = extract_error_message(&body, status.as_u16(), reason);
            if tx.send(StreamEvent::Error(message)).await.is_err() {
                tracing::debug!("stream receiver dropped before HTTP error");
            }
            return;
        }

        let mut es = SseStream::new(resp.bytes_stream());

        loop {
            let maybe_event = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    tracing::info!("stream cancelled by user");
                    if tx.send(StreamEvent::TurnAbort {
                        reason: "cancelled by user".to_string(),
                    }).await.is_err() {
                        tracing::debug!("stream receiver dropped before cancellation notice");
                    }
                    return;
                }
                event = tokio::time::timeout(read_timeout, es.next()) => event,
            };

            let event = match maybe_event {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_elapsed) => {
                    // WHY(#4564): Desktop per-turn streaming must use the
                    // same stale-read policy as skene so a silent SSE stream
                    // cannot outlive the UI indefinitely.
                    tracing::warn!(
                        timeout_secs = read_timeout.as_secs(),
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
                    StreamEvent::TurnComplete { .. } | StreamEvent::TurnAbort { .. }
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

/// Extract a human-readable error message from an HTTP error response body.
fn extract_error_message(body: &str, status_code: u16, reason: &str) -> String {
    format_http_error_body(status_code, reason, body)
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

fn u32_field(json: &serde_json::Value, field: &str, event_type: &str) -> Option<u32> {
    json.get(field)
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .or_else(|| {
            tracing::warn!(
                event_type,
                field,
                "missing or invalid u32 field in stream event"
            );
            None
        })
}

fn optional_u32_any_field(json: &serde_json::Value, fields: &[&str]) -> Option<u32> {
    fields
        .iter()
        .find_map(|field| json.get(field).and_then(serde_json::Value::as_u64))
        .and_then(|value| u32::try_from(value).ok())
}

fn optional_str_any_field<'a>(json: &'a serde_json::Value, fields: &[&str]) -> Option<&'a str> {
    fields
        .iter()
        .find_map(|field| json.get(field).and_then(serde_json::Value::as_str))
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
            let is_error = json
                .get("is_error")
                .or_else(|| json.get("isError"))
                .and_then(|v| v.as_bool())
                .or_else(|| {
                    tracing::warn!(
                        event_type,
                        field = "is_error",
                        "missing required field in stream event"
                    );
                    None
                })?;
            let duration_ms = json
                .get("duration_ms")
                .or_else(|| json.get("durationMs"))
                .and_then(|v| v.as_u64())
                .or_else(|| {
                    tracing::warn!(
                        event_type,
                        field = "duration_ms",
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
            timeout_secs: optional_u32_any_field(&json, &["timeout_secs", "timeoutSecs"])
                .unwrap_or(LEGACY_APPROVAL_TIMEOUT_SECS),
            default_decision: optional_str_any_field(
                &json,
                &["default_decision", "defaultDecision"],
            )
            .unwrap_or(LEGACY_APPROVAL_DEFAULT_DECISION)
            .to_string(),
        }),
        "tool_approval_resolved" => Some(StreamEvent::ToolApprovalResolved {
            tool_id: ToolId::from(
                str_any_field(&json, &["tool_id", "toolId"], event_type)?.to_string(),
            ),
            decision: str_field(&json, "decision", event_type)?.to_string(),
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
        "error" => Some(StreamEvent::Error(stream_error_message(&json, event_type)?)),
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
        "plan_step_start" => Some(StreamEvent::PlanStepStart {
            plan_id: PlanId::from(
                str_any_field(&json, &["plan_id", "planId"], event_type)?.to_string(),
            ),
            step_id: u32_field(&json, "step_id", event_type)
                .or_else(|| u32_field(&json, "stepId", event_type))?,
        }),
        "plan_step_complete" => Some(StreamEvent::PlanStepComplete {
            plan_id: PlanId::from(
                str_any_field(&json, &["plan_id", "planId"], event_type)?.to_string(),
            ),
            step_id: u32_field(&json, "step_id", event_type)
                .or_else(|| u32_field(&json, "stepId", event_type))?,
            status: str_field(&json, "status", event_type)?.to_string(),
        }),
        "plan_complete" => Some(StreamEvent::PlanComplete {
            plan_id: PlanId::from(
                str_any_field(&json, &["plan_id", "planId"], event_type)?.to_string(),
            ),
            status: str_field(&json, "status", event_type)?.to_string(),
        }),
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

fn stream_error_message(json: &serde_json::Value, event_type: &str) -> Option<String> {
    let message = str_field(json, "message", event_type)?;
    Some(format_error_fields_for_display(
        message,
        None,
        json.get("code").and_then(serde_json::Value::as_str),
        json.get("request_id")
            .or_else(|| json.get("requestId"))
            .and_then(serde_json::Value::as_str),
        json.get("details"),
    ))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc as std_mpsc;

    use super::*;

    struct HangingSseServer {
        base_url: String,
        ready_rx: Option<std_mpsc::Receiver<()>>,
        done_tx: std_mpsc::Sender<()>,
        handle: std::thread::JoinHandle<()>,
    }

    impl HangingSseServer {
        async fn wait_ready(&mut self) {
            let ready_rx = self
                .ready_rx
                .take()
                .expect("ready receiver should only be awaited once");
            tokio::task::spawn_blocking(move || ready_rx.recv_timeout(Duration::from_secs(2)))
                .await
                .expect("ready wait task should finish")
                .expect("SSE response headers should be sent");
        }

        fn finish(self) {
            match self.done_tx.send(()) {
                Ok(()) => {}
                Err(_closed) => {}
            }
            self.handle
                .join()
                .expect("hanging SSE server thread should finish");
        }
    }

    struct ScriptedSseServer {
        base_url: String,
        handle: std::thread::JoinHandle<()>,
    }

    impl ScriptedSseServer {
        fn finish(self) {
            self.handle
                .join()
                .expect("scripted SSE server thread should finish");
        }
    }

    fn install_crypto() {
        // Another test may already have installed the process-wide provider.
        match rustls::crypto::ring::default_provider().install_default() {
            Ok(()) => {}
            Err(_already_installed) => {}
        }
    }

    fn streaming_test_client() -> Client {
        install_crypto();
        Client::builder()
            .build()
            .expect("build streaming test client")
    }

    fn serve_hanging_sse_once() -> HangingSseServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let addr = listener.local_addr().expect("read local test server addr");
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let (done_tx, done_rx) = std_mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set request read timeout");
            let mut buf = [0_u8; 2048];
            let read = stream.read(&mut buf).expect("read stream request");
            assert!(read > 0, "client should send an HTTP request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: keep-alive\r\n\r\n",
                )
                .expect("write SSE response headers");
            stream.flush().expect("flush SSE response headers");
            ready_tx.send(()).expect("signal SSE response ready");
            match done_rx.recv_timeout(Duration::from_secs(2)) {
                Ok(()) => {}
                Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                Err(std_mpsc::RecvTimeoutError::Disconnected) => {}
            }
        });

        HangingSseServer {
            base_url: format!("http://{addr}"),
            ready_rx: Some(ready_rx),
            done_tx,
            handle,
        }
    }

    fn serve_sse_response_once(body: &'static str) -> ScriptedSseServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test server");
        let addr = listener.local_addr().expect("read local test server addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set request read timeout");
            let mut buf = [0_u8; 2048];
            let read = stream.read(&mut buf).expect("read stream request");
            assert!(read > 0, "client should send an HTTP request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
                )
                .expect("write SSE response headers");
            stream
                .write_all(body.as_bytes())
                .expect("write scripted SSE body");
            stream.flush().expect("flush scripted SSE response");
        });

        ScriptedSseServer {
            base_url: format!("http://{addr}"),
            handle,
        }
    }

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
    // (model: null from the gateway) silently dropped it and hung the spinner.
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
    fn parse_tool_result_snake_case_valid() {
        let data = r#"{"type":"tool_result","tool_name":"exec","tool_id":"t1","is_error":false,"duration_ms":150,"result":"ok"}"#;
        let result = parse_stream_event("tool_result", data);
        if let Some(StreamEvent::ToolResult {
            tool_name,
            tool_id,
            is_error,
            duration_ms,
            result: tool_result,
        }) = result
        {
            assert_eq!(tool_name, "exec");
            assert_eq!(&*tool_id, "t1");
            assert!(!is_error);
            assert_eq!(duration_ms, 150);
            assert_eq!(tool_result.as_deref(), Some("ok"));
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
    fn parse_tool_use_snake_case_valid() {
        let data = r#"{"type":"tool_use","tool_name":"read_file","tool_id":"t1","input":{"path":"README.md"}}"#;
        let result = parse_stream_event("tool_use", data);
        if let Some(StreamEvent::ToolStart {
            tool_name,
            tool_id,
            input,
        }) = result
        {
            assert_eq!(tool_name, "read_file");
            assert_eq!(&*tool_id, "t1");
            assert!(input.is_some());
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
    fn parse_error_event_preserves_code_request_id_and_details() {
        let data = r#"{"code":"provider_unavailable","message":"provider unavailable","request_id":"req-stream","details":{"provider":"synthetic"}}"#;
        let result = parse_stream_event("error", data);
        let Some(StreamEvent::Error(message)) = result else {
            panic!("expected Error");
        };
        assert!(message.contains("provider unavailable"));
        assert!(message.contains("code provider_unavailable"));
        assert!(message.contains("request_id req-stream"));
        assert!(message.contains(r#""provider":"synthetic""#));
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

    #[tokio::test]
    async fn stream_turn_read_timeout_emits_error_and_closes() {
        let mut server = serve_hanging_sse_once();
        let client = streaming_test_client();
        let cancel = CancellationToken::new();
        let mut rx = stream_turn_with_read_timeout(
            client,
            StreamTurnRequest {
                base_url: &server.base_url,
                nous_id: "syn",
                session_key: "main",
                message: "hello",
                client_turn_id: "turn-timeout",
            },
            cancel,
            Duration::from_millis(50),
        );

        server.wait_ready().await;
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("read timeout error should arrive promptly")
            .expect("timeout should be delivered as a stream event");

        assert!(matches!(event, StreamEvent::Error(ref message) if message == "stream timeout"));
        assert!(
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("stream should close after timeout")
                .is_none()
        );
        server.finish();
    }

    #[tokio::test]
    async fn stream_turn_continues_after_error_until_message_complete() {
        let server = serve_sse_response_once(concat!(
            "event: error\n",
            "data: {\"type\":\"error\",\"code\":\"provider_error\",\"message\":\"provider unavailable\",\"request_id\":\"req-1\"}\n\n",
            "event: message_complete\n",
            "data: {\"type\":\"message_complete\",\"outcome\":{\"text\":\"\",\"nous_id\":\"syn\",\"session_id\":\"s1\",\"model\":\"mock\",\"tool_calls\":0,\"input_tokens\":0,\"output_tokens\":0,\"cache_read_tokens\":0,\"cache_write_tokens\":0,\"stop_reason\":\"error\",\"error\":\"provider unavailable\"}}\n\n",
        ));
        let client = streaming_test_client();
        let cancel = CancellationToken::new();
        let mut rx = stream_turn_with_read_timeout(
            client,
            StreamTurnRequest {
                base_url: &server.base_url,
                nous_id: "syn",
                session_key: "main",
                message: "hello",
                client_turn_id: "turn-error-complete",
            },
            cancel,
            Duration::from_secs(2),
        );

        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("diagnostic error should arrive promptly")
            .expect("diagnostic error should be delivered as a stream event");
        assert!(
            matches!(event, StreamEvent::Error(ref message) if message.contains("provider unavailable"))
        );

        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("message_complete should arrive after diagnostic error")
            .expect("message_complete should be delivered as a stream event");
        let StreamEvent::TurnComplete { outcome } = event else {
            panic!("expected TurnComplete after diagnostic error");
        };
        assert_eq!(outcome.stop_reason.as_deref(), Some("error"));
        assert_eq!(outcome.error.as_deref(), Some("provider unavailable"));
        server.finish();
    }

    #[tokio::test]
    async fn stream_turn_cancellation_emits_abort_and_closes() {
        let mut server = serve_hanging_sse_once();
        let client = streaming_test_client();
        let cancel = CancellationToken::new();
        let mut rx = stream_turn_with_read_timeout(
            client,
            StreamTurnRequest {
                base_url: &server.base_url,
                nous_id: "syn",
                session_key: "main",
                message: "hello",
                client_turn_id: "turn-cancel",
            },
            cancel.clone(),
            Duration::from_secs(5),
        );

        server.wait_ready().await;
        cancel.cancel();
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("cancellation should arrive promptly")
            .expect("cancellation should be delivered as a stream event");

        assert!(
            matches!(event, StreamEvent::TurnAbort { ref reason } if reason == "cancelled by user")
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("stream should close after cancellation")
                .is_none()
        );
        server.finish();
    }

    #[test]
    fn parse_tool_approval_required() {
        let data = r#"{
            "turnId": "t1",
            "toolName": "exec",
            "toolId": "tool-1",
            "input": {"command": "rm -rf /"},
            "risk": "high",
            "reason": "destructive command",
            "timeoutSecs": 45,
            "defaultDecision": "denied"
        }"#;
        let result = parse_stream_event("tool_approval_required", data);
        if let Some(StreamEvent::ToolApprovalRequired {
            tool_name,
            risk,
            reason,
            timeout_secs,
            default_decision,
            ..
        }) = result
        {
            assert_eq!(tool_name, "exec");
            assert_eq!(risk, "high");
            assert_eq!(reason, "destructive command");
            assert_eq!(timeout_secs, 45);
            assert_eq!(default_decision, "denied");
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
    fn extract_error_message_preserves_pylon_envelope() {
        let body = r#"{"error":{"code":"validation_error","message":"invalid stream request","request_id":"req-http","details":{"errors":[{"field":"message","code":"required","message":"message is required"}]}}}"#;
        let message = extract_error_message(body, 422, "Unprocessable Entity");
        assert!(message.contains("invalid stream request"));
        assert!(message.contains("status 422"));
        assert!(message.contains("code validation_error"));
        assert!(message.contains("request_id req-http"));
        assert!(message.contains(r#""field":"message""#));
    }

    #[test]
    fn extract_error_message_fallback() {
        assert_eq!(
            extract_error_message("not json", 500, "Internal"),
            "500 Internal"
        );
    }
}
