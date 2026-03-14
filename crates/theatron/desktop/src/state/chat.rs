//! Per-agent chat state for a single tab/conversation.
//!
//! `AgentChatState` owns everything a tab needs: messages, streaming turn,
//! input buffer, scroll position, operations pane, and error state. In Dioxus
//! this maps to `Store<AgentChatState>` with field-level subscriptions.

use std::time::Instant;

use compact_str::CompactString;

// ---------------------------------------------------------------------------
// Domain ID newtypes (mirrors theatron-tui id.rs)
// ---------------------------------------------------------------------------

/// Agent identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NousId(CompactString);

impl NousId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: Into<CompactString>> From<T> for NousId {
    fn from(v: T) -> Self {
        Self(v.into())
    }
}

/// Session key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(CompactString);

impl SessionId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: Into<CompactString>> From<T> for SessionId {
    fn from(v: T) -> Self {
        Self(v.into())
    }
}

/// Turn identifier within a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TurnId(CompactString);

impl TurnId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: Into<CompactString>> From<T> for TurnId {
    fn from(v: T) -> Self {
        Self(v.into())
    }
}

/// Tool execution identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolId(CompactString);

impl ToolId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: Into<CompactString>> From<T> for ToolId {
    fn from(v: T) -> Self {
        Self(v.into())
    }
}

// ---------------------------------------------------------------------------
// Chat message (finalized, immutable after append)
// ---------------------------------------------------------------------------

/// A finalized chat message in the conversation history.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub text: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub tool_calls: Vec<ToolCallSummary>,
}

/// Message author role.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    ToolResult,
}

/// Summary of a completed tool call, attached to an assistant message.
#[derive(Debug, Clone)]
pub struct ToolCallSummary {
    pub name: String,
    pub tool_id: ToolId,
    pub duration_ms: Option<u64>,
    pub is_error: bool,
}

// ---------------------------------------------------------------------------
// Streaming turn (mutable, in-progress)
// ---------------------------------------------------------------------------

/// Current streaming turn state. Only one turn streams at a time per tab.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub enum StreamingTurn {
    /// No active streaming turn.
    #[default]
    Idle,
    /// Turn in progress, accumulating deltas.
    Active {
        turn_id: TurnId,
        text: String,
        thinking: String,
        tool_calls: Vec<LiveToolCall>,
        pending_approval: Option<ToolApproval>,
    },
    /// Abort requested, waiting for server acknowledgment.
    Aborting { turn_id: TurnId, reason: String },
}

impl StreamingTurn {
    /// Whether a turn is actively streaming.
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    /// The turn ID if streaming or aborting.
    #[must_use]
    pub fn turn_id(&self) -> Option<&TurnId> {
        match self {
            Self::Idle => None,
            Self::Active { turn_id, .. } | Self::Aborting { turn_id, .. } => Some(turn_id),
        }
    }
}

/// A tool call during a streaming turn, tracking its lifecycle.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum LiveToolCall {
    /// Tool is executing.
    Running {
        name: String,
        tool_id: ToolId,
        started_at: Instant,
    },
    /// Tool needs user approval before proceeding.
    AwaitingApproval {
        name: String,
        tool_id: ToolId,
        approval: ToolApproval,
    },
    /// Tool finished (success or failure).
    Completed {
        name: String,
        tool_id: ToolId,
        duration_ms: u64,
        is_error: bool,
    },
}

impl LiveToolCall {
    /// The tool name regardless of lifecycle phase.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Running { name, .. }
            | Self::AwaitingApproval { name, .. }
            | Self::Completed { name, .. } => name,
        }
    }

    /// The tool ID regardless of lifecycle phase.
    #[must_use]
    pub fn tool_id(&self) -> &ToolId {
        match self {
            Self::Running { tool_id, .. }
            | Self::AwaitingApproval { tool_id, .. }
            | Self::Completed { tool_id, .. } => tool_id,
        }
    }
}

/// Pending tool approval request.
#[derive(Debug, Clone)]
pub struct ToolApproval {
    pub tool_name: String,
    pub tool_id: ToolId,
    pub input_json: String,
    pub risk: String,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Input and scroll state
// ---------------------------------------------------------------------------

/// Input buffer state for the chat input area.
#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
}

/// Scroll position for a chat view.
#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    pub offset: usize,
    pub auto_scroll: bool,
}

// ---------------------------------------------------------------------------
// Operations pane state
// ---------------------------------------------------------------------------

/// The operations pane shows thinking, tool calls, and diffs during a turn.
#[derive(Debug, Clone)]
pub struct OperationsPane {
    pub visible: bool,
    pub width_pct: u16,
    pub focused: bool,
    pub scroll_offset: usize,
    pub selected_item: Option<usize>,
    pub thinking_text: String,
    pub thinking_collapsed: bool,
    pub tool_calls: Vec<OpsToolEntry>,
    pub diffs: Vec<DiffEntry>,
}

impl Default for OperationsPane {
    fn default() -> Self {
        Self {
            visible: false,
            width_pct: 40,
            focused: false,
            scroll_offset: 0,
            selected_item: None,
            thinking_text: String::new(),
            thinking_collapsed: false,
            tool_calls: Vec::new(),
            diffs: Vec::new(),
        }
    }
}

/// A tool call entry in the operations pane.
#[derive(Debug, Clone)]
pub struct OpsToolEntry {
    pub name: String,
    pub input_json: Option<String>,
    pub output: Option<String>,
    pub status: OpsToolStatus,
    pub duration_ms: Option<u64>,
    pub expanded: bool,
}

/// Tool call status in the operations pane.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpsToolStatus {
    Running,
    Complete,
    Failed,
}

/// A file diff extracted from tool output.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub file_path: String,
    pub additions: Vec<String>,
    pub deletions: Vec<String>,
}

// ---------------------------------------------------------------------------
// Chat error
// ---------------------------------------------------------------------------

/// Errors that display in the chat view.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ChatError {
    /// SSE stream broke or server returned an error during a turn.
    StreamFailed { message: String },
    /// History fetch failed (network, auth, not found).
    HistoryLoadFailed { message: String },
    /// Session creation failed.
    SessionCreateFailed { message: String },
    /// Lost connection to the server.
    Disconnected,
}

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StreamFailed { message } => write!(f, "stream failed: {message}"),
            Self::HistoryLoadFailed { message } => write!(f, "history load failed: {message}"),
            Self::SessionCreateFailed { message } => {
                write!(f, "session creation failed: {message}")
            }
            Self::Disconnected => write!(f, "disconnected from server"),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-agent chat state (one per tab)
// ---------------------------------------------------------------------------

/// Complete state for one agent chat tab. In Dioxus, this is the value inside
/// `Store<AgentChatState>`. Each tab component owns its own store instance.
#[derive(Debug, Clone)]
pub struct AgentChatState {
    /// Which agent this tab belongs to.
    pub agent_id: NousId,
    /// Active session within this tab.
    pub session_id: Option<SessionId>,
    /// Generation counter: incremented on every session/agent switch.
    /// Stale API responses compare against this to avoid overwriting
    /// the wrong session's messages.
    pub generation: u64,
    /// Finalized conversation history.
    pub messages: Vec<ChatMessage>,
    /// In-progress streaming turn.
    pub streaming: StreamingTurn,
    /// Chat input buffer.
    pub input: InputState,
    /// Scroll position.
    pub scroll: ScrollState,
    /// Right-side operations pane.
    pub ops: OperationsPane,
    /// Current error, if any. Displayed in the chat view.
    pub error: Option<ChatError>,
    /// Whether this tab has unseen activity (background turn completed).
    pub has_notification: bool,
}

impl AgentChatState {
    /// Create a new empty chat state for an agent.
    #[must_use]
    pub fn new(agent_id: NousId) -> Self {
        Self {
            agent_id,
            session_id: None,
            generation: 0,
            messages: Vec::new(),
            streaming: StreamingTurn::Idle,
            input: InputState::default(),
            scroll: ScrollState::default(),
            ops: OperationsPane::default(),
            error: None,
            has_notification: false,
        }
    }

    /// Bump the generation counter. Call on every session or agent switch.
    /// Returns the new generation for passing to async fetches.
    #[must_use]
    pub fn bump_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Check whether a fetch result matches the current generation.
    #[must_use]
    pub fn is_current_generation(&self, expected: u64) -> bool {
        self.generation == expected
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn new_agent_chat_state_is_idle() {
        let state = AgentChatState::new(NousId::from("syn"));
        assert_eq!(state.agent_id.as_str(), "syn");
        assert!(state.session_id.is_none());
        assert_eq!(state.generation, 0);
        assert!(state.messages.is_empty());
        assert!(!state.streaming.is_active());
        assert!(state.error.is_none());
    }

    #[test]
    fn bump_generation_increments_and_returns() {
        let mut state = AgentChatState::new(NousId::from("syn"));
        assert_eq!(state.bump_generation(), 1);
        assert_eq!(state.bump_generation(), 2);
        assert_eq!(state.generation, 2);
    }

    #[test]
    fn is_current_generation_true_when_matching() {
        let mut state = AgentChatState::new(NousId::from("syn"));
        let current = state.bump_generation();
        assert!(state.is_current_generation(current));
    }

    #[test]
    fn is_current_generation_false_when_stale() {
        let mut state = AgentChatState::new(NousId::from("syn"));
        let old = state.bump_generation();
        let _ = state.bump_generation();
        assert!(!state.is_current_generation(old));
    }

    #[test]
    fn streaming_turn_idle_has_no_turn_id() {
        let turn = StreamingTurn::Idle;
        assert!(turn.turn_id().is_none());
        assert!(!turn.is_active());
    }

    #[test]
    fn streaming_turn_active_has_turn_id() {
        let turn = StreamingTurn::Active {
            turn_id: TurnId::from("t1"),
            text: String::new(),
            thinking: String::new(),
            tool_calls: Vec::new(),
            pending_approval: None,
        };
        assert!(turn.is_active());
        assert_eq!(turn.turn_id().unwrap().as_str(), "t1");
    }

    #[test]
    fn streaming_turn_aborting_has_turn_id() {
        let turn = StreamingTurn::Aborting {
            turn_id: TurnId::from("t2"),
            reason: "user cancelled".to_string(),
        };
        assert!(!turn.is_active());
        assert_eq!(turn.turn_id().unwrap().as_str(), "t2");
    }

    #[test]
    fn live_tool_call_name_and_id_accessors() {
        let tc = LiveToolCall::Running {
            name: "read_file".to_string(),
            tool_id: ToolId::from("tc1"),
            started_at: Instant::now(),
        };
        assert_eq!(tc.name(), "read_file");
        assert_eq!(tc.tool_id().as_str(), "tc1");

        let tc = LiveToolCall::Completed {
            name: "write_file".to_string(),
            tool_id: ToolId::from("tc2"),
            duration_ms: 200,
            is_error: false,
        };
        assert_eq!(tc.name(), "write_file");
        assert_eq!(tc.tool_id().as_str(), "tc2");
    }

    #[test]
    fn chat_error_display() {
        let err = ChatError::StreamFailed {
            message: "timeout".to_string(),
        };
        assert_eq!(format!("{err}"), "stream failed: timeout");

        let err = ChatError::Disconnected;
        assert_eq!(format!("{err}"), "disconnected from server");
    }

    #[test]
    fn operations_pane_default() {
        let ops = OperationsPane::default();
        assert!(!ops.visible);
        assert_eq!(ops.width_pct, 40);
        assert!(ops.tool_calls.is_empty());
    }

    #[test]
    fn input_state_default_is_empty() {
        let input = InputState::default();
        assert!(input.text.is_empty());
        assert_eq!(input.cursor, 0);
        assert!(input.history.is_empty());
    }

    #[test]
    fn scroll_state_default() {
        let scroll = ScrollState::default();
        assert_eq!(scroll.offset, 0);
        assert!(!scroll.auto_scroll);
    }

    #[test]
    fn nous_id_equality() {
        let a = NousId::from("syn");
        let b = NousId::from("syn");
        let c = NousId::from("demiurge");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
