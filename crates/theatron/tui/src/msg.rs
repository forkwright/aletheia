use crate::api::types::*;
use crate::id::{NousId, SessionId, ToolId, TurnId};

/// Every possible state transition in the application.
/// No I/O happens here: only data describing what happened.
#[derive(Debug)]
#[non_exhaustive]
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
    Yank,
    YankCycle,
    WordForward,
    WordBackward,
    HistorySearchOpen,
    HistorySearchClose,
    HistorySearchInput(char),
    HistorySearchBackspace,
    HistorySearchNext,
    HistorySearchAccept,
    NewlineInsert,
    ClearScreen,
    ClipboardPaste,
    QueuedMessageCancel(usize),
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
    /// Toggle display of successful tool calls (collapse behind "show all").
    OpsToggleShowAll,
    OpenOverlay(OverlayKind),
    CloseOverlay,
    Resize(u16, u16),

    OverlayUp,
    OverlayDown,
    OverlaySelect,
    OverlayFilter(char),
    OverlayFilterBackspace,
    /// Approve a tool call and add it to the session-scoped always-allow list.
    ToolApprovalAlwaysAllow,

    SseConnected,
    SseDisconnected,
    SseInit {
        active_turns: Vec<ActiveTurn>,
    },
    SseTurnBefore {
        nous_id: NousId,
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
    },
    SseStatusUpdate {
        nous_id: NousId,
        status: String,
    },
    SseSessionCreated {
        nous_id: NousId,
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
        nous_id: NousId,
        turn_id: TurnId,
    },
    StreamTextDelta(String),
    StreamThinkingDelta(String),
    StreamToolStart {
        tool_name: String,
        tool_id: ToolId,
        input: Option<serde_json::Value>,
    },
    StreamToolResult {
        tool_name: String,
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
    StreamToolApprovalResolved,
    StreamPlanProposed {
        plan: Plan,
    },
    StreamPlanStepStart {
        step_id: u32,
    },
    StreamPlanStepComplete {
        step_id: u32,
        status: String,
    },
    StreamPlanComplete {
        status: String,
    },
    StreamTurnComplete {
        outcome: TurnOutcome,
    },
    StreamTurnAbort {
        reason: String,
    },
    StreamError(String),

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

    MemoryPageDown,
    MemoryPageUp,
    MemoryDriftTabNext,
    MemoryDriftTabPrev,

    EditorClose,
    EditorCharInput(char),
    EditorNewline,
    EditorBackspace,
    EditorDelete,
    EditorCursorUp,
    EditorCursorDown,
    EditorCursorLeft,
    EditorCursorRight,
    EditorCursorHome,
    EditorCursorEnd,
    EditorPageUp,
    EditorPageDown,
    EditorSave,
    EditorTabNext,
    EditorTabPrev,
    EditorTabClose,
    EditorTreeToggle,
    EditorFocusToggle,

    EditorCut,
    EditorCopy,
    EditorPaste,
    EditorNewFileStart,
    EditorRenameStart,
    EditorDeleteStart,
    EditorConfirmDelete(bool),
    EditorModalCancel,
    EditorRefreshTree,


    ShowError(String),

    /// Cancel the active LLM turn immediately (Esc / Ctrl+C during streaming).
    CancelTurn,

    SlashCompleteOpen,
    SlashCompleteClose,
    SlashCompleteInput(char),
    SlashCompleteBackspace,
    SlashCompleteUp,
    SlashCompleteDown,
    SlashCompleteSelect,

    #[expect(
        dead_code,
        reason = "accessible via :search command; direct keybinding removed"
    )]
    SessionSearchOpen,
    SessionSearchClose,
    SessionSearchInput(char),
    SessionSearchBackspace,

    SessionSearchUp,
    SessionSearchDown,
    SessionSearchSelect,

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "metrics overlay message, constructed in tests only"
        )
    )]
    MetricsClose,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "metrics overlay message, constructed in tests only"
        )
    )]
    MetricsSelectUp,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "metrics overlay message, constructed in tests only"
        )
    )]
    MetricsSelectDown,
    /// Loaded result from async health check triggered on open.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "metrics overlay message, constructed in tests only"
        )
    )]
    MetricsHealthLoaded(bool),









    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageActionKind {
    Copy,
    YankCodeBlock,
    Edit,
    Delete,
    OpenLinks,
    Inspect,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "context action variant, constructed in tests only"
        )
    )]
    QuoteInReply,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "context action variant, constructed in tests only"
        )
    )]
    RateResponse,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "context action variant, constructed in tests only"
        )
    )]
    FlagForReview,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum OverlayKind {
    Help,
    AgentPicker,
    SessionPicker,
    SystemStatus,
    ContextBudget,

}

/// Severity / type of a notification or toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotificationKind {
    Info,
    Warning,
    Error,
    Success,
}

/// Transient error toast that auto-dismisses.
#[derive(Debug, Clone)]
pub struct ErrorToast {
    pub message: String,
    pub created_at: std::time::Instant,
}

impl ErrorToast {
    pub(crate) fn new(message: String) -> Self {
        Self {
            message,
            created_at: std::time::Instant::now(),
        }
    }

    /// Returns true if this toast has been visible long enough (5s).
    pub(crate) fn is_expired(&self) -> bool {
        self.created_at.elapsed() > std::time::Duration::from_secs(5)
    }
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

    #[test]
    fn notification_kind_variants_distinct() {
        let kinds = [
            NotificationKind::Info,
            NotificationKind::Warning,
            NotificationKind::Error,
            NotificationKind::Success,
        ];
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
    fn overlay_kind_notification_history_debug() {
        let kind = OverlayKind::NotificationHistory;
        let debug = format!("{:?}", kind);
        assert!(debug.contains("NotificationHistory"));
    }

    #[test]
    fn slash_complete_msgs_debug() {
        let msgs = [
            Msg::SlashCompleteOpen,
            Msg::SlashCompleteClose,
            Msg::SlashCompleteUp,
            Msg::SlashCompleteDown,
            Msg::SlashCompleteSelect,
            Msg::SlashCompleteInput('q'),
            Msg::SlashCompleteBackspace,
        ];
        for m in &msgs {
            assert!(!format!("{:?}", m).is_empty());
        }
    }
}
