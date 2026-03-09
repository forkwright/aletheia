use crate::api::types::*;
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};

/// Every possible state transition in the application.
/// No I/O happens here — only data describing what happened.
#[non_exhaustive]
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "variant fields carry event data that update handlers read via destructuring; \
              the compiler sees struct-style variant fields as unread when match arms \
              use wildcard or abbreviated patterns"
)]
pub enum Msg {
    // --- Terminal input ---
    CharInput(char),
    Backspace,
    Delete,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    DeleteWord, // Ctrl+W
    ClearLine,  // Ctrl+U
    HistoryUp,
    HistoryDown,
    Submit,           // Enter — send message
    CopyLastResponse, // Ctrl+Y — copy last assistant response to clipboard
    ComposeInEditor,  // Ctrl+E — open $EDITOR for multi-line compose
    Quit,             // Ctrl+C or Ctrl+Q

    // --- Command palette ---
    CommandPaletteOpen,
    CommandPaletteClose,
    CommandPaletteInput(char),
    CommandPaletteBackspace,
    CommandPaletteDeleteWord,
    CommandPaletteSelect,
    CommandPaletteUp,
    CommandPaletteDown,
    CommandPaletteTab,

    NewSession, // Ctrl+N — start new topic

    // --- Message selection ---
    SelectPrev,                       // k or Up in selection mode
    SelectNext,                       // j or Down in selection mode
    DeselectMessage,                  // Esc — return to auto-scroll
    SelectFirst,                      // Home in selection mode
    SelectLast,                       // G or End in selection mode
    OpenContextActions,               // Enter in selection mode — open popup
    MessageAction(MessageActionKind), // Action on selected message

    // --- Filter (`/` mode) ---
    FilterOpen,
    FilterClose,
    FilterInput(char),
    FilterBackspace,
    FilterClear,     // Ctrl+U — clear text, stay in edit mode
    FilterConfirm,   // Enter — lock filter, exit edit mode
    FilterNextMatch, // n — jump to next match
    FilterPrevMatch, // N — jump to previous match

    // --- Navigation ---
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToBottom,
    FocusAgent(NousId),
    NextAgent, // Ctrl+Tab or similar
    PrevAgent,

    // --- Layout ---
    ToggleSidebar,  // Ctrl+F
    ToggleThinking, // Ctrl+T
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
    SseInit {
        active_turns: Vec<ActiveTurn>,
    },
    SseTurnBefore {
        nous_id: NousId,
        session_id: SessionId,
        turn_id: TurnId,
    },
    SseTurnAfter {
        nous_id: NousId,
        session_id: SessionId,
    },
    SseToolCalled {
        nous_id: NousId,
        tool_name: String,
    },
    SseToolFailed {
        nous_id: NousId,
        tool_name: String,
        error: String,
    },
    SseStatusUpdate {
        nous_id: NousId,
        status: String,
    },
    SseSessionCreated {
        nous_id: NousId,
        session_id: SessionId,
    },
    SseSessionArchived {
        nous_id: NousId,
        session_id: SessionId,
    },
    SseDistillBefore {
        nous_id: NousId,
    },
    SseDistillStage {
        nous_id: NousId,
        stage: String,
    },
    SseDistillAfter {
        nous_id: NousId,
    },

    // --- Streaming response events ---
    StreamTurnStart {
        session_id: SessionId,
        nous_id: NousId,
        turn_id: TurnId,
    },
    StreamTextDelta(String),
    StreamThinkingDelta(String),
    StreamToolStart {
        tool_name: String,
        tool_id: ToolId,
    },
    StreamToolResult {
        tool_name: String,
        tool_id: ToolId,
        is_error: bool,
        duration_ms: u64,
    },
    StreamToolApprovalRequired {
        turn_id: TurnId,
        tool_name: String,
        tool_id: ToolId,
        input: serde_json::Value,
        risk: String,
        reason: String,
    },
    StreamToolApprovalResolved {
        tool_id: ToolId,
        decision: String,
    },
    StreamPlanProposed {
        plan: Plan,
    },
    StreamPlanStepStart {
        plan_id: PlanId,
        step_id: u32,
    },
    StreamPlanStepComplete {
        plan_id: PlanId,
        step_id: u32,
        status: String,
    },
    StreamPlanComplete {
        plan_id: PlanId,
        status: String,
    },
    StreamTurnComplete {
        outcome: TurnOutcome,
    },
    StreamTurnAbort {
        reason: String,
    },
    StreamError(String),

    // --- API responses ---
    AgentsLoaded(Vec<Agent>),
    SessionsLoaded {
        nous_id: NousId,
        sessions: Vec<Session>,
    },
    HistoryLoaded {
        session_id: SessionId,
        messages: Vec<HistoryMessage>,
    },
    CostLoaded {
        daily_total_cents: u32,
    },
    AuthResult(AuthOutcome),
    ApiError(String),

    // --- Settings ---
    SettingsLoaded(serde_json::Value),
    SettingsSaved,
    SettingsSaveError(String),

    // --- Errors / toasts ---
    ShowError(String),
    ShowSuccess(String),
    DismissError,

    // --- Timer ---
    Tick,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageActionKind {
    Copy,
    YankCodeBlock,
    Edit,
    Delete,
    OpenLinks,
    Inspect,
    QuoteInReply,
    RateResponse,
    FlagForReview,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum OverlayKind {
    Help,
    AgentPicker,
    SystemStatus,
    #[expect(dead_code, reason = "opened via command palette")]
    Settings,
}

/// Transient error toast that auto-dismisses.
#[derive(Debug, Clone)]
pub struct ErrorToast {
    pub message: String,
    pub created_at: std::time::Instant,
}

impl ErrorToast {
    pub fn new(message: String) -> Self {
        Self {
            message,
            created_at: std::time::Instant::now(),
        }
    }

    /// Returns true if this toast has been visible long enough (5s).
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > std::time::Duration::from_secs(5)
    }
}

#[non_exhaustive]
#[derive(Debug)]
#[expect(dead_code, reason = "auth flow variants")]
pub enum AuthOutcome {
    Success { token: String },
    NoAuthRequired,
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_toast_is_not_expired_immediately() {
        let toast = ErrorToast::new("test".to_string());
        assert!(!toast.is_expired());
    }

    #[test]
    fn error_toast_message_stored() {
        let toast = ErrorToast::new("hello world".to_string());
        assert_eq!(toast.message, "hello world");
    }

    #[test]
    fn message_action_kind_all_variants() {
        let kinds = [
            MessageActionKind::Copy,
            MessageActionKind::YankCodeBlock,
            MessageActionKind::Edit,
            MessageActionKind::Delete,
            MessageActionKind::OpenLinks,
            MessageActionKind::Inspect,
            MessageActionKind::QuoteInReply,
            MessageActionKind::RateResponse,
            MessageActionKind::FlagForReview,
        ];
        // Verify Debug trait works and variants are distinct
        let debugs: Vec<String> = kinds.iter().map(|k| format!("{:?}", k)).collect();
        for (i, d) in debugs.iter().enumerate() {
            for (j, d2) in debugs.iter().enumerate() {
                if i != j {
                    assert_ne!(d, d2);
                }
            }
        }
    }

    #[test]
    fn overlay_kind_debug() {
        let kinds = [
            OverlayKind::Help,
            OverlayKind::AgentPicker,
            OverlayKind::SystemStatus,
        ];
        for kind in &kinds {
            let debug = format!("{:?}", kind);
            assert!(!debug.is_empty());
        }
    }

    #[test]
    fn msg_quit_debug() {
        let msg = Msg::Quit;
        let debug = format!("{:?}", msg);
        assert!(debug.contains("Quit"));
    }

    #[test]
    fn msg_tick_debug() {
        let msg = Msg::Tick;
        let debug = format!("{:?}", msg);
        assert!(debug.contains("Tick"));
    }
}
