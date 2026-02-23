use crate::api::types::*;

/// Every possible state transition in the application.
/// No I/O happens here — only data describing what happened.
#[derive(Debug)]
pub enum Msg {
    // --- Terminal input ---
    CharInput(char),
    Backspace,
    Delete,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    DeleteWord,   // Ctrl+W
    ClearLine,    // Ctrl+U
    HistoryUp,
    HistoryDown,
    Submit,       // Enter — send message
    Quit,         // Ctrl+C or Ctrl+Q

    // --- Navigation ---
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToBottom,
    FocusAgent(String),
    NextAgent,    // Ctrl+Tab or similar
    PrevAgent,

    // --- Layout ---
    ToggleSidebar,     // Ctrl+F
    ToggleThinking,    // Ctrl+T
    OpenOverlay(OverlayKind),
    CloseOverlay,
    Resize(u16, u16),

    // --- Overlay interaction ---
    OverlayUp,
    OverlayDown,
    OverlaySelect,
    OverlayFilter(char),
    OverlayFilterBackspace,

    // --- SSE global events ---
    SseConnected,
    SseDisconnected,
    SseInit { active_turns: Vec<ActiveTurn> },
    SseTurnBefore { nous_id: String, session_id: String, turn_id: String },
    SseTurnAfter { nous_id: String, session_id: String },
    SseToolCalled { nous_id: String, tool_name: String },
    SseToolFailed { nous_id: String, tool_name: String, error: String },
    SseStatusUpdate { nous_id: String, status: String },
    SseSessionCreated { nous_id: String, session_id: String },
    SseSessionArchived { nous_id: String, session_id: String },
    SseDistillBefore { nous_id: String },
    SseDistillStage { nous_id: String, stage: String },
    SseDistillAfter { nous_id: String },

    // --- Streaming response events ---
    StreamTurnStart { session_id: String, nous_id: String, turn_id: String },
    StreamTextDelta(String),
    StreamThinkingDelta(String),
    StreamToolStart { tool_name: String, tool_id: String },
    StreamToolResult { tool_name: String, tool_id: String, is_error: bool, duration_ms: u64 },
    StreamToolApprovalRequired {
        turn_id: String,
        tool_name: String,
        tool_id: String,
        input: serde_json::Value,
        risk: String,
        reason: String,
    },
    StreamToolApprovalResolved { tool_id: String, decision: String },
    StreamPlanProposed { plan: Plan },
    StreamPlanStepStart { plan_id: String, step_id: u32 },
    StreamPlanStepComplete { plan_id: String, step_id: u32, status: String },
    StreamPlanComplete { plan_id: String, status: String },
    StreamTurnComplete { outcome: TurnOutcome },
    StreamTurnAbort { reason: String },
    StreamError(String),

    // --- API responses ---
    AgentsLoaded(Vec<Agent>),
    SessionsLoaded { nous_id: String, sessions: Vec<Session> },
    HistoryLoaded { session_id: String, messages: Vec<HistoryMessage> },
    CostLoaded { daily_total_cents: u32 },
    AuthResult(AuthOutcome),
    ApiError(String),

    // --- Timer ---
    Tick,
}

#[derive(Debug, Clone)]
pub enum OverlayKind {
    Help,
    AgentPicker,
    SessionBrowser,
    CostSummary,
    SystemStatus,
}

#[derive(Debug)]
pub enum AuthOutcome {
    Success { token: String },
    NoAuthRequired,
    Failed(String),
}
