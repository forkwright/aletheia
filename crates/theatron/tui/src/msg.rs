use aletheia_koina::secret::SecretString;

use crate::api::types::*;
use crate::id::{NousId, PlanId, SessionId, ToolId, TurnId};

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

    #[expect(dead_code, reason = "opened via :editor command, not Msg pipeline")]
    EditorOpen,
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
    #[expect(dead_code, reason = "tree expand triggered via Enter on directory")]
    EditorTreeExpand,
    EditorCut,
    EditorCopy,
    EditorPaste,
    EditorNewFileStart,
    EditorRenameStart,
    EditorDeleteStart,
    EditorConfirmDelete(bool),
    EditorModalCancel,
    EditorRefreshTree,
    #[expect(dead_code, reason = "triggered by Tick handler, not keyboard")]
    EditorAutosaveTick,
    #[expect(dead_code, reason = "triggered by render, not keyboard")]
    EditorScrollTree(usize),

    // WHY: #[allow] over #[expect] — constructed only in test code so dead_code fires for
    // the lib target but not the test target; #[expect] would cause unfulfilled-expectation
    // errors in the test compilation unit.
    #[allow(dead_code)]
    ShowError(String),
    #[allow(dead_code)]
    ShowSuccess(String),
    #[allow(dead_code)]
    DismissError,

    #[expect(dead_code, reason = "planned TUI feature")]
    ExportConversation,

    /// Cancel the active LLM turn immediately (Esc / Ctrl+C during streaming).
    CancelTurn,

    SlashCompleteOpen,
    SlashCompleteClose,
    SlashCompleteInput(char),
    SlashCompleteBackspace,
    SlashCompleteUp,
    SlashCompleteDown,
    SlashCompleteSelect,

    #[expect(dead_code, reason = "sent by API event bridge; not yet wired")]
    ToastPush {
        message: String,
        kind: NotificationKind,
        duration_secs: u64,
    },
    #[expect(dead_code, reason = "sent by API event bridge; not yet wired")]
    ErrorBannerSet(String),
    #[expect(dead_code, reason = "sent by API event bridge; not yet wired")]
    ErrorBannerDismiss,

    #[expect(
        dead_code,
        reason = "accessible via :search command; direct keybinding removed"
    )]
    SessionSearchOpen,
    SessionSearchClose,
    SessionSearchInput(char),
    SessionSearchBackspace,
    #[expect(dead_code, reason = "planned TUI feature")]
    SessionSearchSubmit,
    SessionSearchUp,
    SessionSearchDown,
    SessionSearchSelect,

    // WHY: #[allow] over #[expect] — constructed in tests only; see ShowError note above.
    #[allow(dead_code)]
    MetricsOpen,
    #[allow(dead_code)]
    MetricsClose,
    #[allow(dead_code)]
    MetricsSelectUp,
    #[allow(dead_code)]
    MetricsSelectDown,
    /// Loaded result from async health check triggered on open.
    #[allow(dead_code)]
    MetricsHealthLoaded(bool),

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

    #[expect(dead_code, reason = "planned TUI feature: key bindings not yet wired")]
    DecisionCardNextField,
    #[expect(dead_code, reason = "planned TUI feature: key bindings not yet wired")]
    DecisionCardPrevField,
    #[expect(dead_code, reason = "planned TUI feature")]
    StreamDecisionRequired {
        question: String,
        options: Vec<(String, Option<String>, bool)>,
    },

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
    // WHY: #[allow] over #[expect] — constructed only in test code so dead_code fires for
    // the lib target but not the test target; #[expect] would cause unfulfilled-expectation
    // errors in the test compilation unit.
    #[allow(dead_code)]
    QuoteInReply,
    #[allow(dead_code)]
    RateResponse,
    #[allow(dead_code)]
    FlagForReview,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum OverlayKind {
    Help,
    AgentPicker,
    SessionPicker,
    #[expect(dead_code, reason = "planned TUI feature")]
    SessionPickerAll,
    SystemStatus,
    ContextBudget,
    #[expect(dead_code, reason = "planned TUI feature")]
    Settings,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "opened programmatically; not yet wired to keyboard"
        )
    )]
    NotificationHistory,
}

/// Severity / type of a notification or toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotificationKind {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "API bridge sends these; not yet wired")
    )]
    Info,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "API bridge sends these; not yet wired")
    )]
    Warning,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "API bridge sends these; not yet wired")
    )]
    Error,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "API bridge sends these; not yet wired")
    )]
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

#[derive(Debug)]
#[non_exhaustive]
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
