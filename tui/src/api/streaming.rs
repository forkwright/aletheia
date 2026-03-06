use futures_util::StreamExt;
use reqwest_eventsource::{Event as EsEvent, EventSource};
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::events::StreamEvent;

/// Streams a turn response from POST /api/sessions/stream.
/// Returns a channel that yields parsed StreamEvents.
pub fn stream_message(
    base_url: &str,
    token: Option<&str>,
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

    let mut builder = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .header("Accept", "text/event-stream");

    if let Some(t) = token {
        builder = builder.bearer_auth(t);
    }

    let span = tracing::info_span!("stream_message");
    tokio::spawn(
        async move {
            let mut es = match EventSource::new(builder) {
                Ok(es) => es,
                Err(e) => {
                    let _ = tx
                        .send(StreamEvent::Error(format!("failed to connect: {e}")))
                        .await;
                    return;
                }
            };

            while let Some(event) = es.next().await {
                match event {
                    Ok(EsEvent::Open) => {}
                    Ok(EsEvent::Message(msg)) => {
                        if let Some(parsed) = parse_stream_event(&msg.event, &msg.data) {
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
                                es.close();
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(StreamEvent::Error(format!("stream error: {e}")))
                            .await;
                        es.close();
                        break;
                    }
                }
            }
        }
        .instrument(span),
    );

    rx
}

fn parse_stream_event(event_type: &str, data: &str) -> Option<StreamEvent> {
    let json: serde_json::Value = serde_json::from_str(data).ok()?;

    match event_type {
        "turn_start" => Some(StreamEvent::TurnStart {
            session_id: json.get("sessionId")?.as_str()?.to_string(),
            nous_id: json.get("nousId")?.as_str()?.to_string(),
            turn_id: json.get("turnId")?.as_str()?.to_string(),
        }),
        "text_delta" => Some(StreamEvent::TextDelta(
            json.get("text")?.as_str()?.to_string(),
        )),
        "thinking_delta" => Some(StreamEvent::ThinkingDelta(
            json.get("text")?.as_str()?.to_string(),
        )),
        "tool_start" => Some(StreamEvent::ToolStart {
            tool_name: json.get("toolName")?.as_str()?.to_string(),
            tool_id: json.get("toolId")?.as_str()?.to_string(),
        }),
        "tool_result" => Some(StreamEvent::ToolResult {
            tool_name: json.get("toolName")?.as_str()?.to_string(),
            tool_id: json.get("toolId")?.as_str()?.to_string(),
            is_error: json.get("isError")?.as_bool()?,
            duration_ms: json.get("durationMs")?.as_u64()?,
        }),
        "tool_approval_required" => Some(StreamEvent::ToolApprovalRequired {
            turn_id: json.get("turnId")?.as_str()?.to_string(),
            tool_name: json.get("toolName")?.as_str()?.to_string(),
            tool_id: json.get("toolId")?.as_str()?.to_string(),
            input: json.get("input")?.clone(),
            risk: json.get("risk")?.as_str()?.to_string(),
            reason: json.get("reason")?.as_str()?.to_string(),
        }),
        "tool_approval_resolved" => Some(StreamEvent::ToolApprovalResolved {
            tool_id: json.get("toolId")?.as_str()?.to_string(),
            decision: json.get("decision")?.as_str()?.to_string(),
        }),
        "plan_proposed" => {
            let plan = serde_json::from_value(json.get("plan")?.clone()).ok()?;
            Some(StreamEvent::PlanProposed { plan })
        }
        "plan_step_start" => Some(StreamEvent::PlanStepStart {
            plan_id: json.get("planId")?.as_str()?.to_string(),
            step_id: json.get("stepId")?.as_u64()? as u32,
        }),
        "plan_step_complete" => Some(StreamEvent::PlanStepComplete {
            plan_id: json.get("planId")?.as_str()?.to_string(),
            step_id: json.get("stepId")?.as_u64()? as u32,
            status: json.get("status")?.as_str()?.to_string(),
        }),
        "plan_complete" => Some(StreamEvent::PlanComplete {
            plan_id: json.get("planId")?.as_str()?.to_string(),
            status: json.get("status")?.as_str()?.to_string(),
        }),
        "turn_complete" => {
            let outcome = serde_json::from_value(json.get("outcome")?.clone()).ok()?;
            Some(StreamEvent::TurnComplete { outcome })
        }
        "turn_abort" => Some(StreamEvent::TurnAbort {
            reason: json.get("reason")?.as_str()?.to_string(),
        }),
        "error" => Some(StreamEvent::Error(
            json.get("message")?.as_str()?.to_string(),
        )),
        "queue_drained" => {
            // Informational — don't need to surface this
            tracing::debug!("queue drained: {}", json);
            None
        }
        other => {
            tracing::debug!("unknown stream event: {other}");
            None
        }
    }
}
