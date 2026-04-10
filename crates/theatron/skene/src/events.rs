//! Parsed streaming events from the per-session SSE endpoint.

use crate::api::types::{Plan, TurnOutcome};
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};

/// Parsed events from a `POST /api/v1/sessions/stream` response.
#[derive(Debug)]
#[non_exhaustive]
pub enum StreamEvent {
    /// Turn started: carries session, agent, and turn identifiers.
    TurnStart {
        /// Session this turn belongs to.
        session_id: SessionId,
        /// Agent processing this turn.
        nous_id: NousId,
        /// Unique identifier for this turn.
        turn_id: TurnId,
    },
    /// Incremental text output from the model.
    TextDelta(String),
    /// Incremental extended-thinking output from the model.
    ThinkingDelta(String),
    /// A tool invocation has started.
    ToolStart {
        /// Name of the tool being invoked.
        tool_name: String,
        /// Unique identifier for this tool call.
        tool_id: ToolId,
        /// Tool input parameters, if available.
        input: Option<serde_json::Value>,
    },
    /// A tool invocation has completed.
    ToolResult {
        /// Name of the tool that completed.
        tool_name: String,
        /// Unique identifier for this tool call.
        tool_id: ToolId,
        /// Whether the tool returned an error.
        is_error: bool,
        /// Wall-clock duration of the tool call in milliseconds.
        duration_ms: u64,
        /// Tool output text, if available.
        result: Option<String>,
    },
    /// The server is waiting for user approval of a tool call.
    ToolApprovalRequired {
        /// Turn that owns this tool call.
        turn_id: TurnId,
        /// Name of the tool awaiting approval.
        tool_name: String,
        /// Unique identifier for this tool call.
        tool_id: ToolId,
        /// Tool input parameters.
        input: serde_json::Value,
        /// Risk level assigned by the server.
        risk: String,
        /// Human-readable reason for requiring approval.
        reason: String,
    },
    /// A tool approval decision has been resolved.
    ToolApprovalResolved {
        /// Tool call that was resolved.
        tool_id: ToolId,
        /// Decision: "approved" or "denied".
        decision: String,
    },
    /// The server has proposed a multi-step plan for approval.
    PlanProposed {
        /// The proposed plan.
        plan: Plan,
    },
    /// A plan step has started executing.
    PlanStepStart {
        /// Plan this step belongs to.
        plan_id: PlanId,
        /// Step index within the plan.
        step_id: u32,
    },
    /// A plan step has completed.
    PlanStepComplete {
        /// Plan this step belongs to.
        plan_id: PlanId,
        /// Step index within the plan.
        step_id: u32,
        /// Completion status of the step.
        status: String,
    },
    /// The entire plan has completed.
    PlanComplete {
        /// Plan that completed.
        plan_id: PlanId,
        /// Overall completion status.
        status: String,
    },
    /// The turn has completed successfully.
    TurnComplete {
        /// Summary of the completed turn.
        outcome: TurnOutcome,
    },
    /// The turn was aborted (by user or server).
    TurnAbort {
        /// Reason the turn was aborted.
        reason: String,
    },
    /// An error occurred during streaming.
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_event_text_delta_holds_string() {
        let event = StreamEvent::TextDelta("hello".to_string());
        if let StreamEvent::TextDelta(text) = event {
            assert_eq!(text, "hello");
        } else {
            panic!("expected TextDelta");
        }
    }

    #[test]
    fn stream_event_error_holds_message() {
        let event = StreamEvent::Error("connection lost".to_string());
        if let StreamEvent::Error(msg) = event {
            assert_eq!(msg, "connection lost");
        } else {
            panic!("expected Error");
        }
    }

    #[test]
    fn stream_event_turn_start_fields() {
        let event = StreamEvent::TurnStart {
            session_id: "s1".into(),
            nous_id: "n1".into(),
            turn_id: "t1".into(),
        };
        if let StreamEvent::TurnStart {
            session_id,
            nous_id,
            turn_id,
        } = event
        {
            assert!(session_id == *"s1");
            assert!(nous_id == *"n1");
            assert!(turn_id == *"t1");
        }
    }

    #[test]
    fn stream_event_tool_result_fields() {
        let event = StreamEvent::ToolResult {
            tool_name: "read_file".to_string(),
            tool_id: "t1".into(),
            is_error: true,
            duration_ms: 150,
            result: None,
        };
        if let StreamEvent::ToolResult {
            tool_name,
            is_error,
            duration_ms,
            ..
        } = event
        {
            assert_eq!(tool_name, "read_file");
            assert!(is_error);
            assert_eq!(duration_ms, 150);
        }
    }
}
