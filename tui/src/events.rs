use crate::api::types::SseEvent;

/// Raw events from the three multiplexed sources.
/// Mapped to `Msg` by `App::map_event`.
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
#[derive(Debug)]
pub enum StreamEvent {
    TurnStart {
        session_id: String,
        nous_id: String,
        turn_id: String,
    },
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart {
        tool_name: String,
        tool_id: String,
    },
    ToolResult {
        tool_name: String,
        tool_id: String,
        is_error: bool,
        duration_ms: u64,
    },
    ToolApprovalRequired {
        turn_id: String,
        tool_name: String,
        tool_id: String,
        input: serde_json::Value,
        risk: String,
        reason: String,
    },
    ToolApprovalResolved {
        tool_id: String,
        decision: String,
    },
    PlanProposed {
        plan: crate::api::types::Plan,
    },
    PlanStepStart {
        plan_id: String,
        step_id: u32,
    },
    PlanStepComplete {
        plan_id: String,
        step_id: u32,
        status: String,
    },
    PlanComplete {
        plan_id: String,
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
