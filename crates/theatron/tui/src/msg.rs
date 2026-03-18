use aletheia_koina::secret::SecretString;

use crate::api::types::*;
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};

/// Every possible state transition in the application.
/// No I/O happens here: only data describing what happened.
#[derive(Debug)]
pub enum Msg {
    CharInput(char),
    Backspace,
    Delete,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    DeleteWord,
    ClearLine,
    DeleteToEnd,
    HistoryUp,
    HistoryDown,
    Submit,
    CopyLastResponse,
    ComposeInEditor,
    Quit,

    CommandPaletteOpen,
    CommandPaletteClose,
    CommandPaletteInput(char),
    CommandPaletteBackspace,
    CommandPaletteDeleteWord,
    CommandPaletteSelect,
    CommandPaletteUp,
    CommandPaletteDown,
    CommandPaletteTab,

    NewSession,
    SessionPickerNewSession,
    SessionPickerArchive,

    TabNew,
    TabClose,
    TabNext,
    TabPrev,
    TabJump(usize),
    GPrefix,

    SelectPrev,
    SelectNext,
    DeselectMessage,
    SelectFirst,
    SelectLast,
    MessageAction(MessageActionKind),

    FilterOpen,
    FilterClose,
    FilterInput(char),
    FilterBackspace,
    FilterClear,
    FilterConfirm,
    FilterNextMatch,
    FilterPrevMatch,

    ScrollUp,
    ScrollDown,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToBottom,
    FocusAgent(NousId),
    NextAgent,
    PrevAgent,

    ViewDrillIn,
    ViewPopBack,

    ToggleSidebar,
    ToggleThinking,
    ToggleOpsPane,
    OpsFocusSwitch,
    OpsScrollUp,
    OpsScrollDown,
    OpsSelectPrev,
    OpsSelectNext,
    OpsToggleExpand,
    OpenOverlay(OverlayKind),
    CloseOverlay,
    Resize(u16, u16),

    OverlayUp,
    OverlayDown,
    OverlaySelect,
    OverlayFilter(char),
    OverlayFilterBackspace,

    SseConnected,
    SseDisconnected,
    SseInit {
        active_turns: Vec<ActiveTurn>,
    },
    SseTurnBefore {
        nous_id: NousId,
        #[expect(dead_code, reason = "planned TUI feature")]
        session_id: SessionId,
        #[expect(dead_code, reason = "planned TUI feature")]
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
        #[expect(dead_code, reason = "planned TUI feature")]
        tool_name: String,
        #[expect(dead_code, reason = "planned TUI feature")]
        error: String,
    },
    SseStatusUpdate {
        nous_id: NousId,
        status: String,
    },
    SseSessionCreated {
        nous_id: NousId,
        #[expect(dead_code, reason = "planned TUI feature")]
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

    StreamTurnStart {
        #[expect(dead_code, reason = "planned TUI feature")]
        session_id: SessionId,
        nous_id: NousId,
        turn_id: TurnId,
    },
    StreamTextDelta(String),
    StreamThinkingDelta(String),
    StreamToolStart {
        tool_name: String,
        #[expect(dead_code, reason = "planned TUI feature")]
        tool_id: ToolId,
        input: Option<serde_json::Value>,
    },
    StreamToolResult {
        tool_name: String,
        #[expect(dead_code, reason = "planned TUI feature")]
        tool_id: ToolId,
        is_error: bool,
        duration_ms: u64,
        result: Option<String>,
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
        #[expect(dead_code, reason = "planned TUI feature")]
        tool_id: ToolId,
        #[expect(dead_code, reason = "planned TUI feature")]
        decision: String,
    },
    StreamPlanProposed {
        plan: Plan,
    },
    StreamPlanStepStart {
        #[expect(dead_code, reason = "planned TUI feature")]
        plan_id: PlanId,
        step_id: u32,
    },
    StreamPlanStepComplete {
        #[expect(dead_code, reason = "planned TUI feature")]
        plan_id: PlanId,
        step_id: u32,
        status: String,
    },
    StreamPlanComplete {
        #[expect(dead_code, reason = "planned TUI feature")]
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

    #[expect(dead_code, reason = "planned TUI feature")]
    AgentsLoaded(Vec<Agent>),
    #[expect(dead_code, reason = "planned TUI feature")]
    SessionsLoaded {
        nous_id: NousId,
        sessions: Vec<Session>,
    },
    #[expect(dead_code, reason = "planned TUI feature")]
    HistoryLoaded {
        session_id: SessionId,
        messages: Vec<HistoryMessage>,
    },
    #[expect(dead_code, reason = "planned TUI feature")]
    CostLoaded {
        daily_total_cents: u32,
    },
    #[expect(dead_code, reason = "planned TUI feature")]
    AuthResult(AuthOutcome),
    #[expect(dead_code, reason = "planned TUI feature")]
    ApiError(String),

    #[expect(dead_code, reason = "planned TUI feature")]
    SettingsLoaded(serde_json::Value),
    #[expect(dead_code, reason = "planned TUI feature")]
    SettingsSaved,
    #[expect(dead_code, reason = "planned TUI feature")]
    SettingsSaveError(String),

    MemoryOpen,
    MemoryClose,
    MemoryTabNext,
    MemoryTabPrev,
    MemorySelectUp,
    MemorySelectDown,
    MemorySelectFirst,
    MemorySelectLast,
    MemorySortCycle,
    MemoryFilterOpen,
    MemoryFilterClose,
    MemoryFilterInput(char),
    MemoryFilterBackspace,
    MemoryDrillIn,
    MemoryPopBack,
    MemoryForget,
    MemoryRestore,
    MemoryEditConfidence,
    MemoryConfidenceInput(char),
    MemoryConfidenceBackspace,
    MemoryConfidenceSubmit,
    MemoryConfidenceCancel,
    MemorySearchOpen,
    MemorySearchInput(char),
    MemorySearchBackspace,
    MemorySearchSubmit,
    MemorySearchClose,
    #[expect(dead_code, reason = "planned TUI feature")]
    MemoryFactsLoaded {
        facts: Vec<crate::state::memory::MemoryFact>,
        total: usize,
    },
    #[expect(dead_code, reason = "planned TUI feature")]
    MemoryDetailLoaded(Box<crate::state::memory::FactDetail>),
    #[expect(dead_code, reason = "planned TUI feature")]
    MemoryEntitiesLoaded(Vec<crate::state::memory::MemoryEntity>),
    #[expect(dead_code, reason = "planned TUI feature")]
    MemoryRelationshipsLoaded(Vec<crate::state::memory::MemoryRelationship>),
    #[expect(dead_code, reason = "planned TUI feature")]
    MemoryTimelineLoaded(Vec<crate::state::memory::MemoryTimelineEvent>),
    #[expect(dead_code, reason = "planned TUI feature")]
    MemorySearchResults(Vec<crate::state::memory::MemorySearchResult>),
    #[expect(dead_code, reason = "planned TUI feature")]
    MemoryActionResult(String),
    MemoryPageDown,
    MemoryPageUp,

    #[allow(dead_code, reason = "planned TUI feature")]
    ShowError(String),
    #[allow(dead_code, reason = "planned TUI feature")]
    ShowSuccess(String),
    #[allow(dead_code, reason = "planned TUI feature")]
    DismissError,

    #[expect(dead_code, reason = "planned TUI feature")]
    ExportConversation,

    SessionSearchOpen,
    SessionSearchClose,
    SessionSearchInput(char),
    SessionSearchBackspace,
    #[expect(dead_code, reason = "planned TUI feature")]
    SessionSearchSubmit,
    SessionSearchUp,
    SessionSearchDown,
    SessionSearchSelect,

    #[expect(dead_code, reason = "planned TUI feature")]
    DiffOpen,
    DiffClose,
    DiffCycleMode,
    DiffScrollUp,
    DiffScrollDown,
    DiffPageUp,
    DiffPageDown,
    /// Auto-triggered diff from file modification tool result.
    #[expect(dead_code, reason = "planned TUI feature")]
    DiffFromToolResult {
        path: String,
        old_content: String,
        new_content: String,
    },

    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageActionKind {
    Copy,
    YankCodeBlock,
    Edit,
    Delete,
    OpenLinks,
    Inspect,
    #[allow(
        dead_code,
        reason = "constructed in context action overlay; lint fires in lib but not test target"
    )]
    QuoteInReply,
    #[allow(
        dead_code,
        reason = "constructed in context action overlay; lint fires in lib but not test target"
    )]
    RateResponse,
    #[allow(
        dead_code,
        reason = "constructed in context action overlay; lint fires in lib but not test target"
    )]
    FlagForReview,
}

#[derive(Debug, Clone)]
pub enum OverlayKind {
    Help,
    AgentPicker,
    SessionPicker,
    #[expect(dead_code, reason = "planned TUI feature")]
    SessionPickerAll,
    SystemStatus,
    #[expect(dead_code, reason = "planned TUI feature")]
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

#[derive(Debug)]
pub enum AuthOutcome {
    #[expect(dead_code, reason = "planned TUI feature")]
    Success { token: SecretString },
    #[expect(dead_code, reason = "planned TUI feature")]
    NoAuthRequired,
    #[expect(dead_code, reason = "planned TUI feature")]
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
