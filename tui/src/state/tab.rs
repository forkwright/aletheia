//! Tab bar state for multi-session tabbed navigation.

use std::collections::HashSet;

use crate::id::{NousId, SessionId, ToolId, TurnId};
use crate::state::chat::{ChatMessage, SavedScrollState, ToolCallInfo};
use crate::state::filter::FilterState;
use crate::state::input::InputState;
use crate::state::ops::OpsState;
use crate::state::view_stack::ViewStack;

/// Unique identifier for a tab, distinct from session IDs.
pub(crate) type TabId = u64;

/// Per-tab snapshot of isolated session state.
#[derive(Debug, Clone)]
pub(crate) struct TabState {
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) focused_session_id: Option<SessionId>,
    pub(crate) input: InputState,
    pub(crate) scroll: SavedScrollState,
    pub(crate) selected_message: Option<usize>,
    pub(crate) tool_expanded: HashSet<ToolId>,
    pub(crate) filter: FilterState,
    pub(crate) view_stack: ViewStack,
    pub(crate) streaming_text: String,
    pub(crate) streaming_thinking: String,
    pub(crate) streaming_tool_calls: Vec<ToolCallInfo>,
    pub(crate) active_turn_id: Option<TurnId>,
    pub(crate) cached_markdown_text: String,
    pub(crate) cached_markdown_lines: Vec<ratatui::text::Line<'static>>,
    pub(crate) ops: OpsState,
}

/// A single tab in the tab bar.
#[derive(Debug, Clone)]
pub(crate) struct Tab {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read in tests for ID uniqueness verification")
    )]
    pub(crate) id: TabId,
    pub(crate) agent_id: NousId,
    pub(crate) session_id: Option<SessionId>,
    pub(crate) title: String,
    pub(crate) unread: bool,
    #[expect(dead_code, reason = "reserved for unsaved-state indicator in tab bar")]
    pub(crate) modified: bool,
    pub(crate) state: TabState,
}

/// Tab bar managing multiple tabs with an active tab index.
#[derive(Debug, Clone)]
pub(crate) struct TabBar {
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    next_id: TabId,
}

impl TabState {
    pub(crate) fn new() -> Self {
        Self {
            messages: Vec::new(),
            focused_session_id: None,
            input: InputState::default(),
            scroll: SavedScrollState::default(),
            selected_message: None,
            tool_expanded: HashSet::new(),
            filter: FilterState::default(),
            view_stack: ViewStack::new(),
            streaming_text: String::new(),
            streaming_thinking: String::new(),
            streaming_tool_calls: Vec::new(),
            active_turn_id: None,
            cached_markdown_text: String::new(),
            cached_markdown_lines: Vec::new(),
            ops: OpsState::default(),
        }
    }
}

impl Tab {
    fn new(id: TabId, agent_id: NousId, title: impl Into<String>) -> Self {
        Self {
            id,
            agent_id,
            session_id: None,
            title: title.into(),
            unread: false,
            modified: false,
            state: TabState::new(),
        }
    }
}

impl TabBar {
    pub(crate) fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
        }
    }

    /// Create a new tab and return its index.
    pub(crate) fn create_tab(&mut self, agent_id: NousId, title: impl Into<String>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab::new(id, agent_id, title);
        self.tabs.push(tab);
        self.tabs.len() - 1
    }

    /// Number of open tabs.
    pub(crate) fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether the tab bar is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Get the active tab, if any.
    pub(crate) fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    /// Get a mutable reference to the active tab.
    pub(crate) fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    /// Switch to the next tab (wrapping).
    pub(crate) fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab (wrapping).
    pub(crate) fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
        }
    }

    /// Jump to tab N (0-indexed). Returns false if out of range.
    pub(crate) fn jump_to(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Close the tab at the given index. Returns the closed tab.
    /// After closing, active index adjusts to stay valid.
    pub(crate) fn close_tab(&mut self, index: usize) -> Option<Tab> {
        if index >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if self.active > index {
            self.active -= 1;
        }
        Some(tab)
    }

    /// Close the active tab. Returns the closed tab.
    pub(crate) fn close_active(&mut self) -> Option<Tab> {
        if self.tabs.is_empty() {
            return None;
        }
        self.close_tab(self.active)
    }

    /// Find a tab by fuzzy-matching on the title. Returns the index.
    pub(crate) fn find_by_title(&self, query: &str) -> Option<usize> {
        let query_lower = query.to_lowercase();

        // Exact match first
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.title.to_lowercase() == query_lower)
        {
            return Some(idx);
        }

        // Prefix match
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.title.to_lowercase().starts_with(&query_lower))
        {
            return Some(idx);
        }

        // Substring match
        self.tabs
            .iter()
            .position(|t| t.title.to_lowercase().contains(&query_lower))
    }

    /// Mark a background tab as having unread messages.
    pub(crate) fn mark_unread(&mut self, agent_id: &NousId, session_id: &SessionId) {
        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            if idx != self.active
                && tab.agent_id == *agent_id
                && tab.session_id.as_ref() == Some(session_id)
            {
                tab.unread = true;
            }
        }
    }

    /// Clear unread on the active tab.
    pub(crate) fn clear_active_unread(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.unread = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_agent_id() -> NousId {
        NousId::from("syn")
    }

    fn test_agent_id2() -> NousId {
        NousId::from("demiurge")
    }

    #[test]
    fn tab_bar_starts_empty() {
        let bar = TabBar::new();
        assert!(bar.is_empty());
        assert_eq!(bar.len(), 0);
        assert!(bar.active_tab().is_none());
    }

    #[test]
    fn create_tab_adds_to_bar() {
        let mut bar = TabBar::new();
        let idx = bar.create_tab(test_agent_id(), "main");
        assert_eq!(idx, 0);
        assert_eq!(bar.len(), 1);
        assert!(!bar.is_empty());
        assert_eq!(bar.active_tab().unwrap().title, "main");
    }

    #[test]
    fn create_multiple_tabs() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "main");
        bar.create_tab(test_agent_id(), "research");
        bar.create_tab(test_agent_id2(), "leather");
        assert_eq!(bar.len(), 3);
        // Tab IDs are unique
        let ids: Vec<TabId> = bar.tabs.iter().map(|t| t.id).collect();
        let unique: HashSet<TabId> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len());
    }

    #[test]
    fn next_tab_wraps() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        bar.create_tab(test_agent_id(), "b");
        bar.create_tab(test_agent_id(), "c");
        assert_eq!(bar.active, 0);
        bar.next_tab();
        assert_eq!(bar.active, 1);
        bar.next_tab();
        assert_eq!(bar.active, 2);
        bar.next_tab();
        assert_eq!(bar.active, 0); // wraps
    }

    #[test]
    fn prev_tab_wraps() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        bar.create_tab(test_agent_id(), "b");
        bar.create_tab(test_agent_id(), "c");
        assert_eq!(bar.active, 0);
        bar.prev_tab();
        assert_eq!(bar.active, 2); // wraps
        bar.prev_tab();
        assert_eq!(bar.active, 1);
    }

    #[test]
    fn jump_to_valid_index() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        bar.create_tab(test_agent_id(), "b");
        bar.create_tab(test_agent_id(), "c");
        assert!(bar.jump_to(2));
        assert_eq!(bar.active, 2);
    }

    #[test]
    fn jump_to_invalid_index() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        assert!(!bar.jump_to(5));
        assert_eq!(bar.active, 0);
    }

    #[test]
    fn close_active_tab_adjusts_index() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        bar.create_tab(test_agent_id(), "b");
        bar.create_tab(test_agent_id(), "c");
        bar.active = 1; // focus on "b"
        let closed = bar.close_active();
        assert_eq!(closed.unwrap().title, "b");
        assert_eq!(bar.len(), 2);
        // Active should stay at 1 (now points to "c")
        assert_eq!(bar.active, 1);
        assert_eq!(bar.active_tab().unwrap().title, "c");
    }

    #[test]
    fn close_last_tab_adjusts_to_previous() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        bar.create_tab(test_agent_id(), "b");
        bar.active = 1;
        bar.close_active();
        assert_eq!(bar.active, 0);
        assert_eq!(bar.active_tab().unwrap().title, "a");
    }

    #[test]
    fn close_only_tab_leaves_empty() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        bar.close_active();
        assert!(bar.is_empty());
        assert_eq!(bar.active, 0);
    }

    #[test]
    fn close_tab_before_active_adjusts_index() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        bar.create_tab(test_agent_id(), "b");
        bar.create_tab(test_agent_id(), "c");
        bar.active = 2; // focus on "c"
        bar.close_tab(0); // close "a"
        assert_eq!(bar.active, 1); // adjusted from 2 to 1
        assert_eq!(bar.active_tab().unwrap().title, "c");
    }

    #[test]
    fn find_by_title_exact() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "main");
        bar.create_tab(test_agent_id(), "research");
        assert_eq!(bar.find_by_title("research"), Some(1));
    }

    #[test]
    fn find_by_title_prefix() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "main");
        bar.create_tab(test_agent_id(), "research-task");
        assert_eq!(bar.find_by_title("res"), Some(1));
    }

    #[test]
    fn find_by_title_case_insensitive() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "Main Session");
        assert_eq!(bar.find_by_title("main"), Some(0));
    }

    #[test]
    fn find_by_title_substring() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "Syn: main");
        bar.create_tab(test_agent_id(), "Syn: research");
        assert_eq!(bar.find_by_title("research"), Some(1));
    }

    #[test]
    fn find_by_title_no_match() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "main");
        assert_eq!(bar.find_by_title("xyz"), None);
    }

    #[test]
    fn mark_unread_background_tab() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "main");
        bar.create_tab(test_agent_id(), "research");
        bar.tabs[1].session_id = Some(SessionId::from("sess2"));
        bar.active = 0;
        bar.mark_unread(&test_agent_id(), &SessionId::from("sess2"));
        assert!(!bar.tabs[0].unread);
        assert!(bar.tabs[1].unread);
    }

    #[test]
    fn mark_unread_does_not_affect_active() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "main");
        bar.tabs[0].session_id = Some(SessionId::from("sess1"));
        bar.active = 0;
        bar.mark_unread(&test_agent_id(), &SessionId::from("sess1"));
        assert!(!bar.tabs[0].unread);
    }

    #[test]
    fn clear_active_unread() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "main");
        bar.tabs[0].unread = true;
        bar.clear_active_unread();
        assert!(!bar.tabs[0].unread);
    }

    #[test]
    fn tab_state_isolation() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "tab1");
        bar.create_tab(test_agent_id(), "tab2");

        // Modify tab1 state
        bar.tabs[0].state.messages.push(ChatMessage {
            role: "user".to_string(),
            text: "hello".to_string(),
            text_lower: "hello".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });
        bar.tabs[0].state.scroll.scroll_offset = 42;
        bar.tabs[0].state.input.text = "typing...".to_string();

        // Tab2 should be unaffected
        assert!(bar.tabs[1].state.messages.is_empty());
        assert_eq!(bar.tabs[1].state.scroll.scroll_offset, 0);
        assert!(bar.tabs[1].state.input.text.is_empty());
    }

    #[test]
    fn next_tab_empty_bar_noop() {
        let mut bar = TabBar::new();
        bar.next_tab();
        assert_eq!(bar.active, 0);
    }

    #[test]
    fn prev_tab_empty_bar_noop() {
        let mut bar = TabBar::new();
        bar.prev_tab();
        assert_eq!(bar.active, 0);
    }

    #[test]
    fn close_tab_out_of_range() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        assert!(bar.close_tab(5).is_none());
        assert_eq!(bar.len(), 1);
    }

    #[test]
    fn close_active_empty_bar() {
        let mut bar = TabBar::new();
        assert!(bar.close_active().is_none());
    }

    #[test]
    fn tab_ids_monotonically_increase() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "a");
        bar.create_tab(test_agent_id(), "b");
        bar.close_tab(0);
        bar.create_tab(test_agent_id(), "c");
        // IDs should be 2, 3 (1 was removed)
        assert!(bar.tabs[0].id < bar.tabs[1].id);
    }

    #[test]
    fn active_tab_mut_modifies() {
        let mut bar = TabBar::new();
        bar.create_tab(test_agent_id(), "orig");
        bar.active_tab_mut().unwrap().title = "modified".to_string();
        assert_eq!(bar.active_tab().unwrap().title, "modified");
    }
}
