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
#[expect(
    dead_code,
    reason = "variants reserved for context-aware keybind hints"
)]
#[non_exhaustive]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_palette_default_inactive() {
        let state = CommandPaletteState::default();
        assert!(!state.active);
        assert!(state.input.is_empty());
        assert_eq!(state.cursor, 0);
        assert_eq!(state.selected, 0);
        assert!(state.suggestions.is_empty());
    }

    #[test]
    fn selection_context_default_is_none() {
        let ctx = SelectionContext::default();
        assert_eq!(ctx, SelectionContext::None);
    }

    #[test]
    fn selection_context_variants_distinct() {
        let none = SelectionContext::None;
        let user = SelectionContext::UserMessage { index: 0 };
        let agent = SelectionContext::AgentResponse {
            index: 0,
            has_code: false,
            has_links: false,
        };
        assert_ne!(none, user);
        assert_ne!(user, agent);
    }
}
