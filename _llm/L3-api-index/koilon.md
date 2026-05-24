# L3 API Index: koilon

Crate path: `crates/theatron/koilon`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/app/mod.rs`

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

## `src/command/mod.rs`

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

## `src/error.rs`

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

## `src/events.rs`

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

## `src/lib.rs`

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

## `src/msg.rs`

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

## `src/state/agent.rs`

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

## `src/state/chat.rs`

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

## `src/state/command.rs`

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

## `src/state/filter.rs`

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

## `src/state/input.rs`

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

## `src/state/memory/mod.rs`

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

## `src/state/metrics.rs`

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

## `src/state/notification.rs`

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

## `src/state/ops/state_impl.rs`

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

## `src/state/ops/types.rs`

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

## `src/state/overlay.rs`

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

## `src/state/view_stack.rs`

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

## `src/theme.rs`

```rust
pub static THEME: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(Theme::default);
```

## `src/wizard/mod.rs`

> Returns `true` when the current environment supports a TUI wizard.
> 
> Requires both stdin and stdout to be connected to a TTY.
```rust
pub fn is_tty () -> bool
```

## `src/wizard/state.rs`

```rust
pub struct WizardAnswers {
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
    pub agent_id: String,
    /// Agent display name.
    pub agent_name: String,
    /// Primary model identifier.
    pub model: String,
}
```
