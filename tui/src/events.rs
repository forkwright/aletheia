use crate::api::types::SseEvent;
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};

/// Raw events from the three multiplexed sources.
/// Mapped to `Msg` by `App::map_event`.
#[non_exhaustive]
#[derive(Debug)]
pub enum Event {
    /// Terminal keypress, mouse, resize
    Terminal(crossterm::event::Event),
    /// Global SSE event stream
    Sse(SseEvent),
    /// Per-session streaming response
    Stream(StreamEvent),
    /// 30fps UI tick
    Tick,
}

/// Parsed events from a POST /api/sessions/stream response
#[non_exhaustive]
#[derive(Debug)]
pub enum StreamEvent {
    TurnStart {
        session_id: SessionId,
        nous_id: NousId,
        turn_id: TurnId,
    },
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart {
        tool_name: String,
        tool_id: ToolId,
    },
    ToolResult {
        tool_name: String,
        tool_id: ToolId,
        is_error: bool,
        duration_ms: u64,
    },
    ToolApprovalRequired {
        turn_id: TurnId,
        tool_name: String,
        tool_id: ToolId,
        input: serde_json::Value,
        risk: String,
        reason: String,
    },
    ToolApprovalResolved {
        tool_id: ToolId,
        decision: String,
    },
    PlanProposed {
        plan: crate::api::types::Plan,
    },
    PlanStepStart {
        plan_id: PlanId,
        step_id: u32,
    },
    PlanStepComplete {
        plan_id: PlanId,
        step_id: u32,
        status: String,
    },
    PlanComplete {
        plan_id: PlanId,
        status: String,
    },
    TurnComplete {
        outcome: crate::api::types::TurnOutcome,
    },
    TurnAbort {
        reason: String,
    },
    Error(String),
}
