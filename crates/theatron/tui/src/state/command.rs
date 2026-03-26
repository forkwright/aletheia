/// Inline slash-command autocomplete triggered by `/`.
#[derive(Debug, Default, Clone)]
pub struct SlashCompleteState {
    pub(crate) active: bool,
    pub(crate) query: String,
    pub(crate) suggestions: Vec<SlashSuggestion>,
    pub(crate) cursor: usize,
}

/// A single slash-command suggestion entry.
#[derive(Debug, Clone)]
pub struct SlashSuggestion {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) execute_as: String,
}

/// Command palette state.
#[derive(Debug, Default)]
pub struct CommandPaletteState {
    pub(crate) input: String,
    pub(crate) cursor: usize,
    pub(crate) suggestions: Vec<crate::command::Suggestion>,
    pub(crate) selected: usize,
    pub(crate) active: bool,
}

/// Selection context for context-aware status bar hints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(
    dead_code,
    reason = "variants reserved for context-aware keybind hints"
)]
#[non_exhaustive]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_complete_default_inactive() {
        let state = SlashCompleteState::default();
        assert!(!state.active);
        assert!(state.query.is_empty());
        assert_eq!(state.cursor, 0);
        assert!(state.suggestions.is_empty());
    }

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
