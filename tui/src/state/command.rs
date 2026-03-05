/// Command palette state.
#[derive(Debug, Default)]
pub struct CommandPaletteState {
    pub input: String,
    pub cursor: usize,
    pub suggestions: Vec<crate::command::ScoredCommand>,
    pub selected: usize,
    pub active: bool,
}

/// Selection context for context-aware status bar hints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SelectionContext {
    #[default]
    None,
    UserMessage { index: usize },
    AgentResponse { index: usize, has_code: bool, has_links: bool },
    ToolCall { index: usize, tool_id: String, needs_approval: bool },
    SessionListItem { index: usize },
}
