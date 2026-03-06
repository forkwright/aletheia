/// Command palette state.
#[derive(Debug, Default)]
pub struct CommandPaletteState {
    pub input: String,
    pub cursor: usize,
    pub suggestions: Vec<crate::command::Suggestion>,
    pub selected: usize,
    pub active: bool,
}

/// Selection context for context-aware status bar hints.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(dead_code, reason = "variants reserved for context-aware keybind hints")]
pub enum SelectionContext {
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
