# L3 API Index: theatron

Crate path: `crates/theatron`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `koilon/src/app/mod.rs`

> Agent roster, sessions, messages, and cost tracking.
```rust
pub struct DashboardState {
    pub agents: Vec<AgentState>,
    pub focused_agent: Option<NousId>,
    /// PERF: ArcVec clone is O(1): tab switches share the Arc pointer, not the Vec.
    pub messages: ArcVec<ChatMessage>,
    pub focused_session_id: Option<SessionId>,
    pub daily_cost_cents: u32,
    pub session_cost_cents: u32,
    pub context_usage_pct: Option<u8>,
    /// Raw token count currently in the context window (input + cache-read).
    pub context_tokens_used: Option<u32>,
    /// Total context window capacity for the current model.
    pub context_tokens_total: Option<u32>,
    /// Last-active session per agent, loaded from disk on startup and saved on exit.
    pub(crate) saved_sessions: HashMap<NousId, SessionId>,
    pub submitted_decisions: Vec<crate::state::SubmittedDecision>,
}
```

> SSE link, stream receiver, and reconnect bookkeeping.
```rust
pub struct ConnectionState {
    sse: Option<SseConnection>,
    pub sse_connected: bool,
    /// When the SSE stream last disconnected. Cleared on successful reconnect.
    pub sse_disconnected_at: Option<std::time::Instant>,
    /// Timestamp of the most recent SSE event, for connection quality indicator.
    pub sse_last_event_at: Option<std::time::Instant>,
    /// Number of SSE reconnections since startup.
    pub sse_reconnect_count: u32,
    pub(crate) stream_rx: Option<mpsc::Receiver<StreamEvent>>,
    pub active_turn_id: Option<TurnId>,
    pub streaming_text: String,
    pub streaming_thinking: String,
    pub streaming_tool_calls: Vec<ToolCallInfo>,
    /// Current stream lifecycle phase. Drives status indicator rendering.
    pub stream_phase: crate::state::StreamPhase,
    /// Partial line buffer for line-by-line streaming.
    /// Complete lines (ending in `\n`) are flushed to `streaming_text`;
    /// the incomplete tail stays here until the next delta.
    pub(crate) streaming_line_buffer: String,
    /// Timestamp of the last stream event received during the current turn.
    /// Used for stall detection.
    pub(crate) stream_last_event_at: Option<std::time::Instant>,
    /// Set once the 30s stall warning has been shown for the current turn.
    pub(crate) stall_warned: bool,
    /// Non-dismissing status message shown during stall conditions.
    pub(crate) stall_message: Option<String>,
    /// Monotonic counter incremented on every stream lifecycle boundary
    /// (TurnStart, TurnComplete, TurnAbort, Error, CancelTurn).
    /// SSE handlers snapshot this before an async history reload and compare
    /// afterward: if it changed, a concurrent stream event mutated state and
    /// the reload result must be discarded to prevent stale UI.
    pub(crate) state_epoch: u64,
}
```

> Scroll position, virtual scroll index, and markdown render cache.
```rust
pub struct RenderState {
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub(crate) scroll_states: HashMap<NousId, SavedScrollState>,
    pub(crate) virtual_scroll: VirtualScroll,
    pub markdown_cache: MarkdownCache,
    /// Pre-rendered lines for finalized (committed) messages.
    /// PERF: Never re-rendered; only appended when a new message is committed.
    pub(crate) static_lines: Vec<ratatui::text::Line<'static>>,
    /// Number of committed messages covered by `static_lines`.
    pub(crate) static_message_count: usize,
    /// Width used to render `static_lines`. Cache invalidated on width change.
    pub(crate) static_width: usize,
}
```

> Terminal dimensions, tick counter, dirty flag, frame cache, and render state.
```rust
pub struct ViewportState {
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub tick_count: u64,
    pub error_toast: Option<ErrorToast>,
    pub success_toast: Option<ErrorToast>,
    /// Multi-type toast queue; each entry auto-dismisses after `duration_secs`.
    pub toasts: Vec<Toast>,
    /// Persistent top-of-viewport error banner, dismissed explicitly.
    pub error_banner: Option<ErrorBanner>,
    pub(crate) dirty: bool,
    pub(crate) frame_cache: Option<Buffer>,
    /// Timestamp of the last completed render. Used for 30fps throttling.
    pub(crate) last_render_at: Option<std::time::Instant>,
    pub render: RenderState,
}
```

> Input, tab completion, command palette, slash complete, selection, filter, and key state.
```rust
pub struct InteractionState {
    pub input: InputState,
    pub tab_completion: Option<TabCompletion>,
    pub command_palette: CommandPaletteState,
    pub slash_complete: SlashCompleteState,
    pub command_history: Vec<String>,
    pub command_history_index: Option<usize>,
    pub selection: SelectionContext,
    pub selected_message: Option<usize>,
    pub tool_expanded: HashSet<crate::id::ToolId>,
    pub filter: FilterState,
    pub(crate) keymap: KeyMap,
    /// Tool names that bypass the approval dialog for the lifetime of this TUI session.
    pub(crate) always_allowed_tools: HashSet<String>,
    /// Messages queued while the agent is streaming; auto-sent when the turn completes.
    pub queued_messages: Vec<crate::state::QueuedMessage>,
}
```

> Sidebar, overlay, view stack, ops, tabs, memory inspector, and notification log.
```rust
pub struct LayoutState {
    pub sidebar_visible: bool,
    pub thinking_expanded: bool,
    pub overlay: Option<Overlay>,
    pub view_stack: ViewStack,
    pub(crate) view_scroll_states: HashMap<usize, SavedScrollState>,
    pub ops: OpsState,
    pub(crate) tab_bar: TabBar,
    pub memory: MemoryInspectorState,
    pub(crate) metrics: MetricsState,
    pub(crate) editor: crate::state::editor::EditorState,
    pub(crate) pending_g: bool,
    pub(crate) bell_enabled: bool,
    /// Cross-agent notification log with read/unread tracking.
    pub notifications: NotificationStore,
}
```

```rust
pub struct App {
    pub config: Config,
    pub client: ApiClient,
    pub theme: Theme,
    pub highlighter: crate::highlight::Highlighter,
    pub should_quit: bool,

    pub dashboard: DashboardState,
    pub connection: ConnectionState,
    pub viewport: ViewportState,
    pub interaction: InteractionState,
    pub layout: LayoutState,

    /// Background fire-and-forget tasks (API calls, etc.) tracked so they can
    /// be awaited on shutdown instead of being silently dropped.
    pub(crate) background_tasks: JoinSet<()>,
}
```

```rust
impl App {
    pub async fn init (config: Config) -> Result<Self>;
    pub fn take_sse (&mut self) -> Option<SseConnection>;
    pub fn restore_sse (&mut self, sse: Option<SseConnection>);
    pub fn take_stream (&mut self) -> Option<mpsc::Receiver<StreamEvent>>;
    pub fn restore_stream (&mut self, rx: Option<mpsc::Receiver<StreamEvent>>);
    pub async fn update (&mut self, msg: Msg);
    pub fn view (&mut self, frame: &mut Frame) -> Vec<OscLink>;
}
```

## `koilon/src/command/mod.rs`

```rust
pub enum CommandCategory {
    Navigation,
    Action,
    Query,
    Agent,
}
```

```rust
pub struct Command {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub category: CommandCategory,
    pub shortcut: Option<&'static str>,
}
```

```rust
pub struct Suggestion {
    pub label: String,
    pub description: String,
    pub category: CommandCategory,
    pub aliases: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub score: i64,
    pub execute_as: String,
}
```

```rust
pub static COMMANDS: &[Command] = &[
    Command {
        name: "sessions",
        aliases: &["s"],
        description: "List sessions for current agent",
        category: CommandCategory::Navigation,
        shortcut: Some("Ctrl+S"),
    },
    Command {
        name: "agents",
        aliases: &["a"],
        description: "Switch agent",
        category: CommandCategory::Navigation,
        shortcut: Some("Ctrl+A"),
    },
    Command {
        name: "agent",
        aliases: &[],
        description: "Switch to named agent",
        category: CommandCategory::Agent,
        shortcut: None,
    },
    Command {
        name: "cost",
        aliases: &["$"],
        description: "Show daily cost breakdown",
        category: CommandCategory::Query,
        shortcut: Some("Ctrl+I"),
    },
    Command {
        name: "health",
        aliases: &["h"],
        description: "System health status",
        category: CommandCategory::Query,
        shortcut: Some("Ctrl+I"),
    },
    Command {
        name: "compact",
        aliases: &[],
        description: "Trigger distillation",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "clear",
        aliases: &[],
        description: "Clear conversation / new session",
        category: CommandCategory::Action,
        shortcut: Some("Ctrl+N"),
    },
    Command {
        name: "help",
        aliases: &["?"],
        description: "Show help",
        category: CommandCategory::Navigation,
        shortcut: Some("F1"),
    },
    Command {
        name: "quit",
        aliases: &["q"],
        description: "Quit application",
        category: CommandCategory::Action,
        shortcut: Some("Ctrl+C"),
    },
    Command {
        name: "recall",
        aliases: &["r"],
        description: "Search memory graph",
        category: CommandCategory::Query,
        shortcut: None,
    },
    Command {
        name: "memory",
        aliases: &["mem", "m"],
        description: "Open memory inspector",
        category: CommandCategory::Navigation,
        shortcut: Some("Ctrl+M"),
    },
    Command {
        name: "model",
        aliases: &[],
        description: "Show current model info",
        category: CommandCategory::Query,
        shortcut: None,
    },
    Command {
        name: "settings",
        aliases: &[],
        description: "Open settings",
        category: CommandCategory::Navigation,
        shortcut: None,
    },
    Command {
        name: "new",
        aliases: &[],
        description: "New conversation",
        category: CommandCategory::Action,
        shortcut: Some("Ctrl+N"),
    },
    Command {
        name: "rename",
        aliases: &[],
        description: "Rename current session",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "archive",
        aliases: &[],
        description: "Archive current session",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "unarchive",
        aliases: &[],
        description: "Restore archived session",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "diff",
        aliases: &["d"],
        description: "Show uncommitted changes",
        category: CommandCategory::Query,
        shortcut: None,
    },
    Command {
        name: "ops",
        aliases: &[],
        description: "Toggle operations pane",
        category: CommandCategory::Navigation,
        shortcut: Some("Ctrl+O"),
    },
    Command {
        name: "tab",
        aliases: &[],
        description: "Switch to tab by name",
        category: CommandCategory::Navigation,
        shortcut: None,
    },
    Command {
        name: "export",
        aliases: &[],
        description: "Export conversation to markdown",
        category: CommandCategory::Action,
        shortcut: None,
    },
    Command {
        name: "search",
        aliases: &[],
        description: "Search sessions and messages",
        category: CommandCategory::Query,
        shortcut: None,
    },
    Command {
        name: "notifications",
        aliases: &["notif"],
        description: "View notification history",
        category: CommandCategory::Navigation,
        shortcut: None,
    },
    Command {
        name: "metrics",
        aliases: &["stats"],
        description: "Open metrics dashboard",
        category: CommandCategory::Navigation,
        shortcut: None,
    },
    Command {
        name: "editor",
        aliases: &["edit", "e"],
        description: "Open file editor",
        category: CommandCategory::Navigation,
        shortcut: None,
    },
];
```

> Build suggestions from static commands + dynamic agent entries.
```rust
pub fn build_suggestions (input: &str, agents: &[AgentState]) -> Vec<Suggestion>
```

## `koilon/src/error.rs`

```rust
pub enum Error {
    /// API transport or authentication error from the HTTP client.
    #[snafu(context(false))]
    Api { source: skene::api::ApiError },

    /// Token is required but was not supplied.
    #[snafu(display("{message}"))]
    TokenRequired { message: String },

    /// Gateway is unreachable (health check returned false or connection refused).
    #[snafu(display(
        "cannot reach gateway at {url}\n  Server not running. Start it with: aletheia"
    ))]
    GatewayUnreachable { url: String },

    /// Could not determine the OS config directory (e.g. $HOME unset).
    #[snafu(display("could not determine config directory"))]
    ConfigDir,

    /// File-system I/O error.
    #[snafu(display("{context}: {source}"))]
    Io {
        context: &'static str,
        source: std::io::Error,
    },

    /// TOML serialization error.
    #[snafu(display("TOML error: {source}"))]
    Toml { source: toml::ser::Error },

    /// Invalid `tracing` filter directive.
    #[snafu(display("invalid log directive: {source}"))]
    LogDirective {
        source: tracing_subscriber::filter::ParseError,
    },

    /// An unexpected event type was received during SSE parsing.
    #[snafu(display("unexpected event type: {event_type}"))]
    UnexpectedEventType { event_type: String },

    /// Malformed or missing data in an incoming SSE event.
    #[snafu(display("malformed event data: {detail}"))]
    MalformedEventData { detail: String },

    /// SSE protocol state machine received an event out of sequence.
    #[snafu(display("protocol mismatch: {detail}"))]
    ProtocolMismatch { detail: String },

    /// The user aborted the setup wizard (pressed Esc or Ctrl+C).
    #[snafu(display("setup wizard aborted"))]
    WizardAborted,
}
```

> Convenience alias for `Result` with the TUI [`Error`] type.
```rust
pub type Result<T, E = Error> = std::result::Result<T, E>;
```

## `koilon/src/events.rs`

```rust
pub enum Event {
    /// Terminal keypress, mouse, resize
    Terminal(crossterm::event::Event),
    /// Global SSE event stream
    Sse(SseEvent),
    /// Per-session streaming response
    Stream(StreamEvent),
    /// 60fps UI tick
    Tick,
}
```

## `koilon/src/lib.rs`

> Run the interactive TUI setup wizard for first-run instance initialization.
> 
> Returns [`wizard::WizardAnswers`] when the user confirms on the final step.
> Returns [`error::Error::WizardAborted`] if the user presses Esc or Ctrl+C.
> 
> Call [`wizard::is_tty`] first to verify the terminal supports interactive input.
```rust
pub fn run_wizard (
    root: Option<std::path::PathBuf>,
    api_key: Option<String>,
) -> Result<wizard::WizardAnswers, error::Error>
```

```rust
pub async fn run_tui (
    url: Option<String>,
    token: Option<String>,
    agent: Option<String>,
    session: Option<String>,
    logout: bool,
) -> Result<(), error::Error>
```

## `koilon/src/msg.rs`

```rust
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
    MemoryDriftTabNext,
    MemoryDriftTabPrev,

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

    ShowError(String),
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "constructed by API event bridge, not yet wired")
    )]
    ShowSuccess(String),
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "constructed by API event bridge, not yet wired")
    )]
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

    #[expect(
        dead_code,
        reason = "metrics overlay entry point, keybinding not yet wired"
    )]
    MetricsOpen,
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

    #[expect(
        dead_code,
        reason = "planning view entry point, keybinding not yet wired"
    )]
    PlanningOpen,
    #[expect(dead_code, reason = "planning view close, wired in keybinding handler")]
    PlanningClose,

    #[expect(
        dead_code,
        reason = "retrospective view entry point, keybinding not yet wired"
    )]
    RetrospectiveOpen,
    #[expect(
        dead_code,
        reason = "retrospective view close, wired in keybinding handler"
    )]
    RetrospectiveClose,

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
```

```rust
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
```

```rust
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
```

```rust
pub enum NotificationKind {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "API bridge sends these; not yet wired")
    )]
    Info,
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
```

```rust
pub struct ErrorToast {
    pub message: String,
    pub created_at: std::time::Instant,
}
```

```rust
pub enum AuthOutcome {
    #[expect(dead_code, reason = "planned TUI feature")]
    Success { token: SecretString },
    #[expect(dead_code, reason = "planned TUI feature")]
    NoAuthRequired,
    #[expect(dead_code, reason = "planned TUI feature")]
    Failed(String),
}
```

## `koilon/src/state/agent.rs`

```rust
pub enum AgentStatus {
    Idle,
    Working,
    Streaming,
    Compacting,
}
```

```rust
pub struct ActiveTool {
    pub name: String,
    pub started_at: std::time::Instant,
}
```

```rust
pub struct ToolSummary {
    pub name: String,
    pub enabled: bool,
}
```

```rust
pub struct AgentState {
    pub id: NousId,
    pub name: String,
    /// Pre-lowercased `name`, cached at ingestion to avoid per-frame allocation in view code.
    pub name_lower: String,
    pub emoji: Option<String>,
    pub status: AgentStatus,
    pub active_tool: Option<ActiveTool>,
    pub sessions: Vec<Session>,
    pub model: Option<String>,
    pub compaction_stage: Option<String>,
    /// Set when distillation completes; cleared after 3-second auto-dismiss delay.
    pub distill_completed_at: Option<std::time::Instant>,
    /// Number of unread messages since the user last focused this agent.
    /// Cleared when the user switches to this agent.
    pub unread_count: u32,
    /// Available tools and their enablement state, fetched from the API.
    pub tools: Vec<ToolSummary>,
}
```

## `koilon/src/state/chat.rs`

```rust
pub enum StreamPhase {
    /// No active turn. Input is editable.
    #[default]
    Idle,
    /// HTTP request sent, waiting for first SSE event.
    Requesting,
    /// Receiving text deltas from the model.
    Streaming,
    /// Model is in an extended thinking block.
    Thinking,
    /// Context window compaction in progress.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "constructed when server emits compaction SSE events"
        )
    )]
    Compacting,
    /// Waiting for tool approval or external input.
    Waiting,
    /// Turn ended with an error.
    Error,
    /// Turn completed successfully. Transitions to Idle on next tick.
    Done,
}
```

```rust
pub enum MessageKind {
    /// Normal user or assistant message with full markdown rendering.
    #[default]
    Standard,
    /// Compact one-line tool status summary (not full JSON).
    #[expect(
        dead_code,
        reason = "SSE mapper variant, not yet constructed in TUI crate"
    )]
    ToolStatusLine,
    /// Compact thinking indicator line.
    #[expect(
        dead_code,
        reason = "SSE mapper variant, not yet constructed in TUI crate"
    )]
    ThinkingStatusLine,
    /// Distillation summary boundary marker.
    #[expect(
        dead_code,
        reason = "SSE mapper variant, not yet constructed in TUI crate"
    )]
    DistillationMarker,
    /// Visual separator between conversation topics.
    #[expect(
        dead_code,
        reason = "SSE mapper variant, not yet constructed in TUI crate"
    )]
    TopicBoundary,
}
```

```rust
pub struct ToolCallInfo {
    pub name: String,
    pub tool_id: Option<ToolId>,
    pub duration_ms: Option<u64>,
    pub is_error: bool,
    /// Tool result text, stored for collapsible card rendering.
    pub output: Option<String>,
}
```

```rust
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    /// Pre-lowercased `text`, cached at ingestion to avoid per-frame allocation in view code.
    pub text_lower: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
    /// Semantic category for rendering differentiation.
    pub kind: MessageKind,
}
```

## `koilon/src/state/command.rs`

```rust
pub struct SlashCompleteState {
    pub(crate) active: bool,
    pub(crate) query: String,
    pub(crate) suggestions: Vec<SlashSuggestion>,
    pub(crate) cursor: usize,
}
```

```rust
pub struct SlashSuggestion {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) execute_as: String,
}
```

```rust
pub struct CommandPaletteState {
    pub(crate) input: String,
    pub(crate) cursor: usize,
    pub(crate) suggestions: Vec<crate::command::Suggestion>,
    pub(crate) selected: usize,
    pub(crate) active: bool,
}
```

```rust
pub enum SelectionContext {
    // kanon:ignore RUST/pub-visibility
    #[default]
    None,
    UserMessage {
        index: usize,
    },
    AgentResponse {
        index: usize,
        has_code: bool,
        has_links: bool,
    },
    ToolCall {
        index: usize,
        tool_id: crate::id::ToolId,
        needs_approval: bool,
    },
    SessionListItem {
        index: usize,
    },
}
```

## `koilon/src/state/filter.rs`

```rust
pub enum FilterScope {
    // kanon:ignore RUST/pub-visibility
    #[default]
    Chat,
    #[expect(
        dead_code,
        reason = "variant required for #[non_exhaustive] completeness; sidebar filtering"
    )]
    Agents,
}
```

```rust
pub struct FilterState {
    /// Whether filter mode is active (editing or applied)
    pub(crate) active: bool,
    /// Whether the user is currently typing in the filter bar
    pub(crate) editing: bool,
    /// Current filter text
    pub(crate) text: String,
    /// Pre-lowercased filter text (cached to avoid per-frame allocation)
    text_lower: String,
    /// Cursor position in filter text (byte offset)
    pub(crate) cursor: usize,
    /// Which view the filter applies to
    pub(crate) scope: FilterScope,
    /// Number of matches in current view
    pub(crate) match_count: usize,
    /// Total items before filtering
    pub(crate) total_count: usize,
    /// Index of the currently highlighted match (for n/N navigation)
    pub(crate) current_match: usize,
}
```

## `koilon/src/state/input.rs`

```rust
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub kill_ring: KillRing,
    pub history_search: Option<HistorySearchState>,
    pub image_attachments: Vec<ImageAttachment>,
    /// When the cursor should stop flashing (after input activity).
    pub cursor_flash_until: Option<std::time::Instant>,
}
```

```rust
pub struct TabCompletion {
    pub prefix: String,
    pub candidates: Vec<String>,
    pub index: usize,
    pub insert_start: usize,
}
```

```rust
pub struct KillRing {
    pub(crate) entries: Vec<String>,
    /// Tracks the byte span of the last yank for Alt+Y replacement.
    pub(crate) last_yank: Option<YankSpan>,
}
```

```rust
pub struct YankSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) ring_index: usize,
}
```

```rust
pub struct HistorySearchState {
    pub query: String,
    pub match_index: Option<usize>,
}
```

```rust
pub struct ImageAttachment {
    pub data: Vec<u8>,
    #[expect(
        dead_code,
        reason = "stored for API payload construction when sending image attachments"
    )]
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
}
```

```rust
pub struct QueuedMessage {
    pub text: String,
}
```

## `koilon/src/state/memory/mod.rs`

```rust
pub struct MemoryInspectorState {
    /// Current active tab.
    pub(crate) tab: MemoryTab,
    /// Whether data is being loaded.
    pub(crate) loading: bool,
    /// Fact list state: facts, selection, sorting, detail.
    pub(crate) fact_list: FactListState,
    /// Filter state: text filter, type/tier filters.
    pub(crate) filters: MemoryFilterState,
    /// Search state: query, results, active flag.
    pub(crate) search: MemorySearchState,
    /// Graph state: entities, relationships, timeline events.
    pub(crate) graph: MemoryGraphState,
}
```

## `koilon/src/state/metrics.rs`

```rust
pub struct TurnTokens {
    pub(crate) input: u32,
    pub(crate) output: u32,
    pub(crate) cache_read: u32,
}
```

```rust
pub struct AgentMetrics {
    /// Number of completed turns.
    pub(crate) turns: u32,
    /// Total input tokens.
    pub(crate) input_tokens: u64,
    /// Total output tokens.
    pub(crate) output_tokens: u64,
    /// Total cache-read tokens.
    pub(crate) cache_read_tokens: u64,
}
```

```rust
pub struct MetricsState {
    /// When the TUI app started, for uptime calculation.
    pub(crate) started_at: Instant,
    /// Cumulative input tokens across all turns since startup.
    pub(crate) total_input_tokens: u64,
    /// Cumulative output tokens across all turns since startup.
    pub(crate) total_output_tokens: u64,
    /// Cumulative cache-read tokens across all turns since startup.
    pub(crate) total_cache_read_tokens: u64,
    /// Cumulative cache-write tokens across all turns since startup.
    pub(crate) total_cache_write_tokens: u64,
    /// Per-agent statistics keyed by agent ID.
    pub(crate) agent_stats: HashMap<NousId, AgentMetrics>,
    /// Recent turn token totals for the sparkline, capped at SPARKLINE_CAPACITY.
    pub(crate) turn_history: Vec<TurnTokens>,
    /// Whether the last health check returned OK.
    pub(crate) api_healthy: Option<bool>,
    /// Scroll offset in the per-agent table.
    pub(crate) scroll_offset: usize,
    /// Selected agent row index in the per-agent table.
    pub(crate) selected_agent: usize,
}
```

## `koilon/src/state/notification.rs`

```rust
pub struct Toast {
    pub message: String,
    pub kind: NotificationKind,
    pub duration_secs: u64,
    pub created_at: Instant,
}
```

```rust
pub struct ErrorBanner {
    pub message: String,
}
```

```rust
pub struct Notification {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used for deduplication and future API serialization"
        )
    )]
    pub id: u64,
    pub nous_id: Option<NousId>,
    pub message: String,
    pub kind: NotificationKind,
    pub read: bool,
    #[expect(dead_code, reason = "used for future timestamp rendering")]
    pub created_at: Instant,
}
```

```rust
pub struct NotificationStore {
    pub items: Vec<Notification>,
    next_id: u64,
}
```

## `koilon/src/state/ops/state_impl.rs`

```rust
pub struct OpsState {
    /// Whether the pane is currently visible
    pub(crate) visible: bool,
    /// Width as percentage of terminal (0-100), default 40
    pub(crate) width_pct: u16,
    /// Which pane has keyboard focus
    pub(crate) focused_pane: FocusedPane,
    /// Auto-show behavior
    pub(crate) auto_show: OpsAutoShow,
    /// Scroll offset within the ops pane
    pub(crate) scroll_offset: usize,
    /// Currently selected item index (for j/k navigation)
    pub(crate) selected_item: Option<usize>,

    /// Accumulated thinking text during current turn
    pub(crate) thinking: OpsThinkingBlock,
    /// Tool calls during current turn
    pub(crate) tool_calls: Vec<OpsToolCall>,
    /// File diffs parsed from tool results
    pub(crate) diffs: Vec<OpsDiffEntry>,
    /// Aggregated KPI summary for the current turn.
    pub(crate) summary: OpsSummary,
    /// Wall-clock start time for the current turn (elapsed display).
    pub(crate) turn_started_at: Option<std::time::Instant>,
    /// When true, show all tool calls including successful ones. Default: false (show only errors).
    pub(crate) show_all_successful: bool,
}
```

## `koilon/src/state/ops/types.rs`

```rust
pub enum FocusedPane {
    #[default]
    Chat,
    Operations,
}
```

```rust
pub enum OpsToolStatus {
    Running,
    Complete,
    Failed,
}
```

```rust
pub enum OpsAutoShow {
    /// Show automatically when streaming starts, collapse when idle
    #[default]
    Auto,
}
```

## `koilon/src/state/overlay.rs`

```rust
pub enum Overlay {
    Help,
    AgentPicker {
        cursor: usize,
    },
    SessionPicker(SessionPickerOverlay),
    SystemStatus,
    ContextBudget,
    Settings(SettingsOverlay),
    ToolApproval(ToolApprovalOverlay),
    PlanApproval(PlanApprovalOverlay),
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "context action overlay, constructed in tests only"
        )
    )]
    ContextActions(ContextActionsOverlay),
    DiffView(crate::diff::DiffViewState),
    SessionSearch(SessionSearchOverlay),
    DecisionCard(DecisionCardOverlay),
    NotificationHistory {
        scroll: usize,
    },
}
```

```rust
pub struct SessionSearchOverlay {
    pub query: String,
    pub cursor: usize,
    pub results: Vec<SearchResult>,
    pub selected: usize,
}
```

```rust
pub struct SearchResult {
    pub agent_id: NousId,
    pub agent_name: String,
    pub session_id: SessionId,
    pub session_label: String,
    pub snippet: String,
    pub kind: SearchResultKind,
}
```

```rust
pub enum SearchResultKind {
    SessionName,
    MessageContent { role: String },
}
```

```rust
pub struct ContextActionsOverlay {
    pub actions: Vec<ContextAction>,
    pub cursor: usize,
}
```

```rust
pub struct ContextAction {
    pub label: &'static str,
    pub kind: MessageActionKind,
}
```

```rust
pub struct SessionPickerOverlay {
    pub cursor: usize,
    pub show_archived: bool,
}
```

```rust
pub struct ToolApprovalOverlay {
    pub turn_id: TurnId,
    pub tool_id: ToolId,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub risk: String,
    pub reason: String,
}
```

```rust
pub struct PlanApprovalOverlay {
    pub plan_id: PlanId,
    pub steps: Vec<PlanStepApproval>,
    pub total_cost_cents: u32,
    pub cursor: usize,
}
```

```rust
pub struct PlanStepApproval {
    pub id: u32,
    pub label: String,
    pub role: String,
    pub checked: bool,
}
```

```rust
pub enum DecisionField {
    #[default]
    Options,
    CustomAnswer,
    Notes,
}
```

```rust
pub struct DecisionOption {
    pub label: String,
    pub description: Option<String>,
    pub is_recommendation: bool,
}
```

```rust
pub struct DecisionCardOverlay {
    pub question: String,
    pub options: Vec<DecisionOption>,
    pub cursor: usize,
    pub custom_answer: String,
    pub custom_cursor: usize,
    pub notes: String,
    pub notes_cursor: usize,
    pub focused_field: DecisionField,
}
```

```rust
pub struct SubmittedDecision {
    pub question: String,
    pub chosen_label: String,
    pub notes: String,
    #[expect(
        dead_code,
        reason = "planned TUI feature: used for future expiry/age display"
    )]
    pub submitted_at: std::time::Instant,
}
```

## `koilon/src/state/view_stack.rs`

```rust
pub enum View {
    // kanon:ignore RUST/pub-visibility
    /// Top-level: agent sidebar + active conversation.
    Home,
    /// Session list for a specific agent.
    Sessions { agent_id: NousId },
    /// Single conversation view.
    Conversation {
        agent_id: NousId,
        session_id: SessionId,
    },
    /// Full message detail (content, tool results, metadata).
    MessageDetail { message_index: usize },
    /// Memory inspector: browsing the knowledge graph.
    MemoryInspector,
    /// Fact detail within the memory inspector.
    FactDetail { fact_id: String },
    /// Entity detail within the graph view (node card).
    EntityDetail { entity_id: String },
    /// Metrics dashboard: token usage, cost, service health, per-agent stats.
    Metrics,
    /// Built-in file editor with syntax highlighting and tabs.
    FileEditor,
    /// Planning dashboard: active phases, progress, and pending checkpoint approvals.
    Planning,
    /// Retrospective view: completed project phases with outcomes and key metrics.
    Retrospective,
}
```

```rust
pub struct ViewStack {
    // kanon:ignore RUST/pub-visibility
    stack: Vec<View>,
}
```

## `koilon/src/theme.rs`

```rust
pub static THEME: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(Theme::default);
```

## `koilon/src/wizard/mod.rs`

> Returns `true` when the current environment supports a TUI wizard.
> 
> Requires both stdin and stdout to be connected to a TTY.
```rust
pub fn is_tty () -> bool
```

## `koilon/src/wizard/state.rs`

```rust
pub struct WizardAnswers {
    // kanon:ignore RUST/no-debug-derive-on-public-types — manual Debug impl redacts api_key below
    /// Instance root directory.
    pub root: PathBuf,
    /// API provider (`"anthropic"` or `"openai"`).
    pub api_provider: String,
    /// Raw API key string pasted by the user; `None` means use the environment.
    pub api_key: Option<SecretString>,
    /// Credential resolution source (`"api-key"` or `"auto"`).
    pub credential_source: String, // kanon:ignore RUST/plain-string-secret
    /// Gateway bind target (`"localhost"` or `"lan"`).
    pub bind: String,
    /// Gateway auth mode (`"none"` or `"token"`).
    pub auth_mode: String,
    /// IANA timezone identifier (e.g., `"America/New_York"`).
    pub timezone: String,
    /// Operator display name for `USER.md`.
    pub user_name: String,
    /// Operator role description for `USER.md`.
    pub user_role: String,
    /// Agent identifier (alphanumeric + hyphens/underscores).
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde/external-id field from user input; newtype out of scope
    pub agent_id: String,
    /// Agent display name.
    pub agent_name: String,
    /// Primary model identifier.
    pub model: String,
}
```

## `proskenion/src/api/sse.rs`

```rust
impl SseConnection {
    pub async fn next (&mut self) -> Option<SseEvent>;
}
```

## `proskenion/src/components/chat/mod.rs`

```rust
pub struct ChatMessage {
    /// Who sent this message.
    pub role: MessageRole,
    /// The message text content.
    pub content: String,
    /// Model that generated this message, if from an assistant.
    pub model: Option<String>,
    /// Number of tool calls made during this turn.
    pub tool_calls: u32,
    /// Input tokens consumed by this turn.
    pub input_tokens: u32,
    /// Output tokens produced by this turn.
    pub output_tokens: u32,
    /// Thinking/reasoning content captured during the turn.
    pub thinking: Option<String>,
    /// Rich tool call details for panel display in committed messages.
    pub tool_call_details: Vec<ToolCallState>,
    /// Planning cards associated with this message.
    pub plans: Vec<PlanCardState>,
}
```

```rust
pub enum MessageRole {
    /// Message sent by the user.
    User,
    /// Message generated by the assistant.
    Assistant,
}
```

```rust
pub struct ChatState {
    /// Committed conversation history.
    pub messages: Vec<ChatMessage>,
    /// In-flight streaming state for the active turn.
    pub streaming: StreamingState,
    /// Global SSE connection state.
    pub connection: ConnectionState,
    /// Agent currently being chatted with.
    pub agent_id: Option<NousId>,
    /// Active session key.
    pub session_key: Option<String>,
}
```

## `proskenion/src/lib.rs`

> Launch the desktop application.
> 
> Initialises log-to-file, loads persisted window state, and configures the
> desktop window before showing it. Closing the window exits the process
> cleanly  -  no minimize-to-tray, no hidden background process.
> 
> Pass `verbose = true` (e.g. from a `--verbose` CLI flag) to also emit logs
> to stderr. When `RUST_LOG` is set in the environment stderr output is added
> automatically regardless.
```rust
pub fn run (verbose: bool)
```

## `proskenion/src/services/connection.rs`

> Errors from connection attempts to a pylon server.
```rust
pub enum ConnectionError {
    /// Health check request failed.
    #[snafu(display("health check failed: {source}"))]
    HealthCheck {
        /// Underlying HTTP error.
        source: reqwest::Error,
    },

    /// Server responded but reported unhealthy status.
    #[snafu(display("server returned unhealthy status: {status}"))]
    Unhealthy {
        /// HTTP status code returned.
        status: u16,
    },

    /// Connection attempt exceeded the configured timeout.
    #[snafu(display("connection timed out after {timeout_secs}s"))]
    Timeout {
        /// Configured timeout in seconds.
        timeout_secs: u64,
    },

    /// Auth token contains non-ASCII characters.
    #[snafu(display("invalid auth token: contains non-ASCII characters"))]
    InvalidToken,

    /// Failed to construct the reqwest client.
    #[snafu(display("failed to build HTTP client: {source}"))]
    ClientBuild {
        /// Underlying HTTP error.
        source: reqwest::Error,
    },
}
```

```rust
pub struct PylonClient {
    client: reqwest::Client,
    base_url: String,
}
```

```rust
impl PylonClient {
    pub async fn health (&self) -> Result<(), ConnectionError>;
}
```

```rust
impl ConnectionService {
    pub async fn run (self);
}
```

## `proskenion/src/state/agents.rs`

```rust
pub enum AgentStatus {
    /// Agent is available and not processing a turn.
    #[default]
    Idle,
    /// Agent is currently processing a turn.
    Active,
    /// Agent is in an error state.
    Error,
}
```

```rust
pub struct AgentRecord {
    /// Core agent data from the API.
    pub agent: Agent,
    /// Current runtime status.
    pub status: AgentStatus,
}
```

```rust
pub struct AgentStore {
    /// All known agents, keyed by NousId.
    agents: HashMap<NousId, AgentRecord>,
    /// Ordered list of agent IDs (preserves server order).
    order: Vec<NousId>,
    /// Currently active agent ID.
    pub active_id: Option<NousId>,
}
```

## `proskenion/src/state/app.rs`

```rust
pub struct AgentEntry {
    /// Agent identifier.
    pub id: NousId,
    /// Human-readable agent name.
    pub name: String,
    /// Agent status label (idle, busy, etc.).
    pub status: String,
}
```

```rust
pub struct AppState {
    /// All registered agents, fetched on startup and updated via SSE.
    pub agents: Vec<AgentEntry>,
    /// Currently focused agent. `None` before first agent load.
    pub focused_agent: Option<NousId>,
    /// Tab bar managing multiple open conversations.
    pub tabs: TabBar,
    /// Server connection lifecycle state.
    pub connection: ConnectionState,
    /// User-configured connection parameters.
    pub connection_config: ConnectionConfig,
    /// Modal overlay (help, pickers, approvals). `None` when no overlay.
    pub overlay: Option<Overlay>,
    /// Sidebar visibility toggle.
    pub sidebar_visible: bool,
}
```

```rust
pub struct TabEntry {
    /// Unique tab identifier.
    pub id: TabId,
    /// Agent associated with this tab.
    pub agent_id: NousId,
    /// Server session key associated with the tab, if known.
    pub session_key: Option<String>, // kanon:ignore RUST/plain-string-secret
    /// Display title for the tab.
    pub title: String,
    /// Whether this tab has unread messages.
    pub unread: bool,
}
```

```rust
pub struct TabBar {
    /// Ordered list of open tabs.
    pub tabs: Vec<TabEntry>,
    /// Index of the currently active tab.
    pub active: usize,
    next_id: TabId,
}
```

```rust
pub enum Overlay {
    /// Keyboard shortcut reference overlay.
    Help,
    /// Agent selection picker.
    AgentPicker {
        /// Currently highlighted row in the picker list.
        cursor: usize,
    },
    /// Session history picker.
    SessionPicker {
        /// Currently highlighted row in the picker list.
        cursor: usize,
        /// Whether archived sessions are visible.
        show_archived: bool,
    },
    /// Tool execution approval dialog.
    ToolApproval(ToolApprovalOverlay),
    /// Application settings panel.
    Settings,
}
```

```rust
pub struct ToolApprovalOverlay {
    /// Name of the tool requesting approval.
    pub tool_name: String,
    /// Serialized JSON input for the tool call.
    pub input_json: String,
    /// Risk level label (e.g. "high", "medium").
    pub risk: String,
    /// Human-readable explanation of why approval is needed.
    pub reason: String,
}
```

## `proskenion/src/state/chat.rs`

```rust
pub enum Role {
    /// Message sent by the user.
    User,
    /// Response generated by the assistant.
    Assistant,
    /// System-level message (context, errors).
    System,
}
```

```rust
pub struct ChatMessage {
    /// Unique message identifier (index-based or server-provided).
    pub id: u64,
    /// Who sent this message.
    pub role: Role,
    /// Raw markdown content.
    pub content: String,
    /// Unix timestamp in seconds.
    pub timestamp: i64,
    /// Agent that produced this message (assistant messages only).
    pub agent_id: Option<NousId>,
    /// Number of tool calls made during this turn.
    pub tool_calls: u32,
    /// Extended thinking content, if any.
    pub thinking_content: Option<String>,
    /// Whether this message is still being streamed.
    pub is_streaming: bool,
    /// Model that generated this message.
    pub model: Option<String>,
    /// Input tokens consumed.
    pub input_tokens: u32,
    /// Output tokens produced.
    pub output_tokens: u32,
}
```

## `proskenion/src/state/commands.rs`

```rust
pub enum CommandSource {
    /// Built into the desktop client.
    Client,
    /// Provided by the server for a specific agent.
    Server,
}
```

```rust
pub enum CommandCategory {
    /// Performs an action (export, clear, etc.).
    Action,
    /// Navigates to a different view.
    Navigation,
}
```

```rust
pub struct Command {
    /// Command name without the leading `/`.
    pub name: String,
    /// Short description shown in the palette.
    pub description: String,
    /// Usage hint shown on selection, e.g. `/help [topic]`.
    pub usage: String,
    /// Where this command comes from.
    pub source: CommandSource,
    /// Whether this command is specific to the active agent.
    pub agent_specific: bool,
    /// Functional category for dispatch.
    pub category: CommandCategory,
}
```

```rust
pub struct CommandStore {
    /// Filtered subset currently shown in the palette.
    pub filtered: Vec<Command>,
    /// Highlighted row index into `filtered`.
    pub cursor: usize,
}
```

## `proskenion/src/state/connection.rs`

```rust
pub enum ConnectionState {
    /// No connection attempted yet, or explicitly disconnected by the user.
    #[default]
    Disconnected,
    /// Initial connection in progress (first attempt).
    Connecting,
    /// Connected and healthy: health checks passing.
    Connected,
    /// Lost connection, attempting to restore. `attempt` counts consecutive
    /// failures (1-indexed).
    Reconnecting {
        /// Consecutive reconnection failures (1-indexed).
        attempt: u32,
    },
    /// Connection attempt exceeded the configured timeout.
    TimedOut,
    /// Permanently failed: requires user intervention (e.g. bad URL, auth
    /// rejected, max retries exceeded).
    Failed {
        /// Human-readable failure description.
        reason: String,
    },
}
```

```rust
pub struct ConnectionConfig {
    /// Base URL of the pylon server (e.g. `http://localhost:18789`).
    pub server_url: String,
    /// Optional authentication token. Injected as `Authorization: Bearer <token>`.
    ///
    /// SECURITY: Stored in plaintext in the config file (`~/.config/aletheia/desktop.toml`).
    /// The file is written with 0600 permissions (owner-only), but any process running
    /// as the same user can read it. Full OS keyring integration (libsecret on Linux,
    /// Keychain on macOS) is tracked as future work. Do not copy this file or commit
    /// it to version control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    /// Whether to automatically reconnect on connection loss.
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,
    /// Maximum time in seconds to wait for a connection attempt before timing out.
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
}
```

## `proskenion/src/state/events.rs`

```rust
pub struct EventState {
    /// Currently active agent turns. Populated on `Init`, updated by
    /// `TurnBefore` (add) and `TurnAfter` (remove).
    pub active_turns: Vec<ActiveTurn>,

    /// Per-agent status string from `StatusUpdate` events.
    /// The string value maps to an agent status label at the component layer.
    pub agent_statuses: HashMap<NousId, String>,

    /// Per-agent distillation progress from `Distill*` events.
    pub distillation: HashMap<NousId, DistillationProgress>,

    /// Per-project checkpoint revision counter. Incremented by
    /// `CheckpointCreated` and `CheckpointUpdated` SSE events.
    /// Views compare their last-seen revision to detect when a
    /// re-fetch is needed.
    pub checkpoint_revisions: HashMap<String, u64>,

    /// SSE connection lifecycle state.
    pub connection: SseConnectionState,
}
```

```rust
pub enum SseConnectionState {
    /// Not connected to the SSE stream.
    Disconnected,
    /// Actively receiving events.
    Connected,
    /// Lost connection, attempting to reconnect.
    Reconnecting {
        /// Consecutive reconnection failures (1-indexed).
        attempt: u32,
    },
}
```

```rust
pub enum ConnectionState {
    /// Initial state, not yet attempted.
    Disconnected,
    /// Actively receiving events.
    Connected,
    /// Reconnecting after failure. `attempt` counts consecutive failures.
    Reconnecting {
        /// Consecutive reconnection failures (1-indexed).
        attempt: u32,
    },
}
```

```rust
pub struct StreamingState {
    /// Accumulated response text (not yet committed to history).
    pub text: String,
    /// Accumulated extended thinking output.
    pub thinking: String,
    /// Active tool calls in progress (minimal tracking).
    pub tool_calls: Vec<ToolCallInfo>,
    /// Rich tool call state for panel display (input/output/error details).
    pub tool_call_details: Vec<ToolCallState>,
    /// Tool calls awaiting user approval.
    pub approvals: Vec<ToolApprovalState>,
    /// Active planning cards for this turn.
    pub plans: Vec<PlanCardState>,
    /// Whether the stream is actively receiving deltas.
    pub is_streaming: bool,
    /// Turn ID if a turn is in progress.
    pub turn_id: Option<TurnId>,
    /// Error message if the stream errored.
    pub error: Option<String>,
}
```

```rust
pub struct ToolCallInfo {
    /// Name of the tool being invoked.
    pub tool_name: String,
    /// Unique identifier for this tool call.
    pub tool_id: ToolId,
    /// Whether the tool returned an error.
    pub is_error: bool,
    /// Wall-clock duration of the tool call in milliseconds.
    pub duration_ms: Option<u64>,
    /// Whether the tool call has completed.
    pub completed: bool,
    /// Tool input parameters as JSON (for panel display).
    pub input: Option<serde_json::Value>,
    /// Tool output text (for panel display).
    pub output: Option<String>,
    /// Error detail message distinct from `is_error` flag.
    pub error_message: Option<String>,
}
```

```rust
pub enum DistillationProgress {
    /// Distillation is in progress but no stage reported yet.
    Started,
    /// Currently executing a named stage.
    Stage {
        /// Name of the distillation stage in progress.
        stage: String,
    },
    /// Distillation completed.
    Complete,
}
```

## `proskenion/src/state/input.rs`

```rust
pub struct InputState {
    /// Current text content of the textarea.
    pub text: String,
    /// Ring buffer of previously submitted messages, newest at back.
    history: VecDeque<String>,
    /// Index into history during navigation. `None` means the user is
    /// editing fresh input (not browsing history).
    history_index: Option<usize>,
    /// Stashed draft text saved when the user starts navigating history,
    /// restored when they return past the newest entry.
    draft: String,
}
```

```rust
pub enum SubmissionState {
    /// Ready for input.
    Idle,
    /// Message has been submitted and is in flight.
    Submitting,
    /// Submission failed with an error message.
    Error(String),
}
```

## `proskenion/src/state/ops.rs`

```rust
pub enum HealthTier {
    /// Agent is operating normally.
    #[default]
    Healthy,
    /// Agent has warnings or partial failures.
    Degraded,
    /// Agent is in an error state or unreachable.
    Error,
}
```

```rust
pub enum JobResult {
    #[default]
    Unknown,
    Success,
    Failure,
}
```

```rust
pub enum TaskStatus {
    #[default]
    Running,
    Stopped,
    Failed,
}
```

```rust
pub enum Trend {
    Up,
    Down,
    #[default]
    Stable,
}
```

## `proskenion/src/state/platform.rs`

```rust
pub enum TrayIconStatus {
    /// All agents healthy, none processing.
    #[default]
    Normal,
    /// At least one agent is actively processing a turn.
    Active,
    /// At least one agent is in an error state.
    Error,
    /// Disconnected from the pylon server.
    Disconnected,
}
```

```rust
pub struct TrayState {
    /// Aggregate icon status derived from all agent states.
    pub icon_status: TrayIconStatus,
    /// Total agent count for tooltip.
    pub agent_count: usize,
    /// Number of agents currently processing a turn.
    pub processing_count: usize,
    /// Whether the main window is currently visible.
    pub window_visible: bool,
}
```

```rust
pub enum HotkeyRegistration {
    /// Hotkey registered and active.
    Registered,
    /// Registration failed (key combination taken by another app, or platform limitation).
    Failed {
        /// Human-readable failure reason.
        reason: String,
    },
    /// Platform does not support global hotkeys (e.g. Wayland without portal).
    Unavailable,
}
```

```rust
pub enum HotkeyAction {
    /// Toggle window visibility (summon/dismiss).
    SummonWindow,
    /// Open the quick input overlay.
    QuickInput,
    /// Abort all active streaming responses.
    AbortStreaming,
}
```

```rust
pub struct HotkeyState {
    /// Registration status for each hotkey action.
    pub registrations: Vec<(HotkeyAction, HotkeyRegistration)>,
}
```

```rust
pub struct WindowState {
    /// Window X position in screen coordinates.
    #[serde(default = "default_x")]
    pub x: i32,
    /// Window Y position in screen coordinates.
    #[serde(default = "default_y")]
    pub y: i32,
    /// Window width in logical pixels.
    #[serde(default = "default_width")]
    pub width: u32,
    /// Window height in logical pixels.
    #[serde(default = "default_height")]
    pub height: u32,
    /// Whether the window was maximized.
    #[serde(default)]
    pub maximized: bool,
    /// Active view route path (e.g. "/", "/files", "/planning").
    #[serde(default = "default_active_view")]
    pub active_view: String,
    /// Whether the sidebar is collapsed.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// Sidebar width override in pixels. `None` uses the default 220px.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<u32>,
    /// Last active session ID per agent (keyed by agent ID string).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub active_sessions: HashMap<String, String>,
}
```

```rust
pub struct QuickInputState {
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Currently selected agent for the input.
    pub selected_agent: Option<NousId>,
    /// Current text in the input field.
    pub input_text: String,
}
```

```rust
pub enum CloseBehavior {
    /// Minimize to system tray instead of quitting.
    MinimizeToTray,
    /// Quit the application cleanly (disconnect SSE, persist state, exit).
    #[default]
    Quit,
}
```

## `proskenion/src/state/toasts.rs`

```rust
pub struct ToastStore {
    toasts: Vec<Toast>,
    next_id: u64,
}
```

## `proskenion/src/state/tools.rs`

```rust
pub enum ToolStatus {
    /// Tool call received but not yet executing.
    Pending,
    /// Tool is currently executing.
    Running,
    /// Tool completed successfully.
    Success,
    /// Tool completed with an error.
    Error,
}
```

```rust
pub struct ToolCallState {
    /// Unique identifier for this tool call.
    pub tool_id: ToolId,
    /// Name of the tool.
    pub tool_name: String,
    /// Current execution status.
    pub status: ToolStatus,
    /// Tool input parameters as JSON.
    pub input: Option<serde_json::Value>,
    /// Tool output text on success.
    pub output: Option<String>,
    /// Error message if the tool failed.
    pub error_message: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: Option<u64>,
}
```

```rust
pub enum RiskLevel {
    /// Low risk: safe, read-only operations.
    Low,
    /// Medium risk: writes to local state.
    Medium,
    /// High risk: destructive or external-facing operations.
    High,
    /// Critical risk: irreversible or security-sensitive operations.
    Critical,
}
```

```rust
pub struct ToolApprovalState {
    /// Turn that owns this tool call.
    pub turn_id: TurnId,
    /// Unique identifier for this tool call.
    pub tool_id: ToolId,
    /// Name of the tool requesting approval.
    pub tool_name: String,
    /// Tool input parameters.
    pub input: serde_json::Value,
    /// Risk level assigned by the server.
    pub risk: RiskLevel,
    /// Human-readable reason for requiring approval.
    pub reason: String,
    /// Whether the approval has been resolved (approved or denied).
    pub resolved: bool,
}
```

```rust
pub enum StepStatus {
    /// Step not yet started.
    Pending,
    /// Step currently executing.
    InProgress,
    /// Step completed successfully.
    Complete,
    /// Step failed.
    Failed,
}
```

```rust
pub struct PlanStepState {
    /// Step index within the plan.
    pub id: u32,
    /// Human-readable label.
    pub label: String,
    /// Current step status.
    pub status: StepStatus,
    /// Result summary after completion.
    pub result: Option<String>,
}
```

```rust
pub enum PlanStatus {
    /// Plan has been proposed but not started.
    Proposed,
    /// Plan is actively executing steps.
    InProgress,
    /// Plan has completed (carries final status string from server).
    Complete {
        /// Server-provided completion status label.
        status: String,
    },
}
```

```rust
pub struct PlanCardState {
    /// Plan identifier.
    pub plan_id: PlanId,
    /// Ordered list of plan steps.
    pub steps: Vec<PlanStepState>,
    /// Overall plan status.
    pub status: PlanStatus,
}
```

## `skene/src/api/client.rs`

```rust
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: Option<SecretString>,
}
```

```rust
impl ApiClient {
    pub fn new (base_url: &str, token: Option<String>) -> Result<Self>;
    pub fn token (&self) -> Option<&str>;
    pub async fn health (&self) -> Result<bool>;
    pub async fn auth_mode (&self) -> Result<AuthMode>;
    pub async fn login (&self, username: &str, password: &str) -> Result<LoginResponse>;
    pub async fn agents (&self) -> Result<Vec<Agent>>;
    pub async fn sessions (&self, nous_id: &str) -> Result<Vec<Session>>;
    pub async fn history (&self, session_id: &str) -> Result<Vec<HistoryMessage>>;
    pub async fn create_session (&self, nous_id: &str, session_key: &str) -> Result<Session>;
    pub async fn archive_session (&self, session_id: &str) -> Result<()>;
    pub async fn unarchive_session (&self, session_id: &str) -> Result<()>;
    pub async fn rename_session (&self, session_id: &str, name: &str) -> Result<()>;
    pub async fn abort_turn (&self, turn_id: &str) -> Result<()>;
    pub async fn approve_tool (&self, turn_id: &str, tool_id: &str) -> Result<()>;
    pub async fn deny_tool (&self, turn_id: &str, tool_id: &str) -> Result<()>;
    pub async fn approve_plan (&self, plan_id: &str) -> Result<()>;
    pub async fn cancel_plan (&self, plan_id: &str) -> Result<()>;
    pub async fn today_cost_cents (&self) -> Result<u32>;
    pub async fn compact (&self, session_id: &str) -> Result<()>;
    pub async fn tools (&self, nous_id: &str) -> Result<Vec<NousTool>>;
    pub async fn recall (&self, nous_id: &str, query: &str) -> Result<String>;
    pub async fn config (&self) -> Result<serde_json::Value>;
    pub async fn update_config_section (
        &self,
        section: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value>;
    pub async fn knowledge_facts (
        &self,
        sort: &str,
        order: &str,
        limit: u32,
    ) -> Result<serde_json::Value>;
    pub async fn knowledge_fact_detail (&self, fact_id: &str) -> Result<serde_json::Value>;
    pub async fn knowledge_forget (&self, fact_id: &str) -> Result<()>;
    pub async fn knowledge_restore (&self, fact_id: &str) -> Result<()>;
    pub async fn knowledge_entities (&self) -> Result<serde_json::Value>;
    pub async fn knowledge_entity_relationships (
        &self,
        entity_id: &str,
    ) -> Result<serde_json::Value>;
    pub async fn knowledge_timeline (&self) -> Result<serde_json::Value>;
    pub async fn knowledge_update_confidence (&self, fact_id: &str, confidence: f64) -> Result<()>;
    pub async fn queue_message (&self, session_id: &str, text: &str) -> Result<()>;
    pub fn raw_client (&self) -> &Client;
}
```

## `skene/src/api/routes.rs`

> Template for `GET` project verification.
```rust
pub const PROJECT_VERIFICATION_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/verification";
```

> Template for `POST` project verification refresh.
```rust
pub const PROJECT_VERIFICATION_REFRESH_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/verification/refresh";
```

```rust
pub fn project_verification_path (project_id: &str) -> String
```

```rust
pub fn project_verification_url (base_url: &str, project_id: &str) -> String
```

```rust
pub fn project_verification_refresh_path (project_id: &str) -> String
```

```rust
pub fn project_verification_refresh_url (base_url: &str, project_id: &str) -> String
```

## `skene/src/api/sse.rs`

> Manages the global SSE connection to /api/v1/events.
> Runs in a background task, sends parsed events through a channel.
```rust
pub struct SseConnection {
    // kanon:ignore RUST/pub-visibility
    rx: mpsc::Receiver<SseEvent>,
    _handle: tokio::task::JoinHandle<()>,
}
```

```rust
impl SseConnection {
    pub fn connect (client: Client, base_url: &str) -> Self;
    pub async fn next (&mut self) -> Option<SseEvent>;
}
```

## `skene/src/api/streaming.rs`

```rust
pub fn stream_message (
    // kanon:ignore RUST/pub-visibility
    client: Client,
    base_url: &str,
    nous_id: &str,
    session_key: &str,
    text: &str,
) -> mpsc::Receiver<StreamEvent>
```

## `skene/src/api/types/mod.rs`

```rust
pub struct Agent {
    /// Agent identifier.
    pub id: NousId,
    /// Display name: falls back to `id` if absent.
    #[serde(default)]
    pub name: Option<String>,
    /// Model backing this agent.
    #[serde(default)]
    pub model: Option<String>,
    /// Emoji icon for the agent.
    #[serde(default)]
    pub emoji: Option<String>,
}
```

```rust
impl Agent {
    pub fn display_name (&self) -> &str;
}
```

```rust
pub struct Session {
    /// Session identifier.
    pub id: SessionId,
    /// Agent this session belongs to.
    pub nous_id: NousId,
    /// Session key (human-readable slug, not a secret).
    #[serde(rename = "session_key")]
    pub key: String, // kanon:ignore RUST/plain-string-secret
    /// Session status (e.g. "active", "archived").
    #[serde(default)]
    pub status: Option<String>,
    /// Number of messages in the session.
    #[serde(default)]
    pub message_count: u32,
    /// Session type (e.g. "background").
    #[serde(default)]
    pub session_type: Option<String>,
    /// Last-updated timestamp.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// User-assigned display name.
    #[serde(default, alias = "name")]
    pub display_name: Option<String>,
}
```

```rust
impl Session {
    pub fn label (&self) -> &str;
    pub fn is_archived (&self) -> bool;
    pub fn is_interactive (&self) -> bool;
}
```

```rust
pub struct HistoryMessage {
    /// Role: "user", "assistant", or "tool".
    pub role: String,
    /// Message content (text or structured).
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    /// When the message was created.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Model that generated this message (assistant messages only).
    #[serde(default)]
    pub model: Option<String>,
    /// Tool name if this is a tool-result message.
    #[serde(default)]
    pub tool_name: Option<String>,
}
```

```rust
pub struct HistoryResponse {
    /// Messages in chronological order.
    pub messages: Vec<HistoryMessage>,
}
```

```rust
pub struct TurnOutcome {
    /// Final text output.
    pub text: String,
    /// Agent that processed this turn.
    #[serde(rename = "nousId", alias = "nous_id")]
    pub nous_id: NousId,
    /// Session this turn belongs to.
    #[serde(rename = "sessionId", alias = "session_id")]
    pub session_id: SessionId,
    /// Model used for this turn; `None` when the gateway could not resolve it.
    #[serde(default)]
    pub model: Option<String>,
    /// Number of tool calls made.
    #[serde(rename = "toolCalls", alias = "tool_calls", default)]
    pub tool_calls: u32,
    /// Input tokens consumed.
    #[serde(rename = "inputTokens", alias = "input_tokens", default)]
    pub input_tokens: u32,
    /// Output tokens generated.
    #[serde(rename = "outputTokens", alias = "output_tokens", default)]
    pub output_tokens: u32,
    /// Tokens read from cache.
    #[serde(rename = "cacheReadTokens", alias = "cache_read_tokens", default)]
    pub cache_read_tokens: u32,
    /// Tokens written to cache.
    #[serde(rename = "cacheWriteTokens", alias = "cache_write_tokens", default)]
    pub cache_write_tokens: u32,
    /// Error message, if the turn errored.
    #[serde(default)]
    pub error: Option<String>,
}
```

```rust
pub struct PlanStep {
    /// Step index.
    pub id: u32,
    /// Human-readable label.
    pub label: String,
    /// Role responsible for this step.
    pub role: String,
    /// Steps that can run in parallel with this one.
    #[serde(default)]
    pub parallel: Option<Vec<u32>>,
    /// Current status of this step.
    pub status: String,
    /// Result summary after completion.
    #[serde(default)]
    pub result: Option<String>,
}
```

```rust
pub struct Plan {
    /// Plan identifier.
    pub id: PlanId,
    /// Session this plan was proposed in.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Agent that proposed the plan.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Ordered list of plan steps.
    pub steps: Vec<PlanStep>,
    /// Estimated total cost in cents.
    #[serde(rename = "totalEstimatedCostCents", default)]
    pub total_estimated_cost_cents: u32,
    /// Plan status.
    pub status: String,
}
```

```rust
pub enum SseEvent {
    /// SSE connection established.
    Connected,
    /// SSE connection lost (will auto-reconnect).
    Disconnected,
    /// Initial state dump with currently active turns.
    Init {
        /// Turns that are currently in progress.
        active_turns: Vec<ActiveTurn>,
    },
    /// A turn is about to start.
    TurnBefore {
        /// Agent processing the turn.
        nous_id: NousId,
        /// Session the turn belongs to.
        session_id: SessionId,
        /// Turn identifier.
        turn_id: TurnId,
    },
    /// A turn has completed.
    TurnAfter {
        /// Agent that processed the turn.
        nous_id: NousId,
        /// Session the turn belongs to.
        session_id: SessionId,
    },
    /// A tool was invoked during a turn.
    ToolCalled {
        /// Agent invoking the tool.
        nous_id: NousId,
        /// Name of the tool.
        tool_name: String,
    },
    /// A tool invocation failed.
    ToolFailed {
        /// Agent whose tool failed.
        nous_id: NousId,
        /// Name of the failed tool.
        tool_name: String,
        /// Error description.
        error: String,
    },
    /// Agent status changed.
    StatusUpdate {
        /// Agent whose status changed.
        nous_id: NousId,
        /// New status value.
        status: String,
    },
    /// A new session was created.
    SessionCreated {
        /// Agent the session was created for.
        nous_id: NousId,
        /// New session identifier.
        session_id: SessionId,
    },
    /// A session was archived.
    SessionArchived {
        /// Agent the session belongs to.
        nous_id: NousId,
        /// Archived session identifier.
        session_id: SessionId,
    },
    /// Memory distillation is about to start.
    DistillBefore {
        /// Agent undergoing distillation.
        nous_id: NousId,
    },
    /// Memory distillation progressed to a new stage.
    DistillStage {
        /// Agent undergoing distillation.
        nous_id: NousId,
        /// Current distillation stage.
        stage: String,
    },
    /// Memory distillation completed.
    DistillAfter {
        /// Agent that completed distillation.
        nous_id: NousId,
    },
    /// A new checkpoint was created in a planning project.
    CheckpointCreated {
        /// Project the checkpoint belongs to.
        project_id: String,
        /// Identifier of the created checkpoint.
        checkpoint_id: String,
    },
    /// A checkpoint's status changed (approved, skipped, overridden).
    CheckpointUpdated {
        /// Project the checkpoint belongs to.
        project_id: String,
        /// Identifier of the updated checkpoint.
        checkpoint_id: String,
        /// New status value (e.g. "approved", "skipped", "overridden").
        status: String,
    },
    /// Server heartbeat.
    Ping,
    /// Error event from the server.
    Error {
        /// Error message.
        message: String,
    },
}
```

```rust
pub struct ActiveTurn {
    /// Agent processing this turn.
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    /// Session this turn belongs to.
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    /// Turn identifier.
    #[serde(rename = "turnId")]
    pub turn_id: TurnId,
}
```

```rust
pub struct AuthMode {
    /// Authentication mode (e.g. "token", "none").
    pub mode: String,
}
```

```rust
pub struct LoginResponse {
    /// Authentication token.
    pub token: SecretString,
}
```

```rust
pub struct CostSummary {
    /// Total cost across all agents.
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    /// Per-agent cost breakdown.
    #[serde(default)]
    pub agents: Vec<AgentCost>,
}
```

```rust
pub struct AgentCost {
    /// Agent identifier.
    #[serde(rename = "agentId")]
    pub agent_id: NousId,
    /// Total cost for this agent.
    #[serde(rename = "totalCost", default)]
    pub total_cost: f64,
    /// Number of turns processed.
    #[serde(default)]
    pub turns: u32,
}
```

```rust
pub struct DailyResponse {
    /// Daily cost entries.
    pub daily: Vec<DailyEntry>,
}
```

```rust
pub struct DailyEntry {
    /// Date string (YYYY-MM-DD).
    pub date: String,
    /// Cost in dollars.
    pub cost: f64,
    /// Total tokens consumed.
    #[serde(default)]
    pub tokens: u64,
    /// Number of turns.
    #[serde(default)]
    pub turns: u32,
}
```

```rust
pub struct AgentsResponse {
    /// Server returns `{"nous": [...]}`: accept both keys for resilience.
    #[serde(alias = "agents")]
    pub nous: Vec<Agent>,
}
```

```rust
pub struct SessionsResponse {
    /// List of sessions.
    #[serde(alias = "items")]
    pub sessions: Vec<Session>,
}
```

```rust
pub struct NousTool {
    /// Tool name.
    pub name: String,
    /// Whether the tool is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}
```

```rust
pub struct NousToolsResponse {
    /// List of tools.
    pub tools: Vec<NousTool>,
}
```

## `skene/src/api/types/verification.rs`

```rust
pub enum VerificationStatus {
    /// Requirement fully demonstrated.
    Verified,
    /// Some but not all criteria demonstrated.
    PartiallyVerified,
    /// No verification evidence found.
    Unverified,
    /// Verification attempted but explicitly failed.
    Failed,
}
```

```rust
pub enum RequirementPriority {
    /// Blocking — must be verified before release.
    P0,
    /// High priority.
    P1,
    /// Medium priority.
    P2,
    /// Low or nice-to-have.
    P3,
}
```

```rust
pub struct VerificationEvidence {
    /// Human-readable label for this evidence.
    pub label: String,
    /// Path or reference to the evidence artifact.
    pub artifact: String,
}
```

```rust
pub struct VerificationGap {
    /// Description of the missing criteria.
    pub missing_criteria: String,
    /// Suggested action to close the gap.
    pub suggested_action: String,
}
```

```rust
pub struct RequirementVerification {
    /// Requirement identifier.
    // kanon:ignore RUST/primitive-for-domain-id — skene verification type mirrors server-side string IDs directly
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Version tier (e.g., `"v1"`, `"v2"`).
    pub tier: String,
    /// Priority level.
    pub priority: RequirementPriority,
    /// Current verification status.
    pub status: VerificationStatus,
    /// Coverage percentage 0–100.
    pub coverage_pct: u8,
    /// Evidence supporting this requirement.
    pub evidence: Vec<VerificationEvidence>,
    /// Gaps remaining for this requirement.
    pub gaps: Vec<VerificationGap>,
}
```

```rust
pub struct ProjectVerificationResult {
    /// Project identifier.
    // kanon:ignore RUST/primitive-for-domain-id — skene verification type mirrors server-side string IDs directly
    pub project_id: String,
    /// Per-requirement verification results.
    pub requirements: Vec<RequirementVerification>,
    /// ISO 8601 timestamp of the last verification run.
    pub last_verified_at: String,
}
```

## `skene/src/discovery.rs`

```rust
pub struct DiscoveryConfig {
    /// Gateway port to use for generated localhost, LAN, and Tailscale candidates.
    pub port: u16,
    /// Base URLs to probe exactly as configured, before generated LAN candidates.
    pub base_urls: Vec<String>,
    /// LAN hostnames to probe with the `.lan` suffix.
    pub lan_hostnames: Vec<String>,
    /// Tailscale IPs to probe directly.
    pub tailscale_ips: Vec<String>,
    /// Base URLs discovered from environment variables.
    env_base_urls: Vec<String>,
    /// LAN hostnames discovered from environment variables.
    env_lan_hostnames: Vec<String>,
    /// Tailscale IPs discovered from environment variables.
    env_tailscale_ips: Vec<String>,
    /// Base URLs read from the known-hosts file.
    known_hosts_base_urls: Vec<String>,
    /// LAN hostnames read from the known-hosts file.
    known_hosts_lan_hostnames: Vec<String>,
    /// Tailscale IPs read from the known-hosts file.
    known_hosts_tailscale_ips: Vec<String>,
}
```

```rust
impl DiscoveryConfig {
    pub fn new () -> Self;
    pub fn from_env () -> Self;
    pub fn from_env_and_known_hosts () -> Self;
    pub fn with_lan_hostnames <I, S> (mut self, hostnames: I) -> Self where
        I: IntoIterator<Item = S>,
        S: Into<String>,;
    pub fn with_tailscale_ips <I, S> (mut self, ips: I) -> Self where
        I: IntoIterator<Item = S>,
        S: Into<String>,;
    pub fn with_base_urls <I, S> (mut self, urls: I) -> Self where
        I: IntoIterator<Item = S>,
        S: Into<String>,;
    pub fn with_known_hosts_file <P: AsRef<Path>> (mut self, path: P) -> Self;
}
```

```rust
pub async fn discover_server () -> Option<String>
```

```rust
pub async fn discover_server_with_config (config: &DiscoveryConfig) -> Option<String>
```

## `skene/src/events.rs`

```rust
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
```

## `skene/src/id.rs`

```rust
pub struct TurnId(String);
```

```rust
pub struct PlanId(String);
```
