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
#[expect(dead_code, reason = "variants used when selection tracking is wired")]
pub enum SelectionContext {
    #[default]
    None,
    UserMessage,
    AgentResponse,
    ToolCall,
    SessionList,
}
